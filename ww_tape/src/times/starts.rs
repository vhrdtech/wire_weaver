//! The starts column: implicit, or a linear frame of reference per group of 128.

use super::groups::{Groups, Layout, WidthAt, get_i64, get_u32, group_count, group_len};
use super::{TimeMode, TimesError};
use crate::bitpack::{self, GROUP, Kernel};
use crate::chunk::ColumnKind;
use core::ops::Range;

/// Largest slope a group takes (Q16 ticks per item, exclusive): 2^39 ticks per item.
pub const SLOPE_MAX: i64 = 1 << 55;
/// Bytes of a group record: first tick i64, slope i64, residual minimum i32, width u8, offset u32.
pub(crate) const RECORD_LEN: usize = 25;
const LAYOUT: Layout = Layout {
    record_len: RECORD_LEN,
    width_at: WidthAt::Byte(20),
    offset_at: 21,
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
            Repr::Linear { groups, .. } => get_i64(groups.record(g), 0),
        }
    }

    /// Whether group `g`'s record is in range: slope 0 to [SLOPE_MAX], residual minimum at most 0, first tick inside
    /// the chunk's ticks (the chunk's first for group 0).
    fn record_ok(&self, g: usize) -> bool {
        let Repr::Linear { groups, .. } = self.repr else {
            return true;
        };
        let r = groups.record(g);
        let (f, slope, rmin) = (get_i64(r, 0), get_i64(r, 8), get_u32(r, 16) as i32);
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
        record_group(&groups, g)
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
        values_in(groups.data, h)
    }

    /// Item `j` of a group.
    #[inline]
    fn value(&self, h: Group, j: usize) -> i64 {
        let Repr::Linear { groups, .. } = self.repr else {
            unreachable!("only linear chunks have group records");
        };
        value_in(groups.data, h, j)
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
            Repr::Linear { groups, salvaged } => {
                let g = i / GROUP;
                let h = if salvaged {
                    self.group(g)
                } else {
                    record_group(&groups, g)
                };
                value_in(groups.data, h, i % GROUP)
            }
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
        self.decode_group_with(Kernel::best(), g, out)
    }

    /// [Self::decode_group] with a given bit-pack kernel.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group or this CPU doesn't run `kernel`.
    pub fn decode_group_with(&self, kernel: Kernel, g: usize, out: &mut [i64; GROUP]) -> usize {
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
                bitpack::decode_line_with(kernel, self.values(h), h.width, h.base, h.slope, out);
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
        Iter::new(*self, range)
    }

    /// The starts of all items, either way.
    pub fn iter(&self) -> Iter<'b> {
        self.range(0..self.len())
    }
}

/// Group `g`'s record as stored.
#[inline]
fn record_group(groups: &Groups<'_>, g: usize) -> Group {
    let r: &[u8; RECORD_LEN] = groups.records[g * RECORD_LEN..(g + 1) * RECORD_LEN]
        .try_into()
        .expect("a whole record");
    let first = i64::from_le_bytes([r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]]);
    let slope = i64::from_le_bytes([r[8], r[9], r[10], r[11], r[12], r[13], r[14], r[15]]);
    let rmin = i32::from_le_bytes([r[16], r[17], r[18], r[19]]);
    Group {
        first,
        base: first.wrapping_add(i64::from(rmin)),
        slope,
        width: u32::from(r[20]),
        offset: u32::from_le_bytes([r[21], r[22], r[23], r[24]]) as usize,
    }
}

/// A group's residuals in the packed values `data`.
#[inline]
fn values_in(data: &[u8], h: Group) -> &[u8] {
    if h.width == 0 { &[] } else { &data[h.offset..] }
}

/// Item `j` of a group whose residuals are in `data`.
#[inline]
fn value_in(data: &[u8], h: Group, j: usize) -> i64 {
    let r = if h.width == 0 {
        0
    } else {
        bitpack::read(&data[h.offset..], j, h.width)
    };
    h.base
        .wrapping_add(bitpack::line(h.slope, j))
        .wrapping_add(r as i64)
}

/// `a / b` rounded up, for `b` above 0.
fn ceil_div(a: i128, b: i128) -> i128 {
    -((-a).div_euclid(b))
}

/// Starts of a range of items, a group decoded at a time.
#[derive(Clone)]
pub struct Iter<'b> {
    starts: Starts<'b>,
    /// Next item from the front, and one past the next from the back.
    front: usize,
    back: usize,
    /// The front's group, decoded; the front reads `front_buf[front_at]` until `front_at` reaches `front_end`.
    front_buf: [i64; GROUP],
    front_at: usize,
    front_end: usize,
    /// The back's group, decoded; the back reads `back_buf[back_at - 1]` down to `back_start`.
    back_buf: [i64; GROUP],
    back_at: usize,
    back_start: usize,
}

impl<'b> Iter<'b> {
    fn new(starts: Starts<'b>, range: Range<usize>) -> Self {
        Iter {
            starts,
            front: range.start,
            back: range.end,
            front_buf: [0; GROUP],
            front_at: 0,
            front_end: 0,
            back_buf: [0; GROUP],
            back_at: 0,
            back_start: 0,
        }
    }

    #[cold]
    fn refill_front(&mut self) {
        let g = self.front / GROUP;
        let n = self.starts.decode_group(g, &mut self.front_buf);
        self.front_at = self.front % GROUP;
        self.front_end = n;
    }

    #[cold]
    fn refill_back(&mut self) {
        let g = (self.back - 1) / GROUP;
        self.starts.decode_group(g, &mut self.back_buf);
        self.back_at = self.back - g * GROUP;
        self.back_start = 0;
    }
}

impl Iterator for Iter<'_> {
    type Item = i64;

    #[inline]
    fn next(&mut self) -> Option<i64> {
        if self.front >= self.back {
            return None;
        }
        if self.front_at >= self.front_end {
            self.refill_front();
        }
        let t = self.front_buf[self.front_at];
        self.front_at += 1;
        self.front += 1;
        Some(t)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.back - self.front;
        (n, Some(n))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<i64> {
        if self.front >= self.back {
            return None;
        }
        if self.back_at <= self.back_start {
            self.refill_back();
        }
        self.back_at -= 1;
        self.back -= 1;
        Some(self.back_buf[self.back_at])
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
    let head = groups.len() * RECORD_LEN;
    let data: usize = groups
        .iter()
        .zip(ticks.chunks(GROUP))
        .map(|((_, _, w), c)| bitpack::packed_len(c.len(), *w))
        .sum();
    let mut out = std::vec![0u8; head + data];
    let mut at = 0usize;
    for (g, ((slope, rmin, w), c)) in groups.iter().zip(ticks.chunks(GROUP)).enumerate() {
        let r = g * RECORD_LEN;
        out[r..r + 8].copy_from_slice(&c[0].to_le_bytes());
        out[r + 8..r + 16].copy_from_slice(&slope.to_le_bytes());
        out[r + 16..r + 20].copy_from_slice(&(*rmin as i32).to_le_bytes());
        out[r + 20] = *w as u8;
        out[r + 21..r + 25].copy_from_slice(&(at as u32).to_le_bytes());
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
