//! The starts column: implicit, or a linear frame of reference per group of 128.

use super::groups::{Groups, Layout, WidthAt, get_i64, get_u32, group_count, group_len};
use super::{TimeMode, TimesError};
use crate::bitpack::{self, GROUP};
use crate::chunk::ColumnKind;
use core::ops::Range;

/// Largest slope a group takes (Q16 ticks per item, exclusive): 2^39 ticks per item.
pub const SLOPE_MAX: i64 = 1 << 55;
/// Bytes of a group's first tick.
const FIRST_LEN: usize = 8;
/// Bytes of a group record after the first ticks: slope i64, residual minimum i32, width u8, offset u32.
pub(crate) const RECORD_LEN: usize = 17;
const LAYOUT: Layout = Layout {
    lead: FIRST_LEN,
    record_len: RECORD_LEN,
    width_at: WidthAt::Byte(12),
    offset_at: 13,
};

/// One group's line and residuals.
#[derive(Copy, Clone, Debug)]
struct Group {
    /// The group's first tick.
    first: i64,
    /// first + residual minimum: the line's base.
    base: i64,
    slope: i64,
    width: u32,
    /// Where the residuals start in the packed values; unused at width 0.
    offset: usize,
}

#[derive(Copy, Clone, Debug)]
enum Repr<'b> {
    Implicit {
        period: i64,
    },
    Linear {
        firsts: &'b [u8],
        groups: Groups<'b>,
        /// Read from a damaged column: groups are checked as they are read.
        salvaged: bool,
    },
}

/// The start ticks of a chunk's items, read in place.
///
/// Reads never panic on any bytes; [Starts::get] and [Starts::decode_group] panic only for an item or group past
/// the end. In a column read by salvage, the items of a group whose record doesn't hold up ([Starts::is_suspect])
/// read as that group's first tick, kept inside the chunk's tick range.
#[derive(Copy, Clone, Debug)]
pub struct Starts<'b> {
    n: u32,
    first: i64,
    last: i64,
    repr: Repr<'b>,
}

impl<'b> Starts<'b> {
    /// Read a starts column of `n` items whose first and last ticks the chunk head names; `column` is its bytes,
    /// `tail` the payload from its first byte on. With `salvage`, group records are checked one by one as read
    /// instead of all at once.
    pub(crate) fn parse(
        n: u32,
        (first, last): (i64, i64),
        mode: TimeMode,
        column: &'b [u8],
        tail: &'b [u8],
        salvage: bool,
    ) -> Result<Self, TimesError> {
        let corrupt = |what| TimesError::Corrupt {
            kind: ColumnKind::Starts,
            what,
        };
        let repr = match mode {
            TimeMode::Implicit => {
                if column.len() != 8 {
                    return Err(corrupt("implicit column not one period"));
                }
                let period = get_i64(column, 0);
                let end = i128::from(first) + i128::from(n - 1) * i128::from(period);
                if period < 0 || end != i128::from(last) {
                    return Err(corrupt("period doesn't reach the last tick"));
                }
                Repr::Implicit { period }
            }
            TimeMode::Linear => {
                let groups = Groups::split(ColumnKind::Starts, n, &LAYOUT, column, tail)?;
                Repr::Linear {
                    firsts: &column[..group_count(n) * FIRST_LEN],
                    groups,
                    salvaged: salvage,
                }
            }
        };
        let s = Starts {
            n,
            first,
            last,
            repr,
        };
        if let Repr::Linear { groups, .. } = repr
            && !salvage
        {
            groups.check()?;
            let mut prev = first;
            for g in 0..groups.count() {
                if !s.record_ok(g) {
                    return Err(corrupt("group record out of range"));
                }
                let f = s.raw_first(g);
                if f < prev {
                    return Err(corrupt("group first ticks not ascending"));
                }
                prev = f;
            }
            if s.get(n as usize - 1) != last {
                return Err(corrupt("last item isn't the chunk's last tick"));
            }
        }
        Ok(s)
    }

    /// Items in the chunk.
    pub fn len(&self) -> usize {
        self.n as usize
    }

    /// Always false: a chunk has an item at least.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The first item's start.
    pub fn first_tick(&self) -> i64 {
        self.first
    }

    /// The last item's start.
    pub fn last_tick(&self) -> i64 {
        self.last
    }

    /// The time mode the column is in.
    pub fn mode(&self) -> TimeMode {
        match self.repr {
            Repr::Implicit { .. } => TimeMode::Implicit,
            Repr::Linear { .. } => TimeMode::Linear,
        }
    }

    /// The period of an implicit chunk.
    pub fn period(&self) -> Option<i64> {
        match self.repr {
            Repr::Implicit { period } => Some(period),
            Repr::Linear { .. } => None,
        }
    }

    /// Groups of [GROUP] items (the last may hold fewer).
    pub fn group_count(&self) -> usize {
        group_count(self.n)
    }

    fn raw_first(&self, g: usize) -> i64 {
        match self.repr {
            Repr::Implicit { period } => self.first + (g * GROUP) as i64 * period,
            Repr::Linear { firsts, .. } => get_i64(firsts, g * FIRST_LEN),
        }
    }

    /// Whether group `g`'s record is in range: slope 0 to [SLOPE_MAX], residual minimum at most 0, first tick inside
    /// the chunk's ticks (the chunk's first for group 0).
    fn record_ok(&self, g: usize) -> bool {
        let Repr::Linear { groups, .. } = self.repr else {
            return true;
        };
        let r = groups.record(g);
        let slope = get_i64(r, 0);
        let rmin = get_u32(r, 8) as i32;
        let f = self.raw_first(g);
        (0..SLOPE_MAX).contains(&slope)
            && rmin <= 0
            && (self.first..=self.last).contains(&f)
            && (g > 0 || f == self.first)
    }

    /// Whether group `g` of a salvaged column holds up: its record in range ([Self::record_ok]), fitting the column
    /// ([Groups::group_fits]), and the last group ending on the chunk's last tick.
    fn group_ok(&self, g: usize) -> bool {
        let Repr::Linear { groups, .. } = self.repr else {
            return true;
        };
        if !(groups.group_fits(g) && self.record_ok(g)) {
            return false;
        }
        g + 1 < groups.count()
            || self.value(self.group_raw(g), group_len(self.n, g) - 1) == self.last
    }

    fn group_raw(&self, g: usize) -> Group {
        let Repr::Linear { groups, .. } = self.repr else {
            unreachable!("only linear chunks have group records");
        };
        let r = groups.record(g);
        let first = self.raw_first(g);
        Group {
            first,
            base: first.wrapping_add(i64::from(get_u32(r, 8) as i32)),
            slope: get_i64(r, 0),
            width: groups.raw_width(g),
            offset: groups.raw_offset(g),
        }
    }

    /// Group `g`'s line; a suspect group's is flat at its first tick, kept inside the chunk.
    fn group(&self, g: usize) -> Group {
        let Repr::Linear { salvaged, .. } = self.repr else {
            unreachable!("only linear chunks have group records");
        };
        if salvaged && !self.group_ok(g) {
            let first = self.raw_first(g).clamp(self.first, self.last);
            return Group {
                first,
                base: first,
                slope: 0,
                width: 0,
                offset: 0,
            };
        }
        self.group_raw(g)
    }

    fn values(&self, h: Group) -> &'b [u8] {
        let Repr::Linear { groups, .. } = self.repr else {
            unreachable!("only linear chunks have group records");
        };
        if h.width == 0 {
            return &[];
        }
        &groups.data[h.offset..]
    }

    /// Item `j` of a group.
    #[inline]
    fn value(&self, h: Group, j: usize) -> i64 {
        let r = if h.width == 0 {
            0
        } else {
            bitpack::read(self.values(h), j, h.width)
        };
        h.base
            .wrapping_add(bitpack::line(h.slope, j))
            .wrapping_add(r as i64)
    }

    /// Whether item `i` is in a group a salvage found damaged; its start is then a stand-in.
    pub fn is_suspect(&self, i: usize) -> bool {
        self.is_suspect_group(i / GROUP)
    }

    /// Whether group `g` is one a salvage found damaged.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        matches!(self.repr, Repr::Linear { salvaged: true, .. }) && !self.group_ok(g)
    }

    /// The start of item `i`.
    ///
    /// # Panics
    ///
    /// When `i` is past the last item.
    #[inline]
    pub fn get(&self, i: usize) -> i64 {
        assert!(i < self.len(), "item {i} of {}", self.n);
        match self.repr {
            Repr::Implicit { period } => self.first.wrapping_add((i as i64).wrapping_mul(period)),
            Repr::Linear { .. } => self.value(self.group(i / GROUP), i % GROUP),
        }
    }

    /// The first tick of group `g`: its first item's start, the key the search runs on.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn group_first(&self, g: usize) -> i64 {
        assert!(
            g < self.group_count(),
            "group {g} of {}",
            self.group_count()
        );
        match self.repr {
            Repr::Implicit { .. } => self.raw_first(g),
            Repr::Linear { .. } => self.group(g).first,
        }
    }

    /// Decode group `g` into `out`; returns its item count. Entries past it hold whatever follows.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group.
    pub fn decode_group(&self, g: usize, out: &mut [i64; GROUP]) -> usize {
        assert!(
            g < self.group_count(),
            "group {g} of {}",
            self.group_count()
        );
        match self.repr {
            Repr::Implicit { period } => {
                let mut t = self.raw_first(g);
                for o in out.iter_mut() {
                    *o = t;
                    t = t.wrapping_add(period);
                }
            }
            Repr::Linear { .. } => {
                let h = self.group(g);
                bitpack::decode_line(self.values(h), h.width, h.base, h.slope, out);
            }
        }
        group_len(self.n, g)
    }

    /// The first item starting at or after `t`; [Self::len] when none does.
    pub fn lower_bound(&self, t: i64) -> usize {
        if t <= self.first {
            return 0;
        }
        if t > self.last {
            return self.len();
        }
        match self.repr {
            Repr::Implicit { period } => {
                debug_assert!(period > 0, "first < t <= last needs a period");
                let k = ceil_div(i128::from(t) - i128::from(self.first), i128::from(period));
                (k as usize).min(self.len())
            }
            Repr::Linear { .. } => self.lower_bound_linear(t),
        }
    }

    fn lower_bound_linear(&self, t: i64) -> usize {
        // groups whose first tick is below t: at least group 0, since t > first
        let (mut lo, mut hi) = (1usize, self.group_count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.group(mid).first < t {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let g = lo - 1;
        let h = self.group(g);
        let n = group_len(self.n, g);
        let (mut lo, mut hi) = (1usize, n);
        if h.slope > 0 {
            // item j lies in [base + line(j), base + line(j) + 2^width - 1]: keep only the items whose band holds t
            let s = i128::from(h.slope);
            let span = if h.width >= 64 {
                i128::from(u64::MAX)
            } else {
                (1i128 << h.width) - 1
            };
            let below = (i128::from(t) - i128::from(h.base) - span) << 16;
            if below > 0 {
                lo = lo.max(usize::try_from(below / s).unwrap_or(usize::MAX).min(n));
            }
            let atleast = (i128::from(t) - i128::from(h.base)) << 16;
            if atleast >= 0 {
                hi = hi.min(usize::try_from(ceil_div(atleast, s)).unwrap_or(usize::MAX));
            }
            lo = lo.min(hi);
        }
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.value(h, mid) < t {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        g * GROUP + lo
    }

    /// The last item starting at or before `t`.
    pub fn last_at_or_before(&self, t: i64) -> Option<usize> {
        match t.checked_add(1) {
            Some(t1) => self.lower_bound(t1).checked_sub(1),
            None => Some(self.len() - 1),
        }
    }

    /// The starts of items `range`, either way.
    ///
    /// # Panics
    ///
    /// When `range` reaches past the last item.
    pub fn range(&self, range: Range<usize>) -> Iter<'b> {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "items {range:?} of {}",
            self.n
        );
        Iter {
            starts: *self,
            front: range.start,
            back: range.end,
            front_buf: Buf::EMPTY,
            back_buf: Buf::EMPTY,
        }
    }

    /// The starts of all items, either way.
    pub fn iter(&self) -> Iter<'b> {
        self.range(0..self.len())
    }
}

/// `a / b` rounded up, for `b` above 0.
fn ceil_div(a: i128, b: i128) -> i128 {
    -((-a).div_euclid(b))
}

/// One decoded group.
#[derive(Clone)]
struct Buf {
    group: usize,
    ticks: [i64; GROUP],
}

impl Buf {
    const EMPTY: Self = Buf {
        group: usize::MAX,
        ticks: [0; GROUP],
    };

    fn at(&mut self, s: &Starts<'_>, i: usize) -> i64 {
        let g = i / GROUP;
        if self.group != g {
            s.decode_group(g, &mut self.ticks);
            self.group = g;
        }
        self.ticks[i % GROUP]
    }
}

/// Starts of a range of items, a group decoded at a time.
#[derive(Clone)]
pub struct Iter<'b> {
    starts: Starts<'b>,
    front: usize,
    back: usize,
    front_buf: Buf,
    back_buf: Buf,
}

impl Iterator for Iter<'_> {
    type Item = i64;

    #[inline]
    fn next(&mut self) -> Option<i64> {
        if self.front >= self.back {
            return None;
        }
        let t = self.front_buf.at(&self.starts, self.front);
        self.front += 1;
        Some(t)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.back - self.front;
        (n, Some(n))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<i64> {
        if self.front >= self.back {
            return None;
        }
        self.back -= 1;
        Some(self.back_buf.at(&self.starts, self.back))
    }
}

impl ExactSizeIterator for Iter<'_> {}

/// Pick a group's slope and residual minimum: the fitted slope or 0, whichever needs the narrower width (0 on a
/// tie, and when the fitted one's minimum doesn't fit an i32).
#[cfg(feature = "std")]
fn fit(g: &[i64]) -> (i64, i64, u32) {
    let first = i128::from(g[0]);
    let spread = |slope: i64| {
        let (mut lo, mut hi) = (i128::MAX, i128::MIN);
        for (j, &x) in g.iter().enumerate() {
            let r = i128::from(x) - first - i128::from(bitpack::line(slope, j));
            lo = lo.min(r);
            hi = hi.max(r);
        }
        (lo, hi)
    };
    // a spread past u64 (a fitted slope over ticks near both ends of i64) is wider than any width
    let width = |(lo, hi): (i128, i128)| u64::try_from(hi - lo).map_or(u32::MAX, bitpack::width_of);
    let flat = spread(0);
    let mut best = (0, flat.0 as i64, width(flat));
    if g.len() >= 2 {
        let cand = ((i128::from(g[g.len() - 1]) - first) << 16) / (g.len() as i128 - 1);
        if cand > 0 && cand < i128::from(SLOPE_MAX) {
            let cand = cand as i64;
            let s = spread(cand);
            if width(s) < best.2 && s.0 >= i128::from(i32::MIN) {
                best = (cand, s.0 as i64, width(s));
            }
        }
    }
    best
}

/// Encode ascending `ticks` (at least one): the time mode and the column's bytes.
#[cfg(feature = "std")]
pub(crate) fn encode(ticks: &[i64]) -> (TimeMode, std::vec::Vec<u8>) {
    assert!(!ticks.is_empty(), "a chunk has an item at least");
    let period = if ticks.len() >= 2 {
        ticks[1] - ticks[0]
    } else {
        0
    };
    if ticks.windows(2).all(|w| w[1] - w[0] == period) {
        return (TimeMode::Implicit, period.to_le_bytes().to_vec());
    }
    let groups: std::vec::Vec<(i64, i64, u32)> = ticks.chunks(GROUP).map(fit).collect();
    let head = groups.len() * (FIRST_LEN + RECORD_LEN);
    let data: usize = groups
        .iter()
        .zip(ticks.chunks(GROUP))
        .map(|((_, _, w), c)| bitpack::packed_len(c.len(), *w))
        .sum();
    let mut out = std::vec![0u8; head + data];
    let records = groups.len() * FIRST_LEN;
    let mut at = 0usize;
    for (g, ((slope, rmin, w), c)) in groups.iter().zip(ticks.chunks(GROUP)).enumerate() {
        out[g * FIRST_LEN..(g + 1) * FIRST_LEN].copy_from_slice(&c[0].to_le_bytes());
        let r = records + g * RECORD_LEN;
        out[r..r + 8].copy_from_slice(&slope.to_le_bytes());
        out[r + 8..r + 12].copy_from_slice(&(*rmin as i32).to_le_bytes());
        out[r + 12] = *w as u8;
        out[r + 13..r + 17].copy_from_slice(&(at as u32).to_le_bytes());
        let base = c[0].wrapping_add(*rmin);
        let residuals = c
            .iter()
            .enumerate()
            .map(|(j, &x)| x.wrapping_sub(base).wrapping_sub(bitpack::line(*slope, j)) as u64);
        at += bitpack::pack(&mut out[head + at..], residuals, *w);
    }
    assert_eq!(head + at, out.len(), "packed groups fill the column");
    (TimeMode::Linear, out)
}
