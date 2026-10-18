//! The link tap on the host (TR-14): every client event loop records what crosses its link into a
//! [Recorder](ww_tap::Recorder), the process-wide [default_recorder] unless the config names one
//! ([ClientConfig::tap](crate::ClientConfig::tap)). Nothing is copied while nobody subscribes: a consumer
//! subscribes to the recorder and gets `ww_logbook` blocks of sessions, link events, messages and (opt in) frames.

use std::sync::LazyLock;

use tokio::sync::mpsc;
use wire_weaver::shrink_wrap::DeserializeShrinkWrap;
pub use ww_tap::{
    Chunk, Dir, Layers, LinkEvent, Recorder, RecorderConfig, RecorderLink, Session, Subscription,
    Tap, track_number, track_of,
};
use ww_tap::{Message, ids};

use crate::tracing::TraceEvent;

pub mod decode;
#[cfg(all(test, feature = "in_process"))]
mod e2e_tests;
mod reassembly;
pub mod stats;

pub use decode::{ApiInfo, Check, Decoded, Decoder, DecoderConfig, FrameInfo, MessageInfo, What};
pub use reassembly::{FrameOutcome, FramePart, PieceInfo, PieceOutcome, Reassembled};
pub use stats::{DirStats, LinkStats, PathStats, Stats};

static DEFAULT: LazyLock<Recorder> = LazyLock::new(Recorder::default);

/// The recorder of every client that doesn't name one.
pub fn default_recorder() -> &'static Recorder {
    &DEFAULT
}

/// [TraceEvent]s of one link out of the recorder's blocks, for `Command::RegisterTracer`: requests and events are its
/// data messages, connected, disconnected and errors its link events. Unsubscribes once the receiver is gone.
pub(crate) fn trace_events(
    link: u32,
    tx: mpsc::UnboundedSender<TraceEvent>,
) -> impl FnMut(&Chunk) -> bool + Send + 'static {
    move |chunk| {
        if tx.is_closed() {
            return false;
        }
        let block = chunk.block();
        if block.kind != ww_logbook::BlockKind::Data {
            return true;
        }
        for r in block.records().flatten() {
            let Some((l, stream)) = track_of(r.track) else {
                continue;
            };
            if l != link {
                continue;
            }
            let ev = match stream {
                ids::TX_MESSAGES | ids::RX_MESSAGES => {
                    let Ok(m) = Message::from_ww_bytes(r.item) else {
                        continue;
                    };
                    if m.kind > ww_link::Kind::Data2 as u8 {
                        continue;
                    }
                    let bytes = m.bytes.as_slice().to_vec();
                    if stream == ids::TX_MESSAGES {
                        TraceEvent::Request { bytes }
                    } else if bytes.is_empty() {
                        continue;
                    } else {
                        TraceEvent::Event { bytes }
                    }
                }
                ids::EVENTS => match LinkEvent::from_ww_bytes(r.item) {
                    Ok(LinkEvent::LinkReady) => TraceEvent::Connected {},
                    Ok(LinkEvent::Disconnected {
                        reason,
                        keep_streams,
                    }) => TraceEvent::Disconnected {
                        reason: reason.to_string(),
                        keep_streams,
                    },
                    Ok(LinkEvent::Error { what }) => TraceEvent::Error {
                        reason: what.to_string(),
                    },
                    _ => continue,
                },
                _ => continue,
            };
            if tx.send(ev).is_err() {
                return false;
            }
        }
        true
    }
}

/// Framer configuration of frame-based media with `ww_link`'s USB / UDP setup: `U2Head`, CRC-16 on split messages.
pub(crate) const FRAMED_CRC16: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::Crc16IbmSdlc { full: false },
    tail: ww_tap::FramerTail::None,
    stream: false,
};

/// Framer configuration of stream media (RTT) with `ww_link`'s setup: `U2Head`, no checksum, chunks of a stream.
#[allow(dead_code)] // RTT only
pub(crate) const STREAM_PLAIN: ww_tap::Framer = ww_tap::Framer::U2 {
    checksum: ww_tap::FramerChecksum::None,
    tail: ww_tap::FramerTail::None,
    stream: true,
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
pub(crate) struct LinkEvents(Option<RecorderLink>);

impl LinkEvents {
    pub fn set(&mut self, link: RecorderLink) {
        self.0 = Some(link);
    }

    pub fn link(&self) -> Option<&RecorderLink> {
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
