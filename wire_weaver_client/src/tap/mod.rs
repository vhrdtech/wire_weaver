//! The link tap on the host (TR-14): every client event loop reports what crosses its link to a [Tap], one per
//! link, made by the [TapSource] of [ClientConfig::tap](crate::ClientConfig::tap); without one nothing is copied.
//! `Command::RegisterTracer`'s [TraceEvent]s come from the same place. `ww_tap_decoder` (in the ww_tools repository)
//! reads the tap's records back: it links frames to messages, pairs replies with requests and counts statistics.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::mpsc;
use wire_weaver::shrink_wrap::{RefVec, UNib32};
pub use ww_tap::{Dir, LinkEvent, Session, Tap};

use crate::tracing::TraceEvent;

#[cfg(test)]
mod tests;

/// Makes the [Tap] of each link a client opens, [ClientConfig::tap](crate::ClientConfig::tap).
#[derive(Clone)]
pub struct TapSource(Arc<Source>);

/// Behind an [Arc] so that [TapSource] is a thin pointer: `Command::Connect` carries it.
struct Source {
    make: Box<dyn Fn() -> Box<dyn Tap + Send> + Send + Sync>,
    lost: Option<Lost>,
}

/// See [TapSource::on_lost].
type Lost = Box<dyn Fn(&str) + Send + Sync>;

impl TapSource {
    /// A source that calls `link` once for each link.
    pub fn new(link: impl Fn() -> Box<dyn Tap + Send> + Send + Sync + 'static) -> Self {
        TapSource(Arc::new(Source {
            make: Box::new(link),
            lost: None,
        }))
    }

    /// This source's taps, and `lost` called when one panics: the link drops that tap and goes on without it, and
    /// `lost` gets why (the tap's owner marks its recording broken).
    pub fn on_lost(self, lost: impl Fn(&str) + Send + Sync + 'static) -> Self {
        TapSource(Arc::new(Source {
            make: Box::new(move || (self.0.make)()),
            lost: Some(Box::new(lost)),
        }))
    }

    /// A new tap, `None` when making it panicked.
    fn link(&self) -> Option<Box<dyn Tap + Send>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.0.make)())) {
            Ok(tap) => Some(tap),
            Err(_) => {
                self.lost("making the tap panicked");
                None
            }
        }
    }

    /// Tells the source's owner that a tap of it is gone.
    fn lost(&self, why: &str) {
        tracing::warn!("a link's tap is lost: {why}; the link goes on without it");
        if let Some(lost) = &self.0.lost
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lost(why))).is_err()
        {
            tracing::warn!("a tap source's `lost` panicked too");
        }
    }
}

impl core::fmt::Debug for TapSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TapSource")
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
    /// Where [State::tap] came from: told when it panics
    source: Option<TapSource>,
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
        let tap = source.and_then(TapSource::link);
        ClientTap(Arc::new(Shared {
            has_tap: AtomicBool::new(tap.is_some()),
            tracing: AtomicBool::new(false),
            state: Mutex::new(State {
                tap,
                source: source.cloned(),
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
        st.tap = source.and_then(TapSource::link);
        st.source = source.cloned();
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
    /// `f` on the tap, if there is one. A tap that panics (a capture host's bug) is dropped, its source told
    /// ([TapSource::on_lost]), instead of taking the link's event loop down with it.
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
                shared.has_tap.store(false, Ordering::Relaxed);
                let tap = self.tap.take();
                // its drop is the host's code too
                let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(tap)));
                let why = match dropped {
                    Ok(()) => "the tap panicked",
                    Err(_) => "the tap panicked, and again when dropped",
                };
                if let Some(source) = &self.source {
                    source.lost(why);
                }
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

/// Framer configuration of `ww_link::UsbFraming`, as the client's USB sessions report it: `U2Head`, no checksum,
/// no prefix.
pub const FRAMED_USB: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::None,
};

/// Framer configuration of `ww_link::UsbSeqFraming`: [FRAMED_USB] with the `SeqPrefix` diagnostic.
pub const FRAMED_USB_SEQ: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::Seq,
};

/// Framer configuration of `ww_link::UdpFraming`: `SeqPrefix`, `U2Head`, CRC-32 on every message.
pub const FRAMED_UDP: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::Crc32IsoHdlc { full: true },
    tail: ww_tap::FramerTail::None,
    stream: false,
    prefix: ww_tap::FramerPrefix::Seq,
};

/// Framer configuration of stream media (RTT) with `ww_link`'s setup: `U2Head`, no checksum, chunks of a stream.
pub const STREAM_PLAIN: ww_tap::Framer = ww_tap::Framer::U2 {
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
