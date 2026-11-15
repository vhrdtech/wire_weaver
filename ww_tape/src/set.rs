//! [DescribeSet]: describe records in canonical order, written as describe blocks.

use std::collections::{BTreeMap, BTreeSet};

use shrink_wrap::prelude::*;

use crate::{
    BlockId, BlockKind, BlockWriter, CRC_LEN, ClockTime, Describe, HEADER_LEN, MAX_BLOCK_CLOCKS,
    MAX_BLOCK_LEN, MAX_CLOCK_TABLE, MAX_RECORD_HEAD, Rank,
};

/// Why a describe set can't be built or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescribeError {
    /// A record doesn't encode.
    Encode(shrink_wrap::Error),
    /// A track's dimensions aren't in ascending order of name, or a name repeats.
    Dimensions { track: u32 },
    /// A track, map or sticky record names a clock the set has no [Describe::Clock] for.
    UnknownClock { clock: u32 },
    /// A clock's resolution is under one tick (`resolution · rate < 1`), or not a time step at all.
    Resolution { clock: u32 },
    /// A sticky record on a track the set has no [Describe::Track] for.
    UnknownTrack { track: u32 },
    /// A sticky record on another clock than its track's.
    WrongClock {
        track: u32,
        clock: u32,
        track_clock: u32,
    },
    /// A map that isn't strictly increasing over its segments.
    MapNotIncreasing { from: u32, to: u32 },
    /// An [Epoch](crate::ClockOrigin::Epoch) clock without its epoch link: a map from it to the previous epoch's
    /// clock, one exact offset segment from tick 0 to the end with `b0 > 0` ([ClockMap::is_epoch_link](crate::ClockMap::is_epoch_link)).
    EpochUnmapped { clock: u32 },
    /// A record longer than a block can be.
    TooLong { len: usize },
    /// A track's kind breaks a rule ([TrackDef::check_kind](crate::TrackDef::check_kind)).
    Kind {
        track: u32,
        error: crate::SeriesError,
    },
}

impl core::fmt::Display for DescribeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DescribeError::Encode(e) => write!(f, "describe record doesn't encode: {e:?}"),
            DescribeError::Dimensions { track } => {
                write!(
                    f,
                    "track {track}: dimensions not in ascending order of name"
                )
            }
            DescribeError::UnknownClock { clock } => write!(f, "clock {clock} is not described"),
            DescribeError::Resolution { clock } => {
                write!(f, "clock {clock}: resolution under one tick")
            }
            DescribeError::UnknownTrack { track } => write!(f, "track {track} is not described"),
            DescribeError::MapNotIncreasing { from, to } => {
                write!(
                    f,
                    "map from clock {from} to {to} is not strictly increasing"
                )
            }
            DescribeError::EpochUnmapped { clock } => write!(
                f,
                "epoch clock {clock}: no epoch link (an exact offset map from it to the previous epoch's clock)"
            ),
            DescribeError::WrongClock {
                track,
                clock,
                track_clock,
            } => write!(
                f,
                "track {track}: record on clock {clock}, the track counts clock {track_clock}"
            ),
            DescribeError::TooLong { len } => {
                write!(f, "record of {len} bytes is longer than a block")
            }
            DescribeError::Kind { track, error } => write!(f, "track {track}: {error:?}"),
        }
    }
}

impl std::error::Error for DescribeError {}

/// `d` encoded, into a buffer grown until it fits.
fn encode(d: &Describe<'_>) -> Result<Vec<u8>, DescribeError> {
    let mut buf = vec![0u8; 1024];
    loop {
        let r = d.to_ww_bytes(&mut buf).map(<[u8]>::len);
        match r {
            Ok(len) => {
                buf.truncate(len);
                return Ok(buf);
            }
            Err(_) if buf.len() < MAX_BLOCK_LEN => {
                let n = buf.len() * 2;
                buf.resize(n, 0);
            }
            Err(e) => return Err(DescribeError::Encode(e)),
        }
    }
}

/// The newest record of a sticky track, written after the definitions.
#[derive(Debug, Clone, Copy)]
pub struct Sticky<'a> {
    pub track: u32,
    pub at: ClockTime,
    /// The record's item; on an events track with spans or flags an [EventRecord](crate::EventRecord), the whole
    /// event.
    pub item: &'a [u8],
}

/// Describe records kept in canonical order ([Describe::rank]), so the same definitions give the same bytes
/// whatever order they were added in.
///
/// A record replaces the one of the same rank and key: the producer, a bundle of the same hash, a clock, map or
/// track of the same number(s); typed items equal byte for byte are one.
#[derive(Debug, Clone, Default)]
pub struct DescribeSet {
    records: BTreeMap<(Rank, Vec<u8>), Vec<u8>>,
    clocks: BTreeSet<u32>,
    /// Clocks named by tracks and maps.
    named: BTreeMap<(Rank, Vec<u8>), Vec<u32>>,
    /// The clock of every track.
    track_clocks: BTreeMap<u32, u32>,
    /// Clocks of origin [Epoch](crate::ClockOrigin::Epoch).
    epoch_clocks: BTreeSet<u32>,
    /// Every map by its clocks, and whether it is an epoch link.
    maps: BTreeMap<(u32, u32), bool>,
}

/// Whether `c`'s resolution is at least one tick: `resolution · rate >= 1`, exactly where both are rational.
fn resolution_ok(c: &crate::ClockDef<'_>) -> bool {
    let Ok(ticks) = c.rate.magnitude().checked_mul(c.resolution.magnitude()) else {
        return false;
    };
    match ticks.to_ratio() {
        Some((num, den)) => num >= den,
        // irrational (π in it) or too wide for u128: never exactly 1, so the approximation decides
        None => ticks.checked_to_f64().is_some_and(|t| t >= 1.0),
    }
}

impl DescribeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `d`, replacing a record with the same key. Refused: a track whose dimensions aren't canonical, a clock
    /// whose resolution is under one tick, a map that isn't strictly increasing.
    pub fn insert(&mut self, d: &Describe<'_>) -> Result<(), DescribeError> {
        let bytes = encode(d)?;
        let rank = d.rank();
        let sub = match d {
            Describe::Bundle { hash, .. } => hash
                .iter()
                .collect::<Result<Vec<u8>, _>>()
                .map_err(DescribeError::Encode)?,
            Describe::Item(_) | Describe::Unknown(_) => bytes.clone(),
            _ => Vec::new(),
        };
        let named = match d {
            Describe::Track(t) => {
                if !t.dimensions_canonical() {
                    return Err(DescribeError::Dimensions { track: t.number.0 });
                }
                t.check_kind().map_err(|error| DescribeError::Kind {
                    track: t.number.0,
                    error,
                })?;
                vec![t.clock.0]
            }
            Describe::Map(m) => vec![m.from.0, m.to.0],
            _ => vec![],
        };
        if let Describe::Clock(c) = d
            && !resolution_ok(c)
        {
            return Err(DescribeError::Resolution { clock: c.number.0 });
        }
        if let Describe::Map(m) = d
            && !m.is_increasing()
        {
            return Err(DescribeError::MapNotIncreasing {
                from: m.from.0,
                to: m.to.0,
            });
        }
        let key = (rank, sub);
        match d {
            Describe::Clock(c) => {
                self.clocks.insert(c.number.0);
                if c.origin == crate::ClockOrigin::Epoch {
                    self.epoch_clocks.insert(c.number.0);
                } else {
                    self.epoch_clocks.remove(&c.number.0);
                }
            }
            Describe::Map(m) => {
                self.maps.insert((m.from.0, m.to.0), m.is_epoch_link());
            }
            Describe::Track(t) => {
                self.track_clocks.insert(t.number.0, t.clock.0);
            }
            _ => {}
        }
        if named.is_empty() {
            self.named.remove(&key);
        } else {
            self.named.insert(key.clone(), named);
        }
        self.records.insert(key, bytes);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The encoded records in canonical order.
    pub fn records(&self) -> impl Iterator<Item = &[u8]> {
        self.records.values().map(Vec::as_slice)
    }

    /// Every clock a track or map names is described, and every epoch clock has its epoch link.
    pub fn check(&self) -> Result<(), DescribeError> {
        for clock in self.named.values().flatten() {
            if !self.clocks.contains(clock) {
                return Err(DescribeError::UnknownClock { clock: *clock });
            }
        }
        for clock in &self.epoch_clocks {
            let mapped = self
                .maps
                .iter()
                .any(|(&(from, _), &link)| from == *clock && link);
            if !mapped {
                return Err(DescribeError::EpochUnmapped { clock: *clock });
            }
        }
        Ok(())
    }

    /// Write the set as describe blocks of at most `block_len_max` bytes (a longer record gets a block of its own),
    /// the `sticky` records after the definitions in order of track. A sticky record is on its track's clock. Blocks are numbered from `*id` on in its run,
    /// which is left at the next number; `out` takes each block.
    pub fn write_blocks(
        &self,
        id: &mut BlockId,
        block_len_max: usize,
        sticky: &[Sticky<'_>],
        mut out: impl FnMut(&[u8]),
    ) -> Result<(), DescribeError> {
        self.check()?;
        let mut sticky: Vec<&Sticky<'_>> = sticky.iter().collect();
        sticky.sort_by_key(|s| s.track);
        for s in &sticky {
            let Some(&track_clock) = self.track_clocks.get(&s.track) else {
                return Err(DescribeError::UnknownTrack { track: s.track });
            };
            if s.at.clock != track_clock {
                return Err(DescribeError::WrongClock {
                    track: s.track,
                    clock: s.at.clock,
                    track_clock,
                });
            }
        }
        let entries: Vec<(Option<&Sticky<'_>>, &[u8])> = self
            .records()
            .map(|r| (None, r))
            .chain(sticky.iter().map(|s| (Some(*s), s.item)))
            .collect();
        let empty = HEADER_LEN + MAX_CLOCK_TABLE + CRC_LEN;
        let mut i = 0;
        while i < entries.len() {
            let mut size = empty;
            let mut clocks = BTreeSet::new();
            let mut j = i;
            while j < entries.len() {
                let (s, item) = entries[j];
                let r = item.len() + MAX_RECORD_HEAD;
                let new_clock = s.is_some_and(|s| !clocks.contains(&s.at.clock));
                let fits =
                    size + r <= block_len_max && !(new_clock && clocks.len() == MAX_BLOCK_CLOCKS);
                if j > i && !fits {
                    break;
                }
                if empty + r > MAX_BLOCK_LEN {
                    return Err(DescribeError::TooLong { len: item.len() });
                }
                if let Some(s) = s {
                    clocks.insert(s.at.clock);
                }
                size += r;
                j += 1;
            }
            let clocks: Vec<u32> = clocks.into_iter().collect();
            let mut buf = vec![0u8; size];
            let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, *id, &clocks);
            *id = id.next();
            for (s, item) in &entries[i..j] {
                let pushed = match s {
                    Some(s) => w.push(s.track, s.at, &[item]),
                    None => w.push_describe(&[item]),
                };
                pushed.expect("block sized to fit its records");
            }
            let len = w.finish();
            out(&buf[..len]);
            i = j;
        }
        Ok(())
    }
}
