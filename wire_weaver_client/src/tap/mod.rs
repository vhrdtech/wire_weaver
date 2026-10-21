//! The link tap on the host (TR-14): every client event loop records what crosses its link into a
//! [Recorder], the process-wide [default_recorder] unless the config names one
//! ([ClientConfig::tap](crate::ClientConfig::tap)). Nothing is copied while nobody subscribes: a consumer
//! subscribes to the recorder and gets `ww_tape` blocks of sessions, link events, messages and (opt in) frames.

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
pub use stats::{DirStats, LinkStats, PathStats, PrefixCounts, Stats};
pub use ww_framer::prefix::{FrameCtx, FrameReport};

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
        if block.kind != ww_tape::BlockKind::Data {
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
