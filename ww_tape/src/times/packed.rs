//! The durations column (ends in [EndMode::Durations](crate::EndMode::Durations)) and the flags column: per group a
//! record, then values packed at the group's width.
//!
//! Durations: per group a minimum (u64), a width (u8) and an offset (u32), then `duration - minimum`. Flags: per group
//! the OR of its flags (u32) and an offset (u32), then the flags at the width the OR needs.

use super::TimesError;
#[cfg(feature = "std")]
use super::groups::encode_groups;
use super::groups::{Groups, Layout, Placed, group_len, width_span};
use crate::FromFile;
use crate::bitpack::{self, GROUP, Width};
use crate::chunk::ColumnKind;

/// Bytes of a durations group record.
const DURATION_RECORD_LEN: usize = 13;
/// Bytes of a flags group record.
const FLAGS_RECORD_LEN: usize = 8;

type DurationRecord = [u8; DURATION_RECORD_LEN];
type FlagsRecord = [u8; FLAGS_RECORD_LEN];

fn duration_min(r: &DurationRecord) -> u64 {
    let [a, b, c, d, e, f, g, h, ..] = *r;
    u64::from_le_bytes([a, b, c, d, e, f, g, h])
}

const DURATIONS: Layout<DURATION_RECORD_LEN> = Layout {
    width: |r| {
        let [.., w, _, _, _, _] = *r;
        FromFile::new(u32::from(w))
    },
    offset: |r| {
        let [.., a, b, c, d] = *r;
        FromFile::new(u32::from_le_bytes([a, b, c, d]))
    },
};

fn flags_or(r: &FlagsRecord) -> u32 {
    let [a, b, c, d, ..] = *r;
    u32::from_le_bytes([a, b, c, d])
}

const FLAGS: Layout<FLAGS_RECORD_LEN> = Layout {
    width: |r| FromFile::new(bitpack::width_of(u64::from(flags_or(r)))),
    offset: |r| {
        let [.., a, b, c, d] = *r;
        FromFile::new(u32::from_le_bytes([a, b, c, d]))
    },
};

/// The low 32 bits of a value unpacked at a width of 32 or less.
fn low_u32(v: u64) -> u32 {
    let [a, b, c, d, ..] = v.to_le_bytes();
    u32::from_le_bytes([a, b, c, d])
}

/// Item durations (end minus start), read in place.
///
/// In a column read by salvage, a suspect group's items read as 0 long, and no item reads longer than the chunk's
/// latest end less its first tick.
#[derive(Copy, Clone, Debug)]
pub struct Durations<'b> {
    groups: Groups<'b, DURATION_RECORD_LEN>,
    /// The longest duration any item may have: the chunk's latest end less its first tick.
    max: u64,
    salvaged: bool,
}

impl<'b> Durations<'b> {
    /// Read a durations column of `n` items no longer than `max`; `column` and `tail` as for the starts.
    pub(crate) fn parse(
        n: usize,
        max: u64,
        column: &'b [u8],
        tail: &'b [u8],
        salvage: bool,
    ) -> Result<Self, TimesError> {
        let groups = Groups::split(ColumnKind::Ends, n, DURATIONS, column, tail)?;
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
        duration_min(self.groups.record(g))
    }

    fn record_ok(&self, g: usize) -> bool {
        self.min(g) <= self.max
    }

    /// Group `g`'s values when its record holds up in a salvage.
    fn group_ok(&self, g: usize) -> Option<Placed<'b>> {
        self.groups.group_fits(g).filter(|_| self.record_ok(g))
    }

    /// Whether group `g` is one a salvage found damaged; false past the last group.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        self.salvaged && g < self.groups.count() && self.group_ok(g).is_none()
    }

    /// Whether the column was read by salvage.
    pub(crate) fn salvaged(&self) -> bool {
        self.salvaged
    }

    /// Group `g`'s values when the group reads as it is stored.
    fn usable(&self, g: usize) -> Option<Placed<'b>> {
        if self.salvaged {
            self.group_ok(g)
        } else {
            Some(self.groups.checked(g))
        }
    }

    /// The duration of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn get(&self, i: usize) -> u64 {
        assert!(i < self.groups.n, "item {i} of {}", self.groups.n);
        let g = i / GROUP;
        let Some(p) = self.usable(g) else {
            return 0;
        };
        let r = p.width.read(p.values, i % GROUP);
        // the format's sum: minimum plus packed value, wrapping as the encoder's difference did
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
        let Some(p) = self.usable(g) else {
            return self.max;
        };
        self.min(g)
            .saturating_add(width_span(p.width.bits()))
            .min(self.max)
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
        let Some(p) = self.usable(g) else {
            return;
        };
        let mut d = [0i64; GROUP];
        // durations are u64 end - start, wrapping: the same bits as an i64
        p.width
            .decode_line(p.values, self.min(g).cast_signed(), 0, &mut d);
        if self.salvaged {
            for d in d.iter_mut() {
                *d = d.cast_unsigned().min(self.max).cast_signed();
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
    groups: Groups<'b, FLAGS_RECORD_LEN>,
    salvaged: bool,
}

impl<'b> Flags<'b> {
    /// Read a flags column of `n` items; `column` and `tail` as for the starts.
    pub(crate) fn parse(
        n: usize,
        column: &'b [u8],
        tail: &'b [u8],
        salvage: bool,
    ) -> Result<Self, TimesError> {
        let groups = Groups::split(ColumnKind::Flags, n, FLAGS, column, tail)?;
        if !salvage {
            groups.check()?;
        }
        Ok(Flags {
            groups,
            salvaged: salvage,
        })
    }

    /// Whether group `g` is one a salvage found damaged; false past the last group.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        self.salvaged && g < self.groups.count() && self.groups.group_fits(g).is_none()
    }

    /// Whether the column was read by salvage.
    pub(crate) fn salvaged(&self) -> bool {
        self.salvaged
    }

    /// Group `g`'s values when the group reads as it is stored.
    fn usable(&self, g: usize) -> Option<Placed<'b>> {
        if self.salvaged {
            self.groups.group_fits(g)
        } else {
            Some(self.groups.checked(g))
        }
    }

    /// The flags of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    pub fn get(&self, i: usize) -> u32 {
        assert!(i < self.groups.n, "item {i} of {}", self.groups.n);
        let g = i / GROUP;
        let Some(p) = self.usable(g) else {
            return 0;
        };
        // the width is the bits of a u32 OR
        let f = low_u32(p.width.read(p.values, i % GROUP));
        if self.salvaged {
            f & flags_or(self.groups.record(g))
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
            0
        } else {
            flags_or(self.groups.record(g))
        }
    }

    /// Decode group `g`'s flags into `out`; returns its item count.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn decode_group(&self, g: usize, out: &mut [u32; GROUP]) -> usize {
        let or = self.group_or(g);
        match self.usable(g) {
            Some(p) if p.width != Width::ZERO => {
                let r = p.width.unpack_u32(p.values, out);
                assert!(
                    r.is_ok(),
                    "the width of a u32 OR is at most the kernels' 32"
                );
                if self.salvaged {
                    for f in out.iter_mut() {
                        *f &= or;
                    }
                }
            }
            _ => out.fill(0),
        }
        group_len(self.groups.n, g)
    }
}

/// Encode item durations; None when the packed values pass a u32 offset.
#[cfg(feature = "std")]
pub(crate) fn encode_durations(durations: &[u64]) -> Option<std::vec::Vec<u8>> {
    encode_groups(durations, |c, offset| {
        let min = c.iter().copied().fold(u64::MAX, u64::min);
        let max = c.iter().copied().fold(0, u64::max);
        let w = bitpack::width_of(max.abs_diff(min));
        let mut rec = [0u8; DURATION_RECORD_LEN];
        crate::bytes::put_u64(&mut rec, 0, min)?;
        crate::bytes::put(&mut rec, 8, [u8::try_from(w).ok()?])?;
        crate::bytes::put_u32(&mut rec, 9, offset)?;
        // every duration is at least the minimum: the difference is exact
        let vals = c.iter().map(|d| d.wrapping_sub(min)).collect();
        Some((rec, vals, w))
    })
}

/// Encode item flags; None when the packed values pass a u32 offset.
#[cfg(feature = "std")]
pub(crate) fn encode_flags(flags: &[u32]) -> Option<std::vec::Vec<u8>> {
    encode_groups(flags, |c, offset| {
        let or = c.iter().fold(0, |a, f| a | f);
        let mut rec = [0u8; FLAGS_RECORD_LEN];
        crate::bytes::put_u32(&mut rec, 0, or)?;
        crate::bytes::put_u32(&mut rec, 4, offset)?;
        let vals = c.iter().map(|f| u64::from(*f)).collect();
        Some((rec, vals, bitpack::width_of(u64::from(or))))
    })
}
