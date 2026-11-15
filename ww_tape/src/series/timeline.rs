//! [SeriesTimeline]: a series' segments in order, checked as they come, and the tick and scale of any sample.

use alloc::vec::Vec;

use exact_units::{Magnitude, Ratio64};

use super::{SeriesError, SeriesSegment};
use crate::{BlockKind, ChunkHead};

/// The segments of one series track, checked one by one as a reader finds them in records and describe sets.
///
/// Every segment is checked before it is kept, so a damaged or forged record is refused and the ones kept stay
/// consistent: ascending first indexes, a grid at or before the first sample, ticks that fit in i64, a new grid after
/// the last tick of the segment before. A segment seen again (a describe set repeats the newest) is taken once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesTimeline {
    period: Ratio64,
    segments: Vec<SeriesSegment>,
}

impl SeriesTimeline {
    /// No segments yet, for a series of `period` ticks per sample.
    pub fn new(period: Ratio64) -> Self {
        SeriesTimeline {
            period,
            segments: Vec::new(),
        }
    }

    pub fn period(&self) -> Ratio64 {
        self.period
    }

    /// The segments, in order of first index.
    pub fn segments(&self) -> &[SeriesSegment] {
        &self.segments
    }

    /// Add `s` after the segments there. A segment equal to one there is taken as a repeat and changes nothing.
    pub fn push(&mut self, s: SeriesSegment) -> Result<(), SeriesError> {
        let first = s.first_tick(self.period)?;
        if self.segments.contains(&s) {
            return Ok(());
        }
        let Some(prev) = self.segments.last() else {
            self.segments.push(s);
            return Ok(());
        };
        if s.first_index == prev.first_index {
            return Err(SeriesError::Conflict {
                first_index: s.first_index,
            });
        }
        if s.first_index < prev.first_index {
            return Err(SeriesError::NotAscending {
                first_index: s.first_index,
                previous: prev.first_index,
            });
        }
        if !s.same_grid(prev) {
            let last_index = s
                .first_index
                .checked_sub(1)
                .ok_or(SeriesError::NotAscending {
                    first_index: s.first_index,
                    previous: prev.first_index,
                })?;
            let last = prev
                .tick(self.period, last_index)
                .ok_or(SeriesError::TickOverflow { index: last_index })?;
            if first <= last {
                return Err(SeriesError::TimeNotIncreasing {
                    first_index: s.first_index,
                });
            }
        }
        self.segments.push(s);
        Ok(())
    }

    /// Add the segment a series record holds, whose record time is `tick`: refused when the time isn't the
    /// segment's first sample's tick, as for a damaged record.
    pub fn push_record(&mut self, tick: i64, item: &[u8]) -> Result<(), SeriesError> {
        let s = record_segment(self.period, tick, item)?;
        self.push(s)
    }

    /// The segments of a track's records in any order (record blocks, describe sets, backups), each `(record
    /// tick, item)`: decoded, checked against its record time, sorted by first index and pushed. A record that fails
    /// a check is left out and its error returned beside the timeline, so one damaged record costs only its segment.
    pub fn from_records<'b>(
        period: Ratio64,
        records: impl IntoIterator<Item = (i64, &'b [u8])>,
    ) -> (Self, Vec<SeriesError>) {
        let mut errors = Vec::new();
        let mut segments = Vec::new();
        for (tick, item) in records {
            match record_segment(period, tick, item) {
                Ok(s) => segments.push(s),
                Err(e) => errors.push(e),
            }
        }
        segments.sort_by_key(|s| s.first_index);
        let mut t = SeriesTimeline::new(period);
        for s in segments {
            if let Err(e) = t.push(s) {
                errors.push(e);
            }
        }
        (t, errors)
    }

    /// The segment sample `index` is in, None before the first.
    pub fn segment_of(&self, index: u64) -> Option<&SeriesSegment> {
        let after = self.segments.partition_point(|s| s.first_index <= index);
        after.checked_sub(1).and_then(|i| self.segments.get(i))
    }

    /// First index of the segment after the one sample `index` is in; None in the last segment.
    pub fn segment_end(&self, index: u64) -> Option<u64> {
        let after = self.segments.partition_point(|s| s.first_index <= index);
        self.segments.get(after).map(|s| s.first_index)
    }

    /// Tick of sample `index`, rounded down; None before the first segment or past i64.
    pub fn tick(&self, index: u64) -> Option<i64> {
        self.segment_of(index)?.tick(self.period, index)
    }

    /// The scale of sample `index`'s stored numbers; None before the first segment.
    pub fn scale(&self, index: u64) -> Option<Magnitude> {
        self.segment_of(index).map(|s| s.scale)
    }

    /// First and last tick of `count` samples from `first_index`, which must lie in one segment: what a series chunk
    /// head of them holds.
    pub fn chunk_ticks(&self, first_index: u64, count: u32) -> Result<(i64, i64), SeriesError> {
        let span = SeriesError::ChunkSpan {
            first_item: first_index,
        };
        let last_index = u64::from(count)
            .checked_sub(1)
            .and_then(|n| first_index.checked_add(n))
            .ok_or(span)?;
        let s = self
            .segment_of(first_index)
            .ok_or(SeriesError::NoSegment { index: first_index })?;
        if self
            .segment_end(first_index)
            .is_some_and(|end| last_index >= end)
        {
            return Err(span);
        }
        let first = s
            .tick(self.period, first_index)
            .ok_or(SeriesError::TickOverflow { index: first_index })?;
        let last = s
            .tick(self.period, last_index)
            .ok_or(SeriesError::TickOverflow { index: last_index })?;
        Ok((first, last))
    }

    /// Check a series chunk's head against the segments: its samples in one segment, its first and last tick theirs.
    pub fn check_chunk(&self, head: &ChunkHead) -> Result<(), SeriesError> {
        if head.kind != BlockKind::Series {
            return Err(SeriesError::ChunkSpan {
                first_item: head.first_item,
            });
        }
        let (first, last) = self.chunk_ticks(head.first_item, head.count)?;
        if head.first_tick != first || head.last_tick != last {
            return Err(SeriesError::ChunkTicks {
                first_item: head.first_item,
            });
        }
        Ok(())
    }
}

/// The segment of a series record at `tick`, checked: it decodes, and `tick` is its first sample's.
fn record_segment(period: Ratio64, tick: i64, item: &[u8]) -> Result<SeriesSegment, SeriesError> {
    let s = SeriesSegment::from_record(item)?;
    if s.first_tick(period)? == tick {
        Ok(s)
    } else {
        Err(SeriesError::RecordTime {
            first_index: s.first_index,
        })
    }
}
