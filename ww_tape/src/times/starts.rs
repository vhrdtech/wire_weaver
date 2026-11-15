//! The starts column: implicit, or a linear frame of reference per group of 128.

#[cfg(feature = "std")]
use super::groups::encode_groups;
use super::groups::{
    Groups, Layout, Placed, group_count, group_len, item_i64, partition, width_span,
};
use super::{TimeMode, TimesError};
use crate::FromFile;
use crate::bitpack::{self, GROUP, Kernel, Width};
use crate::chunk::ColumnKind;
use core::ops::Range;

/// Largest slope a group takes (Q16 ticks per item, exclusive): 2^39 ticks per item.
pub const SLOPE_MAX: i64 = 1 << 55;
/// Bytes of a group record: first tick i64, slope i64, residual minimum i32, width u8, offset u32.
const RECORD_LEN: usize = 25;
type Record = [u8; RECORD_LEN];

const LAYOUT: Layout<RECORD_LEN> = Layout {
    width: |r| {
        let [.., w, _, _, _, _] = *r;
        FromFile::new(u32::from(w))
    },
    offset: |r| {
        let [.., a, b, c, d] = *r;
        FromFile::new(u32::from_le_bytes([a, b, c, d]))
    },
};

/// The line of a group record: first tick, slope, residual minimum.
#[derive(Copy, Clone, Debug)]
struct Line {
    first: i64,
    slope: i64,
    rmin: i32,
}

impl Line {
    fn of(r: &Record) -> Self {
        let [
            f0,
            f1,
            f2,
            f3,
            f4,
            f5,
            f6,
            f7,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            m0,
            m1,
            m2,
            m3,
            ..,
        ] = *r;
        Line {
            first: i64::from_le_bytes([f0, f1, f2, f3, f4, f5, f6, f7]),
            slope: i64::from_le_bytes([s0, s1, s2, s3, s4, s5, s6, s7]),
            rmin: i32::from_le_bytes([m0, m1, m2, m3]),
        }
    }
}

/// One group's line and residuals.
#[derive(Copy, Clone, Debug)]
struct Group<'b> {
    /// The group's first tick.
    first: i64,
    /// first + residual minimum: the line's base.
    base: i64,
    slope: i64,
    width: Width,
    /// The residuals from their first byte on; unused at width 0.
    values: &'b [u8],
}

impl<'b> Group<'b> {
    fn new(l: Line, p: Placed<'b>) -> Self {
        Group {
            first: l.first,
            base: l.first.wrapping_add(i64::from(l.rmin)),
            slope: l.slope,
            width: p.width,
            values: p.values,
        }
    }

    /// A stand-in for a suspect group: flat at `first`.
    fn flat(first: i64) -> Self {
        Group {
            first,
            base: first,
            slope: 0,
            width: Width::ZERO,
            values: &[],
        }
    }

    /// Item `j` as stored: base, line and residual, wrapping as the encoder's differences did.
    #[inline]
    fn value(&self, j: usize) -> i64 {
        let r = self.width.read(self.values, j);
        self.base
            .wrapping_add(bitpack::line(self.slope, j))
            .wrapping_add(r.cast_signed())
    }

    /// The items `j` of `1..len` whose band `[base + line(j), base + line(j) + 2^width - 1]` may hold `t`, for a
    /// positive slope; None when the band arithmetic leaves i128.
    fn band(&self, t: i64, len: usize) -> Option<(usize, usize)> {
        let s = i128::from(self.slope);
        let span = i128::from(width_span(self.width.bits()));
        let d = i128::from(t).checked_sub(i128::from(self.base))?;
        let (mut lo, mut hi) = (1usize, len);
        let below = d.checked_sub(span)?.checked_mul(1 << 16)?;
        if below > 0 {
            let k = below.checked_div(s)?;
            lo = lo.max(usize::try_from(k).map_or(len, |k| k.min(len)));
        }
        let atleast = d.checked_mul(1 << 16)?;
        if atleast >= 0 {
            let k = ceil_div(atleast, s)?;
            hi = usize::try_from(k).map_or(hi, |k| hi.min(k));
        }
        Some((lo.min(hi), hi))
    }
}

/// `a / b` rounded up, for `b` above 0.
fn ceil_div(a: i128, b: i128) -> Option<i128> {
    a.checked_neg()?.checked_div_euclid(b)?.checked_neg()
}

/// A starts column of linear groups.
#[derive(Copy, Clone, Debug)]
struct Linear<'b> {
    groups: Groups<'b, RECORD_LEN>,
    /// Read from a damaged column: groups are checked as they are read.
    salvaged: bool,
    /// The chunk's first and last ticks, first at most last.
    first: i64,
    last: i64,
}

impl<'b> Linear<'b> {
    fn line(&self, g: usize) -> Line {
        Line::of(self.groups.record(g))
    }

    /// Whether a record's line is in range: slope 0 to [SLOPE_MAX], residual minimum at most 0, first tick inside
    /// the chunk's ticks (the chunk's first for group 0).
    fn line_ok(&self, g: usize, l: &Line) -> bool {
        (0..SLOPE_MAX).contains(&l.slope)
            && l.rmin <= 0
            && (self.first..=self.last).contains(&l.first)
            && (g > 0 || l.first == self.first)
    }

    /// The last item of group `g` as stored.
    fn last_value(&self, g: usize, h: &Group<'_>) -> Option<i64> {
        let j = group_len(self.groups.n, g).checked_sub(1)?;
        Some(h.value(j))
    }

    /// Group `g` when it reads on its own: fitting the column ([Groups::group_fits]), its line in range and its
    /// first item its first tick.
    fn readable(&self, g: usize) -> Option<Group<'b>> {
        let p = self.groups.group_fits(g)?;
        let l = self.line(g);
        if !self.line_ok(g, &l) {
            return None;
        }
        let h = Group::new(l, p);
        (h.value(0) == h.first).then_some(h)
    }

    /// Group `g` of a salvaged column when it holds up: readable, in order with its readable neighbours (its first
    /// tick at or after the previous group's last item, its last item at or before the next group's first tick),
    /// and the last group ending on the chunk's last tick. Either side of a broken order may be the damaged one, so
    /// both groups fail.
    fn group_ok(&self, g: usize) -> Option<Group<'b>> {
        let h = self.readable(g)?;
        let last = self.last_value(g, &h)?;
        let after_previous = match g.checked_sub(1) {
            None => true,
            Some(p) => self
                .readable(p)
                .and_then(|ph| self.last_value(p, &ph))
                .is_none_or(|l| l <= h.first),
        };
        let before_next = match g.checked_add(1).filter(|&n| n < self.groups.count()) {
            Some(next) => self.readable(next).is_none_or(|nh| last <= nh.first),
            None => last == self.last,
        };
        (after_previous && before_next).then_some(h)
    }

    /// Group `g`'s line; in a salvaged column, a suspect group's is flat at its first tick, kept inside the chunk.
    ///
    /// # Panics
    ///
    /// When `g` is past the last group: the public accessors assert it first.
    fn group(&self, g: usize) -> Group<'b> {
        if !self.salvaged {
            return Group::new(self.line(g), self.groups.checked(g));
        }
        match self.group_ok(g) {
            Some(h) => h,
            None => Group::flat(self.line(g).first.clamp(self.first, self.last)),
        }
    }

    /// Item `j` of a group as read: in a salvaged column, kept inside the chunk's ticks, since a damaged residual
    /// may put it anywhere.
    #[inline]
    fn item(&self, h: &Group<'_>, j: usize) -> i64 {
        let v = h.value(j);
        if self.salvaged {
            v.clamp(self.first, self.last)
        } else {
            v
        }
    }

    fn is_suspect_group(&self, g: usize) -> bool {
        self.salvaged && g < self.groups.count() && self.group_ok(g).is_none()
    }

    /// Check every record of an intact column: placed in the column, lines in range, each group's item 0 its first
    /// tick and its first tick at or after the previous group's last item, the last item the chunk's last tick.
    fn check(&self) -> Result<(), TimesError> {
        let corrupt = |what| TimesError::Corrupt {
            kind: ColumnKind::Starts,
            what,
        };
        self.groups.check()?;
        // the previous group's last item
        let mut prev = self.first;
        for g in 0..self.groups.count() {
            let l = self.line(g);
            if !self.line_ok(g, &l) {
                return Err(corrupt("group record out of range"));
            }
            let h = Group::new(l, self.groups.checked(g));
            if h.value(0) != h.first {
                return Err(corrupt("group's first item isn't its first tick"));
            }
            if h.first < prev {
                return Err(corrupt(
                    "group starts before the previous group's last item",
                ));
            }
            prev = self
                .last_value(g, &h)
                .ok_or(corrupt("group without items"))?;
        }
        if prev != self.last {
            return Err(corrupt("last item isn't the chunk's last tick"));
        }
        Ok(())
    }

    /// The first item starting at or after `t`, for `first < t <= last`.
    fn lower_bound(&self, t: i64) -> usize {
        // groups whose first tick is below t: at least group 0, since t > first
        let k = partition(1, self.groups.count(), |g| self.group(g).first < t);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "partition returns at least its start, 1"
        )]
        let g = k - 1;
        let h = self.group(g);
        let len = group_len(self.groups.n, g);
        let (lo, hi) = if h.slope > 0 {
            h.band(t, len).unwrap_or((1, len))
        } else {
            (1, len)
        };
        let j = partition(lo, hi, |j| self.item(&h, j) < t);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "g is under the group count and j at most its length: an item index, at most n"
        )]
        let i = g * GROUP + j;
        i
    }
}

#[derive(Copy, Clone, Debug)]
enum Repr<'b> {
    Implicit { period: i64 },
    Linear(Linear<'b>),
}

/// The start ticks of a chunk's items, read in place.
///
/// Reads never panic on any bytes; [Starts::get] and [Starts::decode_group] panic only for an item or group past
/// the end. In a column read by salvage, the items of a group whose record doesn't hold up ([Starts::is_suspect])
/// read as that group's first tick, and every item is kept inside the chunk's tick range.
#[derive(Copy, Clone, Debug)]
pub struct Starts<'b> {
    n: usize,
    first: i64,
    last: i64,
    repr: Repr<'b>,
}

impl<'b> Starts<'b> {
    /// Read a starts column of `n` items whose first and last ticks the chunk head names; `column` is its bytes,
    /// `tail` the payload from its first byte on. With `salvage`, group records are checked one by one as read
    /// instead of all at once.
    pub(crate) fn parse(
        n: usize,
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
        if n == 0 {
            return Err(corrupt("no items"));
        }
        if last < first {
            return Err(corrupt("last tick before the first"));
        }
        let repr = match mode {
            TimeMode::Implicit => {
                let period = match *column {
                    [a, b, c, d, e, f, g, h] => i64::from_le_bytes([a, b, c, d, e, f, g, h]),
                    _ => return Err(corrupt("implicit column not one period")),
                };
                let end = i128::try_from(n)
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|k| k.checked_mul(i128::from(period)))
                    .and_then(|span| span.checked_add(i128::from(first)));
                if period < 0 || end != Some(i128::from(last)) {
                    return Err(corrupt("period doesn't reach the last tick"));
                }
                Repr::Implicit { period }
            }
            TimeMode::Linear => {
                let lin = Linear {
                    groups: Groups::split(ColumnKind::Starts, n, LAYOUT, column, tail)?,
                    salvaged: salvage,
                    first,
                    last,
                };
                if !salvage {
                    lin.check()?;
                }
                Repr::Linear(lin)
            }
        };
        Ok(Starts {
            n,
            first,
            last,
            repr,
        })
    }

    /// Items in the chunk.
    pub fn len(&self) -> usize {
        self.n
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
            Repr::Linear(_) => TimeMode::Linear,
        }
    }

    /// The period of an implicit chunk.
    pub fn period(&self) -> Option<i64> {
        match self.repr {
            Repr::Implicit { period } => Some(period),
            Repr::Linear(_) => None,
        }
    }

    /// Groups of [GROUP] items (the last may hold fewer).
    pub fn group_count(&self) -> usize {
        group_count(self.n)
    }

    /// Whether the column was read by salvage.
    pub(crate) fn salvaged(&self) -> bool {
        matches!(self.repr, Repr::Linear(Linear { salvaged: true, .. }))
    }

    /// Whether item `i` is in a group a salvage found damaged; its start is then a stand-in. False past the last
    /// item.
    pub fn is_suspect(&self, i: usize) -> bool {
        self.is_suspect_group(i / GROUP)
    }

    /// Whether group `g` is one a salvage found damaged; false past the last group.
    pub fn is_suspect_group(&self, g: usize) -> bool {
        match self.repr {
            Repr::Implicit { .. } => false,
            Repr::Linear(lin) => lin.is_suspect_group(g),
        }
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
            // the tick lies between the first and the last: the wrapping product gives it exactly
            Repr::Implicit { period } => self.first.wrapping_add(item_i64(i).wrapping_mul(period)),
            Repr::Linear(lin) => lin.item(&lin.group(i / GROUP), i % GROUP),
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
            Repr::Implicit { period } => self.implicit_group_first(g, period),
            Repr::Linear(lin) => lin.group(g).first,
        }
    }

    /// The first tick of group `g` of an implicit column, for `g` under the group count.
    fn implicit_group_first(&self, g: usize, period: i64) -> i64 {
        // the tick lies between the first and the last: wrapping products give it exactly
        let step = period.wrapping_mul(item_i64(GROUP));
        self.first.wrapping_add(item_i64(g).wrapping_mul(step))
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
                let mut t = self.implicit_group_first(g, period);
                for o in out.iter_mut() {
                    *o = t;
                    t = t.wrapping_add(period);
                }
            }
            Repr::Linear(lin) => {
                let h = lin.group(g);
                let r = h
                    .width
                    .decode_line_with(kernel, h.values, h.base, h.slope, out);
                assert!(r.is_ok(), "{kernel:?} doesn't run on this CPU");
                if lin.salvaged {
                    for t in out.iter_mut() {
                        *t = (*t).clamp(self.first, self.last);
                    }
                }
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
                // first < t <= last: the distance is exact, the period above 0
                let d = t.abs_diff(self.first);
                let p = period.unsigned_abs();
                if p == 0 {
                    return self.len();
                }
                usize::try_from(d.div_ceil(p)).map_or(self.len(), |k| k.min(self.len()))
            }
            Repr::Linear(lin) => lin.lower_bound(t),
        }
    }

    /// The last item starting at or before `t`.
    pub fn last_at_or_before(&self, t: i64) -> Option<usize> {
        match t.checked_add(1) {
            Some(t1) => self.lower_bound(t1).checked_sub(1),
            None => self.len().checked_sub(1),
        }
    }

    /// The starts of items `range`, either way.
    ///
    /// # Panics
    ///
    /// When `range` reaches past the last item.
    pub fn range(&self, range: Range<usize>) -> Iter<'b> {
        assert!(range.start <= range.end, "items {range:?} of {}", self.n);
        assert!(range.end <= self.len(), "items {range:?} of {}", self.n);
        Iter::new(*self, range)
    }

    /// The starts of all items, either way.
    pub fn iter(&self) -> Iter<'b> {
        self.range(0..self.len())
    }
}

/// Starts of a range of items, a group decoded at a time.
#[derive(Clone)]
pub struct Iter<'b> {
    starts: Starts<'b>,
    /// The items left, from both ends.
    items: Range<usize>,
    /// The front's group and its decoded starts.
    front_group: Option<usize>,
    front_buf: [i64; GROUP],
    /// The back's group and its decoded starts.
    back_group: Option<usize>,
    back_buf: [i64; GROUP],
}

impl<'b> Iter<'b> {
    fn new(starts: Starts<'b>, items: Range<usize>) -> Self {
        Iter {
            starts,
            items,
            front_group: None,
            front_buf: [0; GROUP],
            back_group: None,
            back_buf: [0; GROUP],
        }
    }
}

/// Item `i` from `buf`, decoding its group into it first when `group` is another.
#[inline]
fn from_group(
    starts: &Starts<'_>,
    group: &mut Option<usize>,
    buf: &mut [i64; GROUP],
    i: usize,
) -> i64 {
    let g = i / GROUP;
    if *group != Some(g) {
        starts.decode_group(g, buf);
        *group = Some(g);
    }
    #[expect(
        clippy::indexing_slicing,
        reason = "i % GROUP is under GROUP, the buffer's length"
    )]
    let t = buf[i % GROUP];
    t
}

impl Iterator for Iter<'_> {
    type Item = i64;

    #[inline]
    fn next(&mut self) -> Option<i64> {
        let i = self.items.next()?;
        Some(from_group(
            &self.starts,
            &mut self.front_group,
            &mut self.front_buf,
            i,
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.items.size_hint()
    }
}

impl DoubleEndedIterator for Iter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<i64> {
        let i = self.items.next_back()?;
        Some(from_group(
            &self.starts,
            &mut self.back_group,
            &mut self.back_buf,
            i,
        ))
    }
}

impl ExactSizeIterator for Iter<'_> {}

/// The smallest and largest residual of ticks `g` (ascending, at least one) at `slope`; None when one leaves i128.
#[cfg(feature = "std")]
fn residuals(g: &[i64], slope: i64) -> Option<(i128, i128)> {
    let first = i128::from(*g.first()?);
    let (mut lo, mut hi) = (i128::MAX, i128::MIN);
    for (j, &x) in g.iter().enumerate() {
        let r = i128::from(x)
            .checked_sub(first)?
            .checked_sub(i128::from(bitpack::line(slope, j)))?;
        lo = lo.min(r);
        hi = hi.max(r);
    }
    Some((lo, hi))
}

/// The width residuals from `lo` to `hi` pack at; a spread past u64 (a fitted slope over ticks near both ends of
/// i64) is wider than any width.
#[cfg(feature = "std")]
fn spread_width((lo, hi): (i128, i128)) -> u32 {
    hi.checked_sub(lo)
        .and_then(|d| u64::try_from(d).ok())
        .map_or(u32::MAX, bitpack::width_of)
}

/// Pick a group's slope and residual minimum: the fitted slope or 0, whichever needs the narrower width (0 on a
/// tie, and when the fitted one's minimum doesn't fit an i32).
#[cfg(feature = "std")]
fn fit(g: &[i64]) -> Option<(i64, i32, u32)> {
    // slope 0: the residuals are the ticks less the first, ascending from 0
    let flat = residuals(g, 0)?;
    let mut best = (0, i32::try_from(flat.0).ok()?, spread_width(flat));
    if let [first, .., last] = g {
        let steps = i128::try_from(g.len().checked_sub(1)?).ok()?;
        let cand = i128::from(*last)
            .checked_sub(i128::from(*first))?
            .checked_mul(1 << 16)?
            .checked_div(steps)?;
        if cand > 0
            && cand < i128::from(SLOPE_MAX)
            && let Ok(cand) = i64::try_from(cand)
        {
            let s = residuals(g, cand)?;
            if spread_width(s) < best.2
                && let Ok(rmin) = i32::try_from(s.0)
            {
                best = (cand, rmin, spread_width(s));
            }
        }
    }
    Some(best)
}

/// Encode ascending `ticks`: the time mode and the column's bytes; None for no ticks, or a column past u32 offsets.
#[cfg(feature = "std")]
pub(crate) fn encode(ticks: &[i64]) -> Option<(TimeMode, std::vec::Vec<u8>)> {
    let (_, rest) = ticks.split_first()?;
    // gaps in i128: two ticks may lie more than i64::MAX apart, and such a period doesn't fit the column
    let mut gaps = ticks
        .iter()
        .zip(rest)
        .map(|(a, b)| i128::from(*b).checked_sub(i128::from(*a)));
    let period = gaps.next().unwrap_or(Some(0));
    if let Some(Ok(period)) = period.map(i64::try_from)
        && gaps.all(|gap| gap == Some(i128::from(period)))
    {
        return Some((TimeMode::Implicit, period.to_le_bytes().to_vec()));
    }
    let column = encode_groups(ticks, |c, offset| {
        let (slope, rmin, w) = fit(c)?;
        let c0 = *c.first()?;
        let mut rec = [0u8; RECORD_LEN];
        crate::bytes::put_i64(&mut rec, 0, c0)?;
        crate::bytes::put_i64(&mut rec, 8, slope)?;
        crate::bytes::put(&mut rec, 16, rmin.to_le_bytes())?;
        crate::bytes::put(&mut rec, 20, [u8::try_from(w).ok()?])?;
        crate::bytes::put_u32(&mut rec, 21, offset)?;
        let base = c0.wrapping_add(i64::from(rmin));
        // the residuals the decoder adds back, wrapping
        let vals = c
            .iter()
            .enumerate()
            .map(|(j, &x)| {
                x.wrapping_sub(base)
                    .wrapping_sub(bitpack::line(slope, j))
                    .cast_unsigned()
            })
            .collect();
        Some((rec, vals, w))
    })?;
    Some((TimeMode::Linear, column))
}
