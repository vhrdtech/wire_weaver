//! Wire types of counted and reliable streams (API-30), see `docs/api/stream_reliability.md`.
//!
//! The state machines that use them live in the `ww_stream` crate.

use wire_weaver::prelude::*;
use ww_si::Second;

/// Per-event head of a counted or reliable stream ([EventKind::StreamItem](crate::EventKind::StreamItem)): where the
/// event's data sits in the stream, how much the device dropped right before it and when it was made.
///
/// Positions count units the device names when it sends (bytes, transactions, sample sets), from the start of the
/// session. The next event's `pos` is `pos + units + lost` of it, so the host tells device drops (`lost`), link
/// losses (a jump `lost` doesn't explain) and duplicates (`pos` lower than expected) apart without knowing anything
/// about the items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamHead {
    /// Position of the event's first unit since the session started. Below 2⁵⁸, see [StreamHead::MAX_POS].
    pub pos: u64,
    /// Units the device dropped right before this event; `pos` counts them.
    pub lost: Option<u64>,
    /// Device time of the event's first unit (`ww_timebase` clock).
    pub t: Option<Second>,
    /// Rare markers.
    pub ext: Option<HeadExt>,
}

impl StreamHead {
    /// Largest position the head can carry: 2 bits in the first byte and up to 7 more bytes.
    pub const MAX_POS: u64 = (1 << 58) - 1;

    /// Head with a position only.
    pub const fn at(pos: u64) -> Self {
        StreamHead {
            pos,
            lost: None,
            t: None,
            ext: None,
        }
    }

    /// `lost` as a number, 0 when not there.
    pub fn lost_units(&self) -> u64 {
        self.lost.unwrap_or(0)
    }
}

/// Rare markers of a [StreamHead], evolvable: fields can be appended later.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq, Default)
)]
pub struct HeadExt {
    /// The device's clock restarted (a reboot) before this event: timestamps from here on belong to this epoch.
    pub clock_epoch: Option<UVlq32>,
}
