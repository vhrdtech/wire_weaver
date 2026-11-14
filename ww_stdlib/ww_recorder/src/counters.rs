use wire_weaver::prelude::*;
use ww_si::units::{Block, Event, Item, Record, Segment, Session};
use ww_si::{Bytes, Count, Millis, Second};

/// Totals, see [Recorder::counters]. Numbers are variable length: an idle recorder's answer is a few bytes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct RecorderCounters {
    /// Data and meta bytes written since the format.
    pub bytes_written: Bytes<UVlq64>,
    /// Data segments the ring went round and overwrote since the format; with `ring_segments` the overwritten
    /// span.
    pub segments_reused: Count<Segment, UVlq64>,
    pub ring_segments: Count<Segment, UVlq64>,
    /// Blocks and records a mount found torn by a power cut or damaged by the card, this boot and since the
    /// format.
    pub torn_blocks: Count<Block, UVlq32>,
    pub torn_records: Count<Record, UVlq32>,
    /// Units lost in all tracks, this boot.
    pub dropped: Count<Item, UVlq64>,
    /// Writes slower than the stall threshold, and the longest, this boot.
    pub card_stalls: Count<Event, UVlq32>,
    pub worst_stall: Millis<UVlq32>,
    /// Write errors of the card, this boot.
    pub card_errors: Count<Event, UVlq32>,
    /// Sessions since the format (this one included).
    pub sessions: Count<Session, UVlq32>,
    /// Device time of the oldest data still in the ring, `None` if empty or unknown.
    pub oldest: Option<Second>,
}
