//! The ALP encoder: sampling, then per group the smallest of the decimal form and the two `rd` forms.
//!
//! Costs are counts of bits or bytes that saturate at the type's maximum, which no form is chosen at; every other
//! step is checked, and a step that can't be taken makes the encoder report the buffer full.

use super::{
    ALP_GROUP, DECIMAL, DICT_BITS_MAX, DICT_MAX, F10, F32, F64, Float, IF10, LEFT_MAX, RD,
};
use crate::bitpack::{self, packed_len};
use crate::samples::raw::mask;
use crate::samples::{ENTRY, Element, GroupWriter, Sample, SampleError};

/// Most groups the first sampling level looks at per column and lane (the paper's `m`, section 4).
const SAMPLE_GROUPS: usize = 8;
/// Groups of a column per group the first level samples: the paper samples 8 vectors of a row-group of 100.
const SAMPLE_GROUPS_PER: (usize, usize) = (8, 100);
/// Values sampled per group, at both levels (the paper's `n` and `s`).
const SAMPLE_VALUES: usize = 32;
/// Combinations kept from the first level (the paper's `k`).
const COMBOS_MAX: usize = 5;
/// Left widths of `rd` the first level weighs: 0 to [LEFT_MAX].
pub(super) const LEFT_WIDTHS: usize = 17;
/// Lanes whose first-level plan the encoder keeps for the whole column.
const PLANS_KEPT: usize = 8;
/// `2^52 + 2^51`: added and taken away, it rounds a double below 2^51 to an integer (section 3.1).
const SWEET: f64 = 6_755_399_441_055_744.0;
/// Magnitude below which [SWEET] rounds.
const ROUND_MAX: f64 = 2_251_799_813_685_248.0;

/// An exponent and a factor.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Combo {
    e: u8,
    f: u8,
}

/// Pick `j` of `s` equidistant picks among `n`: `j · n / s`.
#[inline(always)]
fn spread(j: usize, n: usize, s: usize) -> Option<usize> {
    j.checked_mul(n)?.checked_div(s)
}

/// Lane `l` of samples of `lanes` scalars.
struct Lane<'v, T> {
    values: &'v [T],
    lanes: usize,
    l: usize,
}

impl<T: Sample> Lane<'_, T> {
    /// Bits of sample `i`, None past the end.
    #[inline(always)]
    fn get(&self, i: usize) -> Option<u64> {
        let at = i.checked_mul(self.lanes)?.checked_add(self.l)?;
        self.values.get(at).map(|v| v.to_bits())
    }

    /// Up to [SAMPLE_VALUES] equidistant values of the `n` samples from `first`; None past the values.
    fn sample(&self, first: usize, n: usize, out: &mut [u64; SAMPLE_VALUES]) -> Option<usize> {
        let s = n.min(SAMPLE_VALUES);
        for (j, o) in out.iter_mut().take(s).enumerate() {
            *o = self.get(first.checked_add(spread(j, n, s)?)?)?;
        }
        Some(s)
    }
}

/// `ALP_enc` of a value: `round(n × 10^e × 10^-f)`, None when it is no integer of the scalar's range.
#[inline(always)]
fn enc<F: Float>(bits: u64, c: Combo) -> Option<i64> {
    let x = F::widen(bits) * *F10.get(usize::from(c.e))? * *IF10.get(usize::from(c.f))?;
    if !(x > -ROUND_MAX && x < ROUND_MAX) {
        return None;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "x is inside ±ROUND_MAX (2^51), checked above: the rounded value is an integer an i64 holds"
    )]
    let d = ((x + SWEET) - SWEET) as i64;
    (F::D_MIN..=F::D_MAX).contains(&d).then_some(d)
}

/// The integer of a value that decodes back bit for bit, None for an exception.
#[inline(always)]
fn encode_value<F: Float>(bits: u64, c: Combo, m: F::M) -> Option<i64> {
    let d = enc::<F>(bits, c)?;
    (F::dec(d, m) == bits).then_some(d)
}

/// Bits a decimal lane of `sample` costs with `c`: the packed values and the exceptions; [u64::MAX] as soon as the
/// exceptions alone reach `bound`.
fn decimal_cost<F: Float>(sample: &[u64], c: Combo, bound: u64) -> u64 {
    let Some(m) = F::factors(c.e, c.f) else {
        return u64::MAX;
    };
    let exception_bits = u64::from(F::BITS).saturating_add(8);
    let (mut lo, mut hi, mut exceptions) = (i64::MAX, i64::MIN, 0u64);
    for &bits in sample {
        match encode_value::<F>(bits, c, m) {
            Some(d) => {
                lo = lo.min(d);
                hi = hi.max(d);
            }
            None => {
                exceptions = exceptions.saturating_add(exception_bits);
                if exceptions >= bound {
                    return u64::MAX;
                }
            }
        }
    }
    let width = if lo > hi {
        0
    } else {
        bitpack::width_of(hi.wrapping_sub(lo).cast_unsigned())
    };
    u64::try_from(sample.len())
        .map_or(u64::MAX, |n| n.saturating_mul(u64::from(width)))
        .saturating_add(exceptions)
}

/// The best combination for `sample` over every exponent and factor; ties go to the lower exponent, then factor.
fn best_combo<F: Float>(sample: &[u64]) -> (Combo, u64) {
    let mut best = (Combo::default(), u64::MAX);
    for e in 0..=F::E_MAX {
        for f in 0..=e {
            let c = Combo { e, f };
            let cost = decimal_cost::<F>(sample, c, best.1);
            if cost < best.1 {
                best = (c, cost);
            }
        }
    }
    best
}

/// The left part of a value: its top `left` bits, None when that is more than 16.
#[inline(always)]
fn left_of<F: Float>(bits: u64, left: u32) -> Option<u16> {
    if left == 0 {
        return Some(0);
    }
    u16::try_from(bits.checked_shr(F::BITS.checked_sub(left)?)?).ok()
}

/// A skewed dictionary: the most frequent left parts first (ties to the smaller value), with their counts.
#[derive(Copy, Clone, Debug, Default)]
struct Dict {
    entries: [u16; DICT_MAX],
    counts: [u32; DICT_MAX],
}

impl Dict {
    /// The dictionary of `lefts`, which it sorts.
    fn of(lefts: &mut [u16]) -> Option<Self> {
        lefts.sort_unstable();
        let mut d = Dict::default();
        for run in lefts.chunk_by(|a, b| a == b) {
            d.offer(*run.first()?, u32::try_from(run.len()).ok()?);
        }
        Some(d)
    }

    /// Keep `v` among the most frequent; values come ascending, so an equal count keeps the earlier, smaller one.
    fn offer(&mut self, v: u16, count: u32) {
        let Some(at) = self.counts.iter().position(|c| *c < count) else {
            return;
        };
        if let (Some(entries), Some(counts)) =
            (self.entries.get_mut(at..), self.counts.get_mut(at..))
        {
            // the entries from `at` move down one, the last drops out
            entries.rotate_right(1);
            counts.rotate_right(1);
            if let (Some(e), Some(c)) = (entries.first_mut(), counts.first_mut()) {
                *e = v;
                *c = count;
            }
        }
    }

    /// Values covered by the first `2^code_bits` entries.
    fn covered(&self, code_bits: u32) -> Option<u32> {
        self.counts
            .iter()
            .take(1usize.checked_shl(code_bits)?)
            .try_fold(0u32, |sum, c| sum.checked_add(*c))
    }

    /// The code of `v` in the first `2^code_bits` entries.
    fn code(&self, v: u16, code_bits: u32) -> Option<u8> {
        self.entries
            .iter()
            .take(1usize.checked_shl(code_bits)?)
            .position(|e| *e == v)
            .and_then(|p| u8::try_from(p).ok())
    }
}

/// The shape of an `rd` lane: left bits, the right parts' common trailing zeros, dictionary code bits.
#[derive(Copy, Clone, Debug)]
struct RdShape {
    left: u32,
    zeros: u32,
    code_bits: u32,
}

/// Bytes of an `rd` lane of `n` values of `bits` bits with `exceptions` left parts outside the dictionary: scheme,
/// left, zeros, code bits, the dictionary, the exception count, codes, right parts, positions and left parts.
fn rd_len(n: usize, bits: u32, s: RdShape, exceptions: usize) -> Option<usize> {
    let dict = 1usize.checked_shl(s.code_bits)?.checked_mul(2)?;
    let right = bits.checked_sub(s.left)?.checked_sub(s.zeros)?;
    dict.checked_add(5)?
        .checked_add(packed_len(n, s.code_bits))?
        .checked_add(packed_len(n, right))?
        .checked_add(exceptions.checked_mul(3)?)
}

/// The right parts' common trailing zeros of `vals` at `left` bits: all of the right part when they are all zero.
fn zeros_of<F: Float>(vals: &[u64], left: u32) -> Option<u32> {
    let right = F::BITS.checked_sub(left)?;
    let any = vals.iter().fold(0u64, |a, b| a | b) & mask(right);
    Some(any.trailing_zeros().min(right))
}

/// The smallest dictionary for `n` values at `left` bits and `zeros`: the shape and its bytes.
fn rd_best(dict: &Dict, n: usize, bits: u32, left: u32, zeros: u32) -> Option<(RdShape, usize)> {
    let shape = |code_bits| RdShape {
        left,
        zeros,
        code_bits,
    };
    if left == 0 {
        return Some((shape(0), rd_len(n, bits, shape(0), 0)?));
    }
    let mut best = (shape(0), usize::MAX);
    for code_bits in 0..=DICT_BITS_MAX {
        let covered = usize::try_from(dict.covered(code_bits)?).ok()?;
        let len = rd_len(n, bits, shape(code_bits), n.checked_sub(covered)?)?;
        if len < best.1 {
            best = (shape(code_bits), len);
        }
    }
    Some(best)
}

/// What the first sampling level decides for a lane of a column.
struct Plan {
    combos: [Combo; COMBOS_MAX],
    k: usize,
    /// Left bits of `rd`.
    left: u32,
    /// `rd` won on the sample: groups skip the decimal form.
    rd_only: bool,
}

/// The first sampling level (section 3.2): the best combination of 8 % of the groups, at most [SAMPLE_GROUPS], the
/// [COMBOS_MAX] most frequent kept (ties to the higher exponent, then factor); and the `rd` left width that costs
/// the fewest bits on the same samples (section 3.4).
fn first_level<F: Float, T: Sample>(lane: &Lane<'_, T>, count: usize) -> Option<Plan> {
    let groups = count.div_ceil(ALP_GROUP);
    let picks = groups
        .checked_mul(SAMPLE_GROUPS_PER.0)?
        .div_ceil(SAMPLE_GROUPS_PER.1)
        .clamp(1, SAMPLE_GROUPS);
    let mut found = [(Combo::default(), 0u32); SAMPLE_GROUPS];
    let mut decimal_bytes = 0usize;
    // bytes of `rd` per left width
    let mut rd_bytes = [0usize; LEFT_WIDTHS];
    let left_max = LEFT_MAX.min(F::BITS);
    for j in 0..picks {
        let first = spread(j, groups, picks)?.checked_mul(ALP_GROUP)?;
        let n = count
            .min(first.checked_add(ALP_GROUP)?)
            .checked_sub(first)?;
        let mut sample = [0u64; SAMPLE_VALUES];
        let s = lane.sample(first, n, &mut sample)?;
        let sample = sample.get(..s)?;
        let (c, cost) = best_combo::<F>(sample);
        // a decimal lane's bytes as the first level weighs them: 6 header bytes, the base and the bits
        let cost_bytes = usize::try_from(cost).map_or(usize::MAX, |b| b.div_ceil(8));
        decimal_bytes = decimal_bytes
            .saturating_add(F::BYTES)
            .saturating_add(6)
            .saturating_add(cost_bytes);
        if let Some(slot) = found.iter_mut().find(|x| x.1 == 0 || x.0 == c) {
            *slot = (c, slot.1.saturating_add(1));
        }
        for left in 0..=left_max {
            let mut lefts = [0u16; SAMPLE_VALUES];
            for (o, b) in lefts.iter_mut().zip(sample) {
                *o = left_of::<F>(*b, left)?;
            }
            let dict = Dict::of(lefts.get_mut(..s)?)?;
            let zeros = zeros_of::<F>(sample, left)?;
            let len = rd_best(&dict, s, F::BITS, left, zeros)?.1;
            let total = rd_bytes.get_mut(usize::try_from(left).ok()?)?;
            *total = total.saturating_add(len);
        }
    }
    found.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    let mut plan = Plan {
        combos: [Combo::default(); COMBOS_MAX],
        k: 0,
        left: 0,
        rd_only: false,
    };
    plan.k = plan
        .combos
        .iter_mut()
        .zip(found.iter().filter(|x| x.1 > 0))
        .map(|(o, (c, _))| *o = *c)
        .count();
    let (left, rd) = rd_bytes
        .iter()
        .enumerate()
        .take(usize::try_from(left_max).ok()?.checked_add(1)?)
        .fold(
            (0, usize::MAX),
            |best, (l, b)| if *b < best.1 { (l, *b) } else { best },
        );
    plan.left = u32::try_from(left).ok()?;
    plan.rd_only = rd < decimal_bytes;
    Some(plan)
}

/// The second sampling level (section 3.2): the best of the kept combinations on a group's sample, stopping when
/// two in a row do no better.
fn second_level<F: Float>(plan: &Plan, sample: &[u64]) -> Combo {
    let combos = plan.combos.get(..plan.k).unwrap_or_default();
    let Some((&first, rest)) = combos.split_first() else {
        return Combo::default();
    };
    if rest.is_empty() {
        return first;
    }
    let mut best = (first, decimal_cost::<F>(sample, first, u64::MAX));
    let mut worse_before = false;
    for &c in rest {
        let cost = decimal_cost::<F>(sample, c, best.1);
        if cost < best.1 {
            best = (c, cost);
            worse_before = false;
        } else if worse_before {
            break;
        } else {
            worse_before = true;
        }
    }
    best.0
}

/// A group's lane as the decimal form: the integers, exceptions in place as residual 0.
struct Decimal {
    c: Combo,
    base: i64,
    width: u32,
    d: [i64; ALP_GROUP],
    positions: [u8; ALP_GROUP],
    exceptions: usize,
}

impl Decimal {
    fn new<F: Float>(vals: &[u64; ALP_GROUP], n: usize, c: Combo) -> Option<Self> {
        let mut x = Decimal {
            c,
            base: 0,
            width: 0,
            d: [0; ALP_GROUP],
            positions: [0; ALP_GROUP],
            exceptions: 0,
        };
        let Some(m) = F::factors(c.e, c.f) else {
            // no combination of the sampling is outside the tables; were one, every value is an exception
            for (i, p) in x.positions.iter_mut().take(n).enumerate() {
                *p = u8::try_from(i).ok()?;
            }
            x.exceptions = n;
            return Some(x);
        };
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        let mut ok = [false; ALP_GROUP];
        for (i, ((d, ok), &bits)) in x.d.iter_mut().zip(&mut ok).zip(vals).take(n).enumerate() {
            if let Some(v) = encode_value::<F>(bits, c, m) {
                (*d, *ok) = (v, true);
                lo = lo.min(v);
                hi = hi.max(v);
            } else if let Some(p) = x.positions.get_mut(x.exceptions) {
                *p = u8::try_from(i).ok()?;
                x.exceptions = x.exceptions.checked_add(1)?;
            }
        }
        if lo <= hi {
            x.base = lo;
            x.width = bitpack::width_of(hi.wrapping_sub(lo).cast_unsigned());
        }
        for (d, ok) in x.d.iter_mut().zip(ok).take(n) {
            if !ok {
                *d = x.base;
            }
        }
        Some(x)
    }

    /// Bytes of the lane: scheme, e, f, base, width, exception count, the packed values and the exceptions.
    fn len(&self, n: usize, bytes: usize) -> Option<usize> {
        bytes
            .checked_add(5)?
            .checked_add(packed_len(n, self.width))?
            .checked_add(self.exceptions.checked_mul(bytes.checked_add(1)?)?)
    }

    fn write(
        &self,
        w: &mut GroupWriter<'_>,
        vals: &[u64; ALP_GROUP],
        n: usize,
        bytes: usize,
    ) -> Option<()> {
        w.bytes(&[DECIMAL, self.c.e, self.c.f])?;
        w.uint(self.base.cast_unsigned(), bytes)?;
        w.bytes(&[
            u8::try_from(self.width).ok()?,
            u8::try_from(self.exceptions).ok()?,
        ])?;
        let base = self.base;
        w.packed(
            self.d.iter().map(|d| d.wrapping_sub(base).cast_unsigned()),
            n,
            self.width,
        )?;
        let positions = self.positions.get(..self.exceptions)?;
        w.bytes(positions)?;
        for &p in positions {
            w.uint(*vals.get(usize::from(p))?, bytes)?;
        }
        Some(())
    }
}

/// A group's lane as `rd` of `shape`.
fn write_rd<F: Float>(
    w: &mut GroupWriter<'_>,
    vals: &[u64; ALP_GROUP],
    n: usize,
    shape: RdShape,
    dict: &Dict,
) -> Option<()> {
    let RdShape {
        left,
        zeros,
        code_bits,
    } = shape;
    let right = F::BITS.checked_sub(left)?;
    let right_mask = mask(right);
    w.bytes(&[
        RD,
        u8::try_from(left).ok()?,
        u8::try_from(zeros).ok()?,
        u8::try_from(code_bits).ok()?,
    ])?;
    for e in dict.entries.iter().take(1usize.checked_shl(code_bits)?) {
        w.bytes(&e.to_le_bytes())?;
    }
    let vals = vals.get(..n)?;
    let mut lefts = [0u16; ALP_GROUP];
    for (o, b) in lefts.iter_mut().zip(vals) {
        *o = left_of::<F>(*b, left)?;
    }
    let lefts = lefts.get(..n)?;
    let code = |l: &u16| dict.code(*l, code_bits);
    let exceptions = lefts.iter().filter(|l| code(l).is_none()).count();
    w.u8(u8::try_from(exceptions).ok()?)?;
    // an exception's slot holds code 0; its left part is patched in after the codes
    w.packed(
        lefts.iter().map(|l| u64::from(code(l).unwrap_or(0))),
        n,
        code_bits,
    )?;
    // a shift past the scalar leaves no right bit
    w.packed(
        vals.iter()
            .map(|b| (b & right_mask).checked_shr(zeros).unwrap_or(0)),
        n,
        right.checked_sub(zeros)?,
    )?;
    for (i, l) in lefts.iter().enumerate() {
        if code(l).is_none() {
            w.u8(u8::try_from(i).ok()?)?;
        }
    }
    for l in lefts {
        if code(l).is_none() {
            w.bytes(&l.to_le_bytes())?;
        }
    }
    Some(())
}

/// Code a group's lane: the smallest of the decimal form (unless `rd` won the column), `rd` at the column's left
/// width and `rd` with no left part; ties in that order.
fn write_lane<F: Float>(
    w: &mut GroupWriter<'_>,
    plan: &Plan,
    vals: &[u64; ALP_GROUP],
    n: usize,
) -> Option<()> {
    let group = vals.get(..n)?;
    let plain_shape = RdShape {
        left: 0,
        zeros: zeros_of::<F>(group, 0)?,
        code_bits: 0,
    };
    let plain = rd_len(n, F::BITS, plain_shape, 0)?;
    let zeros = zeros_of::<F>(group, plan.left)?;
    let rd_floor = rd_len(
        n,
        F::BITS,
        RdShape {
            left: plan.left,
            zeros,
            code_bits: 0,
        },
        0,
    )?;
    let decimal = if plan.rd_only {
        None
    } else {
        let mut sample = [0u64; SAMPLE_VALUES];
        let s = n.min(SAMPLE_VALUES);
        for (j, o) in sample.iter_mut().take(s).enumerate() {
            *o = *vals.get(spread(j, n, s)?)?;
        }
        let c = second_level::<F>(plan, sample.get(..s)?);
        Some(Decimal::new::<F>(vals, n, c)?)
    };
    let decimal_len = match &decimal {
        Some(d) => d.len(n, F::BYTES)?,
        None => usize::MAX,
    };
    let rd = if plan.left > 0 && decimal_len > rd_floor {
        let mut lefts = [0u16; ALP_GROUP];
        for (o, b) in lefts.iter_mut().zip(group) {
            *o = left_of::<F>(*b, plan.left)?;
        }
        let dict = Dict::of(lefts.get_mut(..n)?)?;
        let (shape, len) = rd_best(&dict, n, F::BITS, plan.left, zeros)?;
        Some((dict, shape, len))
    } else {
        None
    };
    let rd_bytes = rd.as_ref().map_or(usize::MAX, |r| r.2);
    match (decimal, rd) {
        (Some(d), _) if decimal_len <= rd_bytes && decimal_len <= plain => {
            d.write(w, vals, n, F::BYTES)
        }
        (_, Some((dict, shape, len))) if len <= plain => write_rd::<F>(w, vals, n, shape, &dict),
        _ => write_rd::<F>(w, vals, n, plain_shape, &Dict::default()),
    }
}

fn encode_with<F: Float, T: Sample>(values: &[T], lanes: usize, out: &mut [u8]) -> Option<usize> {
    let count = values.len().checked_div(lanes)?;
    let table = count.div_ceil(ALP_GROUP).checked_mul(ENTRY)?;
    let (entries, data) = out.split_at_mut_checked(table)?;
    let mut w = GroupWriter { out: data, at: 0 };
    // the first level runs once per lane; lanes past the cache run it again per group, giving the same plan
    let mut plans: [Option<Plan>; PLANS_KEPT] = [const { None }; PLANS_KEPT];
    for (g, entry) in entries.as_chunks_mut::<ENTRY>().0.iter_mut().enumerate() {
        let first = g.checked_mul(ALP_GROUP)?;
        let n = count
            .min(first.checked_add(ALP_GROUP)?)
            .checked_sub(first)?;
        for l in 0..lanes {
            let lane = Lane { values, lanes, l };
            let mut vals = [0u64; ALP_GROUP];
            for (j, v) in vals.iter_mut().take(n).enumerate() {
                *v = lane.get(first.checked_add(j)?)?;
            }
            match plans.get_mut(l) {
                Some(slot) => {
                    if slot.is_none() {
                        *slot = Some(first_level::<F, T>(&lane, count)?);
                    }
                    write_lane::<F>(&mut w, slot.as_ref()?, &vals, n)?;
                }
                None => write_lane::<F>(&mut w, &first_level::<F, T>(&lane, count)?, &vals, n)?,
            }
        }
        // max_len keeps the data under 4 GiB
        *entry = u32::try_from(w.at).ok()?.to_le_bytes();
    }
    table.checked_add(w.at)
}

/// Code `values` (samples of `lanes` scalars) into `out`, sized by [super::max_len]; returns the bytes written.
pub(in crate::samples) fn encode<T: Sample>(
    values: &[T],
    lanes: usize,
    out: &mut [u8],
) -> Result<usize, SampleError> {
    let needed = out.len();
    let written = match T::ELEMENT {
        Element::F32 => encode_with::<F32, T>(values, lanes, out),
        Element::F64 => encode_with::<F64, T>(values, lanes, out),
        element => {
            return Err(SampleError::Unsupported {
                codec: crate::Codec::ALP,
                element,
            });
        }
    };
    written.ok_or(SampleError::Full { needed })
}
