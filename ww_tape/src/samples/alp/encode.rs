//! The ALP encoder: sampling, then per group the smallest of the decimal form and the two `rd` forms.

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

use super::{
    ALP_GROUP, DECIMAL, DICT_BITS_MAX, DICT_MAX, F10, F32, F64, Float, IF10, LEFT_MAX, RD, e_max,
};
use crate::bitpack::{self, packed_len};
use crate::samples::{ENTRY, Element, Sample, SampleError};

/// Most groups the first sampling level looks at per column and lane (the paper's `m`, section 4).
const SAMPLE_GROUPS: usize = 8;
/// Groups of a column per group the first level samples: the paper samples 8 vectors of a row-group of 100.
const SAMPLE_GROUPS_PER: (usize, usize) = (8, 100);
/// Values sampled per group, at both levels (the paper's `n` and `s`).
const SAMPLE_VALUES: usize = 32;
/// Combinations kept from the first level (the paper's `k`).
const COMBOS_MAX: usize = 5;
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

    /// Up to [SAMPLE_VALUES] equidistant values of the `n` samples from `first`.
    fn sample(&self, first: usize, n: usize, out: &mut [u64; SAMPLE_VALUES]) -> usize {
        let s = n.min(SAMPLE_VALUES);
        for (j, o) in out.iter_mut().take(s).enumerate() {
            *o = self.get(first + j * n / s).unwrap_or(0);
        }
        s
    }
}

/// `ALP_enc` of a value: `round(n × 10^e × 10^-f)`, None when it is no integer of the scalar's range.
#[inline(always)]
fn enc<F: Float>(bits: u64, c: Combo) -> Option<i64> {
    let x = F::widen(bits) * *F10.get(usize::from(c.e))? * *IF10.get(usize::from(c.f))?;
    if !(x > -ROUND_MAX && x < ROUND_MAX) {
        return None;
    }
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
    let exception_bits = 8 + u64::from(F::BITS);
    let (mut lo, mut hi, mut exceptions) = (i64::MAX, i64::MIN, 0u64);
    for &bits in sample {
        match encode_value::<F>(bits, c, m) {
            Some(d) => {
                lo = lo.min(d);
                hi = hi.max(d);
            }
            None => {
                exceptions += 1;
                if exceptions * exception_bits >= bound {
                    return u64::MAX;
                }
            }
        }
    }
    let width = if lo > hi {
        0
    } else {
        bitpack::width_of(hi.wrapping_sub(lo) as u64)
    };
    sample.len() as u64 * u64::from(width) + exceptions * exception_bits
}

/// The best combination for `sample` over every exponent and factor; ties go to the lower exponent, then factor.
fn best_combo<F: Float>(sample: &[u64]) -> (Combo, u64) {
    let mut best = (Combo::default(), u64::MAX);
    for e in 0..=e_max::<F>() {
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

/// The left part of a value: its top `left` bits.
#[inline(always)]
fn left_of<F: Float>(bits: u64, left: u32) -> u16 {
    if left == 0 {
        0
    } else {
        (bits >> (F::BITS - left)) as u16
    }
}

/// A skewed dictionary: the most frequent left parts first (ties to the smaller value), with their counts.
#[derive(Copy, Clone, Debug, Default)]
struct Dict {
    entries: [u16; DICT_MAX],
    counts: [u32; DICT_MAX],
}

impl Dict {
    /// The dictionary of `lefts`, which it sorts.
    fn of(lefts: &mut [u16]) -> Self {
        lefts.sort_unstable();
        let mut d = Dict::default();
        let mut i = 0;
        while let Some(&v) = lefts.get(i) {
            let run = lefts
                .get(i..)
                .map_or(0, |r| r.iter().take_while(|x| **x == v).count());
            i += run;
            d.offer(v, run as u32);
        }
        d
    }

    /// Keep `v` among the most frequent; values come ascending, so an equal count keeps the earlier, smaller one.
    fn offer(&mut self, v: u16, count: u32) {
        let Some(at) = self.counts.iter().position(|c| *c < count) else {
            return;
        };
        self.entries.copy_within(at..DICT_MAX - 1, at + 1);
        self.counts.copy_within(at..DICT_MAX - 1, at + 1);
        if let (Some(e), Some(c)) = (self.entries.get_mut(at), self.counts.get_mut(at)) {
            *e = v;
            *c = count;
        }
    }

    /// Values covered by the first `2^code_bits` entries.
    fn covered(&self, code_bits: u32) -> u32 {
        self.counts.iter().take(1 << code_bits).sum()
    }

    /// The code of `v` in the first `2^code_bits` entries.
    fn code(&self, v: u16, code_bits: u32) -> Option<u8> {
        self.entries
            .iter()
            .take(1 << code_bits)
            .position(|e| *e == v)
            .map(|p| p as u8)
    }
}

/// The shape of an `rd` lane: left bits, the right parts' common trailing zeros, dictionary code bits.
#[derive(Copy, Clone, Debug)]
struct RdShape {
    left: u32,
    zeros: u32,
    code_bits: u32,
}

/// Bytes of an `rd` lane of `n` values of `bits` bits with `exceptions` left parts outside the dictionary.
const fn rd_len(n: usize, bits: u32, s: RdShape, exceptions: usize) -> usize {
    4 + 2 * (1 << s.code_bits)
        + 1
        + packed_len(n, s.code_bits)
        + packed_len(n, bits - s.left - s.zeros)
        + exceptions * 3
}

/// The right parts' common trailing zeros of `vals` at `left` bits: all of the right part when they are all zero.
fn zeros_of<F: Float>(vals: &[u64], left: u32) -> u32 {
    let right = F::BITS - left;
    let any = vals.iter().fold(0u64, |a, b| a | b) & right_mask(right);
    any.trailing_zeros().min(right)
}

/// The low `right` bits set.
const fn right_mask(right: u32) -> u64 {
    if right >= 64 {
        u64::MAX
    } else {
        (1u64 << right) - 1
    }
}

/// The smallest dictionary for `n` values at `left` bits and `zeros`: the shape and its bytes.
fn rd_best(dict: &Dict, n: usize, bits: u32, left: u32, zeros: u32) -> (RdShape, usize) {
    let shape = |code_bits| RdShape {
        left,
        zeros,
        code_bits,
    };
    if left == 0 {
        return (shape(0), rd_len(n, bits, shape(0), 0));
    }
    let mut best = (shape(0), usize::MAX);
    for code_bits in 0..=DICT_BITS_MAX {
        let exceptions = n - dict.covered(code_bits) as usize;
        let len = rd_len(n, bits, shape(code_bits), exceptions);
        if len < best.1 {
            best = (shape(code_bits), len);
        }
    }
    best
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
fn first_level<F: Float, T: Sample>(lane: &Lane<'_, T>, count: usize) -> Plan {
    let groups = count.div_ceil(ALP_GROUP);
    let picks = (groups * SAMPLE_GROUPS_PER.0)
        .div_ceil(SAMPLE_GROUPS_PER.1)
        .clamp(1, SAMPLE_GROUPS);
    let mut found = [(Combo::default(), 0u32); SAMPLE_GROUPS];
    let mut decimal_bytes = 0usize;
    let mut rd_bytes = [0usize; LEFT_MAX as usize + 1];
    let left_max = LEFT_MAX.min(F::BITS);
    for j in 0..picks {
        let g = j * groups / picks;
        let first = g * ALP_GROUP;
        let mut sample = [0u64; SAMPLE_VALUES];
        let s = lane.sample(first, count.min(first + ALP_GROUP) - first, &mut sample);
        let sample = sample.get(..s).unwrap_or_default();
        let (c, cost) = best_combo::<F>(sample);
        decimal_bytes += 6 + F::BYTES + (cost as usize).div_ceil(8);
        if let Some(slot) = found.iter_mut().find(|x| x.1 == 0 || x.0 == c) {
            *slot = (c, slot.1 + 1);
        }
        for (left, total) in rd_bytes.iter_mut().enumerate().take(left_max as usize + 1) {
            let mut lefts = [0u16; SAMPLE_VALUES];
            for (o, b) in lefts.iter_mut().zip(sample) {
                *o = left_of::<F>(*b, left as u32);
            }
            let dict = Dict::of(lefts.get_mut(..s).unwrap_or_default());
            let zeros = zeros_of::<F>(sample, left as u32);
            *total += rd_best(&dict, s, F::BITS, left as u32, zeros).1;
        }
    }
    found.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    let mut plan = Plan {
        combos: [Combo::default(); COMBOS_MAX],
        k: 0,
        left: 0,
        rd_only: false,
    };
    for (o, (c, _)) in plan.combos.iter_mut().zip(found.iter().filter(|x| x.1 > 0)) {
        *o = *c;
        plan.k += 1;
    }
    let (left, rd) = rd_bytes
        .iter()
        .enumerate()
        .take(left_max as usize + 1)
        .fold(
            (0, usize::MAX),
            |best, (l, b)| if *b < best.1 { (l, *b) } else { best },
        );
    plan.left = left as u32;
    plan.rd_only = rd < decimal_bytes;
    plan
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
    let mut worse = 0;
    for &c in rest {
        let cost = decimal_cost::<F>(sample, c, best.1);
        if cost < best.1 {
            best = (c, cost);
            worse = 0;
        } else {
            worse += 1;
            if worse == 2 {
                break;
            }
        }
    }
    best.0
}

/// Writes into the buffer [super::max_len] sized; None when it would run past it.
struct Writer<'o> {
    out: &'o mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn bytes(&mut self, b: &[u8]) -> Option<()> {
        let end = self.at.checked_add(b.len())?;
        self.out.get_mut(self.at..end)?.copy_from_slice(b);
        self.at = end;
        Some(())
    }

    fn u8(&mut self, v: u8) -> Option<()> {
        self.bytes(&[v])
    }

    /// The low `n` bytes of `v`, little endian.
    fn uint(&mut self, v: u64, n: usize) -> Option<()> {
        self.bytes(v.to_le_bytes().get(..n)?)
    }

    fn packed(&mut self, vals: impl IntoIterator<Item = u64>, n: usize, width: u32) -> Option<()> {
        let end = self.at.checked_add(packed_len(n, width))?;
        bitpack::pack(
            self.out.get_mut(self.at..end)?,
            vals.into_iter().take(n),
            width,
        );
        self.at = end;
        Some(())
    }
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
    fn new<F: Float>(vals: &[u64; ALP_GROUP], n: usize, c: Combo) -> Self {
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
                *p = i as u8;
            }
            x.exceptions = n;
            return x;
        };
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        let mut ok = [false; ALP_GROUP];
        for (i, ((d, ok), &bits)) in x.d.iter_mut().zip(&mut ok).zip(vals).take(n).enumerate() {
            if let Some(v) = encode_value::<F>(bits, c, m) {
                (*d, *ok) = (v, true);
                lo = lo.min(v);
                hi = hi.max(v);
            } else if let Some(p) = x.positions.get_mut(x.exceptions) {
                *p = i as u8;
                x.exceptions += 1;
            }
        }
        if lo <= hi {
            x.base = lo;
            x.width = bitpack::width_of(hi.wrapping_sub(lo) as u64);
        }
        for (d, ok) in x.d.iter_mut().zip(ok).take(n) {
            if !ok {
                *d = x.base;
            }
        }
        x
    }

    fn len(&self, n: usize, bytes: usize) -> usize {
        1 + 1 + 1 + bytes + 1 + 1 + packed_len(n, self.width) + self.exceptions * (1 + bytes)
    }

    fn write(
        &self,
        w: &mut Writer<'_>,
        vals: &[u64; ALP_GROUP],
        n: usize,
        bytes: usize,
    ) -> Option<()> {
        w.bytes(&[DECIMAL, self.c.e, self.c.f])?;
        w.uint(self.base as u64, bytes)?;
        w.bytes(&[self.width as u8, self.exceptions as u8])?;
        let base = self.base;
        w.packed(
            self.d.iter().map(|d| d.wrapping_sub(base) as u64),
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
    w: &mut Writer<'_>,
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
    let right = F::BITS - left;
    let mask = right_mask(right);
    w.bytes(&[RD, left as u8, zeros as u8, code_bits as u8])?;
    for e in dict.entries.iter().take(1 << code_bits) {
        w.bytes(&e.to_le_bytes())?;
    }
    let vals = vals.get(..n)?;
    let code = |b: &u64| dict.code(left_of::<F>(*b, left), code_bits);
    let exceptions = vals.iter().filter(|b| code(b).is_none()).count();
    w.u8(exceptions as u8)?;
    w.packed(
        vals.iter().map(|b| u64::from(code(b).unwrap_or(0))),
        n,
        code_bits,
    )?;
    w.packed(
        vals.iter()
            .map(|b| (b & mask).checked_shr(zeros).unwrap_or(0)),
        n,
        right - zeros,
    )?;
    for (i, b) in vals.iter().enumerate() {
        if code(b).is_none() {
            w.u8(i as u8)?;
        }
    }
    for b in vals {
        if code(b).is_none() {
            w.bytes(&left_of::<F>(*b, left).to_le_bytes())?;
        }
    }
    Some(())
}

/// Code a group's lane: the smallest of the decimal form (unless `rd` won the column), `rd` at the column's left
/// width and `rd` with no left part; ties in that order.
fn write_lane<F: Float>(
    w: &mut Writer<'_>,
    plan: &Plan,
    vals: &[u64; ALP_GROUP],
    n: usize,
) -> Option<()> {
    let group = vals.get(..n)?;
    let plain_shape = RdShape {
        left: 0,
        zeros: zeros_of::<F>(group, 0),
        code_bits: 0,
    };
    let plain = rd_len(n, F::BITS, plain_shape, 0);
    let zeros = zeros_of::<F>(group, plan.left);
    let rd_floor = rd_len(
        n,
        F::BITS,
        RdShape {
            left: plan.left,
            zeros,
            code_bits: 0,
        },
        0,
    );
    let decimal = (!plan.rd_only).then(|| {
        let mut sample = [0u64; SAMPLE_VALUES];
        let s = n.min(SAMPLE_VALUES);
        for (j, o) in sample.iter_mut().take(s).enumerate() {
            *o = vals.get(j * n / s).copied().unwrap_or(0);
        }
        let c = second_level::<F>(plan, sample.get(..s).unwrap_or_default());
        Decimal::new::<F>(vals, n, c)
    });
    let decimal_len = decimal.as_ref().map_or(usize::MAX, |d| d.len(n, F::BYTES));
    let rd = (plan.left > 0 && decimal_len > rd_floor).then(|| {
        let mut lefts = [0u16; ALP_GROUP];
        for (o, b) in lefts.iter_mut().zip(group) {
            *o = left_of::<F>(*b, plan.left);
        }
        let dict = Dict::of(lefts.get_mut(..n).unwrap_or_default());
        let (shape, len) = rd_best(&dict, n, F::BITS, plan.left, zeros);
        (dict, shape, len)
    });
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
    let groups = count.div_ceil(ALP_GROUP);
    let table = groups.checked_mul(ENTRY)?;
    let mut w = Writer { out, at: table };
    // the first level runs once per lane; lanes past the cache run it again per group, giving the same plan
    let mut plans: [Option<Plan>; PLANS_KEPT] = [const { None }; PLANS_KEPT];
    for g in 0..groups {
        let first = g * ALP_GROUP;
        let n = count.min(first + ALP_GROUP) - first;
        for l in 0..lanes {
            let lane = Lane { values, lanes, l };
            let mut vals = [0u64; ALP_GROUP];
            for (j, v) in vals.iter_mut().take(n).enumerate() {
                *v = lane.get(first + j)?;
            }
            match plans.get_mut(l) {
                Some(slot) => {
                    let plan = slot.get_or_insert_with(|| first_level::<F, T>(&lane, count));
                    write_lane::<F>(&mut w, plan, &vals, n)?;
                }
                None => write_lane::<F>(&mut w, &first_level::<F, T>(&lane, count), &vals, n)?,
            }
        }
        let end = u32::try_from(w.at - table).ok()?;
        w.out
            .get_mut(g * ENTRY..(g + 1) * ENTRY)?
            .copy_from_slice(&end.to_le_bytes());
    }
    Some(w.at)
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
