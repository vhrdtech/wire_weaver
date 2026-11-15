//! The durations column (ends in [EndMode::Durations](crate::EndMode::Durations)) and the flags column: per group a
//! record, then values packed at the group's width.
//!
//! Durations: per group a minimum (u64), a width (u8) and an offset (u32), then `duration - minimum`. Flags: per group
//! the OR of its flags (u32) and an offset (u32), then the flags at the width the OR needs.

use super::TimesError;
#[cfg(feature = "std")]
use super::groups::group_count;
use super::groups::{Groups, Layout, WidthAt, get_i64, get_u32, group_len};
use crate::bitpack::{self, GROUP};
use crate::chunk::ColumnKind;

/// Bytes of a durations group record.
pub(crate) const DURATION_RECORD_LEN: usize = 13;
/// Bytes of a flags group record.
pub(crate) const FLAGS_RECORD_LEN: usize = 8;

const DURATIONS: Layout = Layout {
    record_len: DURATION_RECORD_LEN,
    width_at: WidthAt::Byte(8),
    offset_at: 9,
};

const FLAGS: Layout = Layout {
    record_len: FLAGS_RECORD_LEN,
    width_at: WidthAt::OrOf(0),
    offset_at: 4,
};

/// Item durations (end minus start), read in place.
///
/// In a column read by salvage, a suspect group's items read as 0 long, and no item reads longer than the chunk's
/// latest end less its first tick.
#[derive(Copy, Clone, Debug)]
pub struct Durations<'b> {
    groups: Groups<'b>,
    /// The longest duration any item may have: the chunk's latest end less its first tick.
    max: u64,
    salvaged: bool,
}

impl<'b> Durations<'b> {
    /// Read a durations column of `n` items no longer than `max`; `column` and `tail` as for the starts.
    pub(crate) fn parse(
        n: u32,
        max: u64,
        column: &'b [u8],
        tail: &'b [u8],
        salvage: bool,
    ) -> Result<Self, TimesError> {
        let groups = Groups::split(ColumnKind::Ends, n, &DURATIONS, column, tail)?;
        let d = Durations {
            groups,
            max,
            salvaged: salvage,
        };
        if !salvage {
            groups.check()?;
            if (0..groups.count()).any(|g| !d.record_ok(g)) {
                return Err(TimesError::Corrupt {
                    kind: ColumnKind::Ends,
                    what: "duration minimum past the chunk's latest end",
                });
            }
        }
        Ok(d)
    }

    fn min(&self, g: usize) -> u64 {
        get_i64(self.groups.record(g), 0) as u64
    }

    fn record_ok(&self, g: usize) -> bool {
        self.min(g) <= self.max
    }

    fn group_ok(&self, g: usize) -> bool {
        self.groups.group_fits(g) && self.record_ok(g)
    }

    /// Whether group `g` is one a salvage found damaged.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        self.salvaged && !self.group_ok(g)
    }

    /// Whether the column was read by salvage.
    pub(crate) fn salvaged(&self) -> bool {
        self.salvaged
    }

    /// Whether group `g` reads as it is stored.
    fn usable(&self, g: usize) -> bool {
        !self.salvaged || self.group_ok(g)
    }

    /// The duration of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn get(&self, i: usize) -> u64 {
        assert!(i < self.groups.n as usize, "item {i} of {}", self.groups.n);
        let g = i / GROUP;
        if !self.usable(g) {
            return 0;
        }
        let w = self.groups.raw_width(g);
        let r = if w == 0 {
            0
        } else {
            bitpack::read(self.groups.values(g), i % GROUP, w)
        };
        let d = self.min(g).wrapping_add(r);
        if self.salvaged { d.min(self.max) } else { d }
    }

    /// No item of group `g` lasts longer: its minimum plus 2^width - 1, for an overlap query's scan back.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn group_bound(&self, g: usize) -> u64 {
        assert!(
            g < self.groups.count(),
            "group {g} of {}",
            self.groups.count()
        );
        if !self.usable(g) {
            return self.max;
        }
        let w = self.groups.raw_width(g);
        let span = if w >= 64 { u64::MAX } else { (1u64 << w) - 1 };
        self.min(g).saturating_add(span).min(self.max)
    }

    /// Ends of group `g` from its starts: `starts[j] + duration[j]`, in place.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn add_to_group(&self, g: usize, ticks: &mut [i64; GROUP]) {
        assert!(
            g < self.groups.count(),
            "group {g} of {}",
            self.groups.count()
        );
        if !self.usable(g) {
            return;
        }
        let w = self.groups.raw_width(g);
        let mut d = [0i64; GROUP];
        let src = if w == 0 {
            &[][..]
        } else {
            self.groups.values(g)
        };
        bitpack::decode_line(src, w, self.min(g) as i64, 0, &mut d);
        if self.salvaged {
            for d in d.iter_mut() {
                *d = (*d as u64).min(self.max) as i64;
            }
        }
        for (t, d) in ticks.iter_mut().zip(d) {
            *t = t.wrapping_add(d);
        }
    }
}

/// The u32 flags of items, read in place.
///
/// In a column read by salvage, a suspect group's flags and OR read as 0, and an item's flags keep only the bits of
/// its group's OR.
#[derive(Copy, Clone, Debug)]
pub struct Flags<'b> {
    groups: Groups<'b>,
    salvaged: bool,
}

impl<'b> Flags<'b> {
    /// Read a flags column of `n` items; `column` and `tail` as for the starts.
    pub(crate) fn parse(
        n: u32,
        column: &'b [u8],
        tail: &'b [u8],
        salvage: bool,
    ) -> Result<Self, TimesError> {
        let groups = Groups::split(ColumnKind::Flags, n, &FLAGS, column, tail)?;
        if !salvage {
            groups.check()?;
        }
        Ok(Flags {
            groups,
            salvaged: salvage,
        })
    }

    /// Whether group `g` is one a salvage found damaged.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        self.salvaged && !self.groups.group_fits(g)
    }

    /// The flags of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn get(&self, i: usize) -> u32 {
        assert!(i < self.groups.n as usize, "item {i} of {}", self.groups.n);
        let g = i / GROUP;
        if self.is_suspect_group(g) {
            return 0;
        }
        let w = self.groups.raw_width(g);
        if w == 0 {
            return 0;
        }
        let f = bitpack::read(self.groups.values(g), i % GROUP, w) as u32;
        if self.salvaged {
            f & get_u32(self.groups.record(g), 0)
        } else {
            f
        }
    }

    /// The OR of group `g`'s flags.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn group_or(&self, g: usize) -> u32 {
        assert!(
            g < self.groups.count(),
            "group {g} of {}",
            self.groups.count()
        );
        if self.is_suspect_group(g) {
            return 0;
        }
        get_u32(self.groups.record(g), 0)
    }

    /// Decode group `g`'s flags into `out`; returns its item count.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn decode_group(&self, g: usize, out: &mut [u32; GROUP]) -> usize {
        let or = self.group_or(g);
        let w = bitpack::width_of(u64::from(or));
        if w == 0 {
            out.fill(0);
        } else {
            bitpack::unpack_u32(self.groups.values(g), w, out);
            if self.salvaged {
                for f in out.iter_mut() {
                    *f &= or;
                }
            }
        }
        group_len(self.groups.n, g)
    }
}

/// Write per-group records and packed values for `values` (one per item) into a new column. `record(g, group values,
/// offset)` fills a group's record and returns its packed values and their width.
#[cfg(feature = "std")]
fn encode_groups<T: Copy>(
    values: &[T],
    record_len: usize,
    mut record: impl FnMut(&[T], &mut [u8], u32) -> (std::vec::Vec<u64>, u32),
) -> std::vec::Vec<u8> {
    let head = group_count(values.len() as u32) * record_len;
    let mut out = std::vec![0u8; head];
    let mut at = 0usize;
    for (g, c) in values.chunks(GROUP).enumerate() {
        let (packed, w) = record(c, &mut out[g * record_len..(g + 1) * record_len], at as u32);
        let len = bitpack::packed_len(c.len(), w);
        out.resize(head + at + len, 0);
        bitpack::pack(&mut out[head + at..], packed, w);
        at += len;
    }
    out
}

/// Encode item durations.
#[cfg(feature = "std")]
pub(crate) fn encode_durations(durations: &[u64]) -> std::vec::Vec<u8> {
    encode_groups(durations, DURATION_RECORD_LEN, |c, rec, offset| {
        let min = c.iter().copied().min().expect("a group has an item");
        let max = c.iter().copied().max().expect("a group has an item");
        let w = bitpack::width_of(max - min);
        rec[0..8].copy_from_slice(&min.to_le_bytes());
        rec[8] = w as u8;
        rec[9..13].copy_from_slice(&offset.to_le_bytes());
        (c.iter().map(|d| d - min).collect(), w)
    })
}

/// Encode item flags.
#[cfg(feature = "std")]
pub(crate) fn encode_flags(flags: &[u32]) -> std::vec::Vec<u8> {
    encode_groups(flags, FLAGS_RECORD_LEN, |c, rec, offset| {
        let or = c.iter().fold(0, |a, f| a | f);
        rec[0..4].copy_from_slice(&or.to_le_bytes());
        rec[4..8].copy_from_slice(&offset.to_le_bytes());
        (
            c.iter().map(|f| u64::from(*f)).collect(),
            bitpack::width_of(u64::from(or)),
        )
    })
}
