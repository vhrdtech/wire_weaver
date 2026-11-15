//! The event time codec: starts, ends and flags of an events chunk, read in place on the chunk's bytes.
//!
//! **Starts** ([ColumnKind::Starts], codec [STARTS]) are in one of two time modes, the chunk head's time mode
//! ([TimeMode]):
//!
//! - **Implicit** (0): item j starts at `first tick + j × period`; the column is the period (i64). A chunk whose
//!   starts are evenly spaced takes it, a chunk of one item too (period 0).
//! - **Linear** (1): a linear frame of reference per group of 128 items. The column holds a 25-byte record per
//!   group, then each group's residuals packed at its width ([bitpack](crate::bitpack)):
//!
//! ```text
//! per group: first tick i64 (its first item's start, the search key), slope i64 (Q16 ticks per item, 0 to 2^55),
//!            residual minimum i32 (at most 0), width u8 (0 to 64), offset u32 (of the group's residuals, from the
//!            end of the records)
//! then:      residuals, group after group, 128 × width bits each (the last group: its items)
//! ```
//!
//! Item j of a group starts at `first + rmin + ((j × slope) >> 16) + residual[j]`. The encoder fits the slope
//! through the group's first and last item and keeps it only when it gives a narrower width than slope 0, so periodic
//! starts cost their jitter, not their period. It is integer-only: the same starts give the same bytes.
//!
//! **Ends** follow the chunk head's [EndMode]: [EndMode::Absent] (every item an instant, no column),
//! [EndMode::NextStart] (item i ends where item i + 1 starts, the last item at the head's latest end, no column) or
//! [EndMode::Durations] ([ColumnKind::Ends], codec [DURATIONS]): per group a minimum (u64), width (u8) and offset
//! (u32), then `end - start - minimum` packed. The minimum plus 2^width - 1 bounds the group's longest item.
//!
//! **Flags** ([ColumnKind::Flags], codec [FLAGS], optional): per group the OR of its flags (u32) and an offset (u32),
//! then the flags packed at the width the OR needs, so a group without flags costs its record only and the OR is a
//! zoomed-out view's flags without a decode. No column: every item's flags are 0.
//!
//! Numbers are little endian, unaligned. Every read is checked against the column's bounds: [EventTimes::read]
//! checks all group records once, then reads are infallible and never panic on any bytes.
//!
//! **What read checks.** Per column: widths at most 64, each group's values starting where the previous group's end,
//! the last group's at the column's end. Starts: slope 0 to [SLOPE_MAX], residual minimum at most 0, first ticks inside
//! the chunk's (group 0's the chunk's first), each group's item 0 equal to its first tick, each group's first tick at
//! or after the previous group's last item, the last item the chunk's last tick; so the search keys agree with
//! [Starts::get], and [EventTimes::decode_group_ends] with [EventTimes::end] in [EndMode::NextStart]. Durations: each
//! minimum at most the latest end less the first tick. Left to the writer, since checking them decodes every
//! value: items ascending inside a group, every end at or before the latest end (an overlap query's scan back trusts
//! [Durations::group_bound]), duration minimums and flag ORs that match their values.
//!
//! **Salvage.** A chunk whose payload CRC failed ([Block::salvage](crate::Block::salvage)) still reads through
//! [EventTimes::salvage]: columns in a bad segment have their groups checked one by one as they are read, and only
//! the items of a group that fails are suspect ([EventTimes::is_suspect]). A group fails when:
//!
//! - its width is over 64 or its values reach past the column;
//! - its offset or width breaks the chain with a neighbour (a damaged offset breaks the links on both sides of its
//!   group, a damaged width only the one after it, so a good neighbour isn't blamed);
//! - starts: its slope, residual minimum or first tick is out of range, its item 0 isn't its first tick, or it is out
//!   of order with a neighbour (either side may be the damaged one, so both groups fail), or it is the last group and
//!   doesn't end on the chunk's last tick (so a flipped residual in the chunk's last item marks the whole last group);
//! - durations: its minimum is past the latest end less the first tick.
//!
//! Every salvaged start is kept inside the chunk's ticks, every end at or before its latest end, and every flag
//! inside its group's OR. A flipped bit in a residual damages one item (item 0's and the chunk's last item's, the
//! group); one in a group record, one group or two, unless it moves a first tick, slope or duration minimum by
//! too little to break an order: such a group reads shifted and isn't marked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unreachable,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "STD-131: hardening of this module in progress"
)]

mod groups;
mod packed;
mod starts;
#[cfg(test)]
mod tests;

pub use packed::{Durations, Flags};
pub use starts::{Iter, SLOPE_MAX, Starts};

use crate::BlockKind;
use crate::bitpack::GROUP;
use crate::chunk::{Chunk, ChunkHead, Codec, Column, ColumnKind, EndMode};
use core::ops::Range;

/// Codec of a starts column.
pub const STARTS: Codec = Codec(1);
/// Codec of an ends column of durations.
pub const DURATIONS: Codec = Codec(2);
/// Codec of a flags column.
pub const FLAGS: Codec = Codec(3);

/// How a chunk's starts are stored (the chunk head's time mode).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TimeMode {
    /// First tick plus a period per item.
    Implicit,
    /// A linear frame of reference per group of 128 items.
    Linear,
}

impl TimeMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(TimeMode::Implicit),
            1 => Some(TimeMode::Linear),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            TimeMode::Implicit => 0,
            TimeMode::Linear => 1,
        }
    }
}

/// Why a chunk's time columns can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimesError {
    /// The chunk is not an events chunk.
    NotEvents,
    /// The chunk head's time mode is not one of [TimeMode].
    UnknownMode(u8),
    /// A column the head's modes call for is missing.
    Missing(ColumnKind),
    /// A column the head's modes leave out is there, or there twice.
    Unexpected(ColumnKind),
    /// A time column in a codec this reader doesn't know.
    UnknownCodec(ColumnKind, Codec),
    /// A column breaks the format.
    Corrupt {
        kind: ColumnKind,
        what: &'static str,
    },
}

impl core::fmt::Display for TimesError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TimesError::NotEvents => write!(f, "not an events chunk"),
            TimesError::UnknownMode(m) => write!(f, "unknown time mode {m}"),
            TimesError::Missing(k) => write!(f, "no {k:?} column"),
            TimesError::Unexpected(k) => write!(f, "unexpected {k:?} column"),
            TimesError::UnknownCodec(k, c) => write!(f, "{k:?} column in unknown codec {}", c.0),
            TimesError::Corrupt { kind, what } => write!(f, "{kind:?} column: {what}"),
        }
    }
}

impl core::error::Error for TimesError {}

/// How a chunk's items end.
#[derive(Copy, Clone, Debug)]
pub enum Ends<'b> {
    /// Every item is an instant.
    Absent,
    /// Item i ends where item i + 1 starts; the last item at `last`.
    NextStart { last: i64 },
    /// Each item's duration.
    Durations(Durations<'b>),
}

/// The times and flags of an events chunk, read in place.
#[derive(Copy, Clone, Debug)]
pub struct EventTimes<'b> {
    starts: Starts<'b>,
    ends: Ends<'b>,
    flags: Option<Flags<'b>>,
    /// The chunk head's latest end: no end of a salvaged item reads past it.
    latest_end: i64,
}

impl<'b> EventTimes<'b> {
    /// Read the time columns of an intact events chunk, checking every group record.
    pub fn read(chunk: &Chunk<'b>) -> Result<Self, TimesError> {
        Self::open(chunk, |_| false)
    }

    /// Read the time columns of a chunk whose payload may be damaged ([Chunk::bad_segments]): columns with a byte in
    /// a bad segment are read group by group, and the items of a group that doesn't hold up are suspect
    /// ([Self::is_suspect]). Fails only when a column as a whole doesn't hold up.
    pub fn salvage(chunk: &Chunk<'b>) -> Result<Self, TimesError> {
        Self::open(chunk, |c| chunk.column_damaged(c))
    }

    fn open(chunk: &Chunk<'b>, damaged: impl Fn(&Column<'b>) -> bool) -> Result<Self, TimesError> {
        let head = chunk.head();
        if head.kind != BlockKind::Events {
            return Err(TimesError::NotEvents);
        }
        let mode =
            TimeMode::from_u8(head.time_mode).ok_or(TimesError::UnknownMode(head.time_mode))?;
        let payload = chunk.payload();
        let mut starts = None;
        let mut ends = None;
        let mut flags = None;
        for c in chunk.columns() {
            let slot = match c.kind {
                ColumnKind::Starts => &mut starts,
                ColumnKind::Ends => &mut ends,
                ColumnKind::Flags => &mut flags,
                _ => continue,
            };
            if slot.replace(c).is_some() {
                return Err(TimesError::Unexpected(c.kind));
            }
        }
        let codec = |c: &Column<'_>, want: Codec| {
            if c.codec == want {
                Ok(())
            } else {
                Err(TimesError::UnknownCodec(c.kind, c.codec))
            }
        };
        let s = starts.ok_or(TimesError::Missing(ColumnKind::Starts))?;
        codec(&s, STARTS)?;
        let starts = Starts::parse(
            head.count,
            (head.first_tick, head.last_tick),
            mode,
            s.bytes,
            &payload[s.offset..],
            damaged(&s),
        )?;
        let ends = match (head.end_mode, ends) {
            (EndMode::Absent, None) => Ends::Absent,
            (EndMode::NextStart, None) => Ends::NextStart {
                last: head.latest_end,
            },
            (EndMode::Durations, Some(e)) => {
                codec(&e, DURATIONS)?;
                let max = (i128::from(head.latest_end) - i128::from(head.first_tick)) as u64;
                Ends::Durations(Durations::parse(
                    head.count,
                    max,
                    e.bytes,
                    &payload[e.offset..],
                    damaged(&e),
                )?)
            }
            (EndMode::Durations, None) => return Err(TimesError::Missing(ColumnKind::Ends)),
            (_, Some(_)) => return Err(TimesError::Unexpected(ColumnKind::Ends)),
        };
        let flags = match flags {
            Some(f) => {
                codec(&f, FLAGS)?;
                Some(Flags::parse(
                    head.count,
                    f.bytes,
                    &payload[f.offset..],
                    damaged(&f),
                )?)
            }
            None => None,
        };
        Ok(EventTimes {
            starts,
            ends,
            flags,
            latest_end: head.latest_end,
        })
    }

    /// Items in the chunk.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// Always false: a chunk has an item at least.
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn starts(&self) -> &Starts<'b> {
        &self.starts
    }

    pub fn ends(&self) -> &Ends<'b> {
        &self.ends
    }

    /// The flags column, None when every item's flags are 0.
    pub fn flags(&self) -> Option<&Flags<'b>> {
        self.flags.as_ref()
    }

    /// The start of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn start(&self, i: usize) -> i64 {
        self.starts.get(i)
    }

    /// The end of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn end(&self, i: usize) -> i64 {
        match &self.ends {
            Ends::Absent => self.starts.get(i),
            Ends::NextStart { last } => {
                if i + 1 == self.len() {
                    *last
                } else {
                    self.starts.get(i + 1)
                }
            }
            Ends::Durations(d) => {
                let end = self.starts.get(i).wrapping_add(d.get(i) as i64);
                if d.salvaged() {
                    end.min(self.latest_end)
                } else {
                    end
                }
            }
        }
    }

    /// The flags of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn flag(&self, i: usize) -> u32 {
        assert!(i < self.len(), "item {i} of {}", self.len());
        self.flags.as_ref().map_or(0, |f| f.get(i))
    }

    /// The OR of the flags of group `g`.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn group_flags(&self, g: usize) -> u32 {
        assert!(
            g < self.starts.group_count(),
            "group {g} of {}",
            self.starts.group_count()
        );
        self.flags.as_ref().map_or(0, |f| f.group_or(g))
    }

    /// Decode the ends of group `g` into `out`; returns its item count.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn decode_group_ends(&self, g: usize, out: &mut [i64; GROUP]) -> usize {
        let n = self.starts.decode_group(g, out);
        match &self.ends {
            Ends::Absent => {}
            Ends::NextStart { last } => {
                out.copy_within(1..n, 0);
                out[n - 1] = if g + 1 < self.starts.group_count() {
                    self.starts.group_first(g + 1)
                } else {
                    *last
                };
            }
            Ends::Durations(d) => {
                d.add_to_group(g, out);
                if d.salvaged() {
                    for t in out[..n].iter_mut() {
                        *t = (*t).min(self.latest_end);
                    }
                }
            }
        }
        n
    }

    /// Whether item `i` is in a group a salvage found damaged in any column.
    pub fn is_suspect(&self, i: usize) -> bool {
        self.is_suspect_group(i / GROUP)
    }

    /// Whether group `g` is one a salvage found damaged in any column.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        self.starts.is_suspect_group(g)
            || matches!(&self.ends, Ends::Durations(d) if d.is_suspect_group(g))
            || self.flags.as_ref().is_some_and(|f| f.is_suspect_group(g))
    }

    /// The item ranges of the groups a salvage found damaged, ascending.
    pub fn suspect_items(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let n = self.len();
        (0..self.starts.group_count())
            .filter(|&g| self.is_suspect_group(g))
            .map(move |g| g * GROUP..((g + 1) * GROUP).min(n))
    }
}

#[cfg(feature = "std")]
mod encode;
#[cfg(feature = "std")]
pub use encode::{EncodeError, EncodedTimes};

/// The fields of a chunk head the time codec fills in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimeHead {
    pub count: u32,
    pub first_tick: i64,
    pub last_tick: i64,
    pub latest_end: i64,
    pub time_mode: TimeMode,
    pub end_mode: EndMode,
}

impl TimeHead {
    /// A chunk head for these times.
    pub fn chunk_head(&self, track_id: u64, track_number: u32, first_item: u64) -> ChunkHead {
        ChunkHead {
            kind: BlockKind::Events,
            track_id,
            track_number,
            first_item,
            count: self.count,
            first_tick: self.first_tick,
            last_tick: self.last_tick,
            latest_end: self.latest_end,
            time_mode: self.time_mode.to_u8(),
            end_mode: self.end_mode,
        }
    }
}
