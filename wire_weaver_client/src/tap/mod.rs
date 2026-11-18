//! The link tap on the host (TR-14): every client event loop reports what crosses its link to a [Tap], one per
//! link, made by the [TapSource] of [ClientConfig::tap](crate::ClientConfig::tap); without one nothing is copied.
//! `Command::RegisterTracer`'s [TraceEvent]s come from the same place. A [Recorder] is a [TapSource]: it batches
//! what its links report into `ww_tape` blocks for `ww monitor`, `ww stats` and `ww capture`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::mpsc;
use wire_weaver::shrink_wrap::{RefVec, UNib32};
pub use ww_tap::{
    Chunk, Dir, Layers, LinkEvent, Recorder, RecorderConfig, RecorderLink, Session, Subscription,
    Tap, track_number, track_of,
};

use crate::tracing::TraceEvent;

pub mod decode;
#[cfg(all(test, feature = "in_process"))]
mod e2e_tests;
mod reassembly;
pub mod stats;
#[cfg(test)]
mod tests;

pub use decode::{
    ApiInfo, Check, Decoded, Decoder, DecoderConfig, FrameInfo, MessageInfo, What,
    session_wire_version,
};
pub use reassembly::{FrameOutcome, FramePart, PieceInfo, PieceOutcome, Reassembled};
pub use stats::{DirStats, LinkStats, PathStats, PrefixCounts, Stats};
pub use ww_framer::prefix::{FrameCtx, FrameReport};

/// Makes the [Tap] of each link a client opens, [ClientConfig::tap](crate::ClientConfig::tap).
#[derive(Clone)]
pub struct TapSource(Arc<MakeTap>);

/// Boxed again so that [TapSource] is a thin pointer: `Command::Connect` carries it.
type MakeTap = Box<dyn Fn() -> Box<dyn Tap + Send> + Send + Sync>;

impl TapSource {
    /// A source that calls `link` once for each link.
    pub fn new(link: impl Fn() -> Box<dyn Tap + Send> + Send + Sync + 'static) -> Self {
        TapSource(Arc::new(Box::new(link)))
    }

    fn link(&self) -> Box<dyn Tap + Send> {
        (self.0)()
    }
}

impl core::fmt::Debug for TapSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TapSource")
    }
}

/// Every link recorded into the recorder.
impl From<Recorder> for TapSource {
    fn from(recorder: Recorder) -> Self {
        TapSource::new(move || Box::new(recorder.link()))
    }
}

/// The tap of one client link: the [TapSource]'s [Tap] and the tracers of `Command::RegisterTracer`. Clones share
/// the link: the event loop's tx and rx tasks and the transport's halves report to it.
#[derive(Clone, Default)]
pub(crate) struct ClientTap(Arc<Shared>);

#[derive(Default)]
struct Shared {
    /// A [TapSource] made a tap: frames and sessions are of use only to it. Changed by [ClientTap::set_tap].
    has_tap: AtomicBool,
    /// Tracers are registered: messages and link events are wanted.
    tracing: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    tap: Option<Box<dyn Tap + Send>>,
    tracers: Vec<mpsc::UnboundedSender<TraceEvent>>,
    /// The newest session, given again with the device's `DeviceInfo` once the link is set up, and to a tap set
    /// later ([ClientTap::set_tap]).
    session: Option<SessionHead>,
    /// The device's `DeviceInfo` bytes once the link is set up, for a tap set later.
    device_info: Option<Vec<u8>>,
}

/// A [Session] without its `device_info`, owned.
struct SessionHead {
    side: ww_tap::Side,
    medium: ww_tap::Medium,
    framer: ww_tap::Framer,
    frame_size: UNib32,
    peer: String,
}

impl SessionHead {
    fn with_info<'a>(&'a self, device_info: &'a [u8]) -> Session<'a> {
        Session {
            side: self.side,
            medium: self.medium,
            framer: self.framer,
            frame_size: self.frame_size,
            peer: &self.peer,
            device_info: RefVec::Slice { slice: device_info },
        }
    }
}

impl ClientTap {
    /// The tap of a new link: `source`'s, else tracers only.
    pub fn new(source: Option<&TapSource>) -> Self {
        let tap = source.map(TapSource::link);
        ClientTap(Arc::new(Shared {
            has_tap: AtomicBool::new(tap.is_some()),
            tracing: AtomicBool::new(false),
            state: Mutex::new(State {
                tap,
                ..State::default()
            }),
        }))
    }

    /// The state, also after a panic while it was held: a tap that panics is dropped ([State::call]), the rest is
    /// always whole.
    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `Command::RegisterTracer`: requests, events and link changes go to `tx` until its receiver is gone.
    pub fn add_tracer(&self, tx: mpsc::UnboundedSender<TraceEvent>) {
        self.state().tracers.push(tx);
        self.0.tracing.store(true, Ordering::Relaxed);
    }

    /// Link setup is done: the newest session again, now with the device's `DeviceInfo` bytes, then
    /// [LinkEvent::LinkReady].
    pub fn link_ready(&mut self, device_info: &[u8]) {
        {
            let mut st = self.state();
            st.device_info = Some(device_info.to_vec());
            if let Some(s) = st.session.take() {
                st.call(&self.0, |tap| tap.session(&s.with_info(device_info)));
                st.session = Some(s);
            }
        }
        self.link_event(&LinkEvent::LinkReady);
    }

    /// `Command::SetTap`: the link is reported to a tap of `source` from now on, or to none. The new tap first gets
    /// what it missed of the link: its newest session (with the device's `DeviceInfo` once the link is set up) and,
    /// for a link already set up, [LinkEvent::LinkReady]. A tap replaced is dropped without a further call.
    pub fn set_tap(&self, source: Option<&TapSource>) {
        let mut st = self.state();
        st.tap = source.map(TapSource::link);
        self.0.has_tap.store(st.tap.is_some(), Ordering::Relaxed);
        if let Some(s) = st.session.take() {
            let info = st.device_info.take();
            st.call(&self.0, |tap| {
                tap.session(&s.with_info(info.as_deref().unwrap_or_default()));
                if info.is_some() {
                    tap.link_event(&LinkEvent::LinkReady);
                }
            });
            (st.session, st.device_info) = (Some(s), info);
        }
    }

    /// `ev` to every tracer; those whose receiver is gone are dropped.
    fn trace(&self, st: &mut State, ev: TraceEvent) {
        st.tracers.retain(|tx| tx.send(ev.clone()).is_ok());
        if st.tracers.is_empty() {
            self.0.tracing.store(false, Ordering::Relaxed);
        }
    }
}

impl State {
    /// `f` on the tap, if there is one. A tap that panics (a capture host's bug) is dropped with a warning instead
    /// of taking the link's event loop down with it.
    fn call<R: Default>(
        &mut self,
        shared: &Shared,
        f: impl FnOnce(&mut (dyn Tap + Send)) -> R,
    ) -> R {
        let Some(tap) = self.tap.as_deref_mut() else {
            return R::default();
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(tap))) {
            Ok(r) => r,
            Err(_) => {
                tracing::warn!("a link's tap panicked: the link goes on without it");
                self.tap = None;
                shared.has_tap.store(false, Ordering::Relaxed);
                R::default()
            }
        }
    }
}

impl Tap for ClientTap {
    fn wants_frames(&self) -> bool {
        self.0.has_tap.load(Ordering::Relaxed) && self.state().call(&self.0, |t| t.wants_frames())
    }

    fn wants_messages(&self) -> bool {
        self.0.tracing.load(Ordering::Relaxed)
            || (self.0.has_tap.load(Ordering::Relaxed)
                && self.state().call(&self.0, |t| t.wants_messages()))
    }

    fn session(&mut self, session: &Session<'_>) {
        // kept without a tap too: one set later starts with it
        let mut st = self.state();
        st.session = Some(SessionHead {
            side: session.side,
            medium: session.medium,
            framer: session.framer,
            frame_size: session.frame_size,
            peer: session.peer.to_owned(),
        });
        st.call(&self.0, |tap| tap.session(session));
    }

    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        if self.0.has_tap.load(Ordering::Relaxed) {
            self.state().call(&self.0, |tap| tap.frame(dir, bytes));
        }
    }

    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        let mut st = self.state();
        st.call(&self.0, |tap| {
            if tap.wants_messages() {
                tap.message(dir, kind, bytes);
            }
        });
        // tracers see the data messages: requests out, events (all but empty ones) in
        if st.tracers.is_empty() || kind > ww_link::Kind::Data2 as u8 {
            return;
        }
        let ev = match dir {
            Dir::Tx => TraceEvent::Request {
                bytes: bytes.to_vec(),
            },
            Dir::Rx if bytes.is_empty() => return,
            Dir::Rx => TraceEvent::Event {
                bytes: bytes.to_vec(),
            },
        };
        self.trace(&mut st, ev);
    }

    fn link_event(&mut self, event: &LinkEvent<'_>) {
        let mut st = self.state();
        st.call(&self.0, |tap| tap.link_event(event));
        if st.tracers.is_empty() {
            return;
        }
        let ev = match event {
            LinkEvent::LinkReady => TraceEvent::Connected {},
            LinkEvent::Disconnected {
                reason,
                keep_streams,
            } => TraceEvent::Disconnected {
                reason: (*reason).to_owned(),
                keep_streams: *keep_streams,
            },
            LinkEvent::Error { what } => TraceEvent::Error {
                reason: (*what).to_owned(),
            },
            _ => return,
        };
        self.trace(&mut st, ev);
    }
}

/// Framer configuration of `ww_link::UsbFraming`: `U2Head`, no checksum, no prefix.
pub(crate) const FRAMED_USB: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::None,
};

/// Framer configuration of `ww_link::UsbSeqFraming`: [FRAMED_USB] with the `SeqPrefix` diagnostic.
#[allow(dead_code)] // USB only
pub(crate) const FRAMED_USB_SEQ: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::Seq,
};

/// Framer configuration of `ww_link::UdpFraming`: `SeqPrefix`, `U2Head`, CRC-32 on every message.
#[allow(dead_code)] // UDP only
pub(crate) const FRAMED_UDP: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::Crc32IsoHdlc { full: true },
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::Seq,
};

/// Framer configuration of stream media (RTT) with `ww_link`'s setup: `U2Head`, no checksum, chunks of a stream.
#[allow(dead_code)] // RTT only
pub(crate) const STREAM_PLAIN: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: true,
    prefix: ww_tap::FramerPrefix::None,
};

/// A host side session record.
pub(crate) fn host_session<'i>(
    medium: ww_tap::Medium,
    framer: ww_tap::Framer,
    frame_size: usize,
    peer: &'i str,
) -> Session<'i> {
    Session {
        side: ww_tap::Side::Host,
        medium,
        framer,
        frame_size: wire_weaver::shrink_wrap::UNib32(frame_size as u32),
        peer,
        device_info: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] },
    }
}

/// Link events of the cores, into the link's tap; nothing before the link exists.
#[derive(Default)]
pub(crate) struct LinkEvents(Option<ClientTap>);

impl LinkEvents {
    pub fn set(&mut self, link: ClientTap) {
        self.0 = Some(link);
    }

    pub fn link(&self) -> Option<&ClientTap> {
        self.0.as_ref()
    }

    pub fn link_ready(&mut self, device_info: &[u8]) {
        if let Some(l) = &mut self.0 {
            l.link_ready(device_info);
        }
    }

    pub fn disconnected(&mut self, reason: &str, keep_streams: bool) {
        self.0.link_event(&LinkEvent::Disconnected {
            reason,
            keep_streams,
        });
    }

    pub fn error(&mut self, what: &str) {
        self.0.link_event(&LinkEvent::Error { what });
    }
}
