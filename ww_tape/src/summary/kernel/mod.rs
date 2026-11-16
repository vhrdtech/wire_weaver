//! Single-pass statistics of a batch of integer samples up to 32 bits: every statistic of a series summary at once.
//!
//! [int_batch] takes the frames of one channel (`C` components each) and keeps per component the minimum, maximum,
//! sum and sum of squares, per frame the clipped count and, for complex frames, the sum and maximum of `I² + Q²`. All
//! of it is exact integer arithmetic, so the order of the frames doesn't change a result and the compiler may
//! vectorise freely. The frames go through in blocks of [BLOCK]: a block keeps narrow lanes ([Narrow::Sum],
//! [Narrow::Sq]) that can't overflow within it, then adds them to the batch's 128-bit totals.
//!
//! A square of a 32-bit value takes up to 64 bits, so squares and powers are kept as two parts, the bits from 32 up
//! and the 32 bits below, each summed in a `u64` lane; a total is `high · 2^32 + low`. Types up to 16 bits have no
//! high part (it is a constant 0, which the compiler drops).
//!
//! The same code runs portable or compiled for AVX2 or AVX-512 ([Kernel]), chosen at run time.

#[cfg(target_arch = "x86_64")]
mod x86;

/// Frames a block keeps in narrow lanes before adding them to the batch's totals.
pub(super) const BLOCK: usize = 64;
/// [BLOCK] as a u128, for the overflow bounds checked at build time.
const BLOCK_WIDE: u128 = 64;
#[expect(
    clippy::as_conversions,
    reason = "a build-time check; From is not const"
)]
const _: () = assert!(BLOCK_WIDE == BLOCK as u128, "BLOCK_WIDE is BLOCK");

/// An accumulator lane: adds without overflow within a block (each type's bounds are checked at build time).
pub(crate) trait Lane: Copy {
    const ZERO: Self;
    /// `self + o`; the bounds of [Narrow] keep it from wrapping.
    fn plus(self, o: Self) -> Self;
    fn wide(self) -> i128;
}

macro_rules! lane {
    ($($t:ty),+) => {$(
        impl Lane for $t {
            const ZERO: Self = 0;
            #[inline(always)]
            fn plus(self, o: Self) -> Self {
                self.wrapping_add(o)
            }
            #[inline(always)]
            fn wide(self) -> i128 {
                i128::from(self)
            }
        }
    )+};
}
lane!(i32, i64, u32, u64);

impl Lane for u128 {
    const ZERO: Self = 0;
    #[inline(always)]
    fn plus(self, o: Self) -> Self {
        self.wrapping_add(o)
    }
    /// A power is under 2^65 (two squares of 32-bit values): an i128 holds it.
    #[inline(always)]
    fn wide(self) -> i128 {
        self.cast_signed()
    }
}

/// An integer scalar of at most 32 bits, with the lanes its block sums fit in.
pub(crate) trait Narrow: Copy + Ord {
    const LOW: Self;
    const HIGH: Self;
    /// A block's sum of values.
    type Sum: Lane;
    /// A block's sum of one part of squares or powers.
    type Sq: Lane;
    /// One power `I² + Q²`, exact.
    type Pow: Lane + Ord;
    fn sum(self) -> Self::Sum;
    /// The square as `(high, low)`: `high · 2^32 + low`.
    fn sq(self) -> (Self::Sq, Self::Sq);
    fn pow(i: Self, q: Self) -> Self::Pow;
    /// The power of `(i, q)` as `(high, low)`: `high · 2^32 + low`.
    fn pow_parts(i: Self, q: Self) -> (Self::Sq, Self::Sq);
}

/// `x` squared as a u64: exact for values up to 32 bits.
#[inline(always)]
fn sq64(x: i64) -> u64 {
    x.wrapping_mul(x).cast_unsigned()
}

/// The bits of `v` from 32 up and below 32.
#[inline(always)]
fn split(v: u64) -> (u64, u64) {
    (v >> 32, v & 0xffff_ffff)
}

/// `a + b` of two squares as `(high, low)` parts, each under 2^33.
#[inline(always)]
fn split_pow(a: u64, b: u64) -> (u64, u64) {
    let ((ah, al), (bh, bl)) = (split(a), split(b));
    (ah.wrapping_add(bh), al.wrapping_add(bl))
}

/// A type up to 16 bits: squares and powers in one part, a square exact in the u32 of an i32's wrapping product. `$abs` is the largest magnitude of a value; the bounds of
/// the lanes are checked at build time.
macro_rules! narrow_small {
    ($t:ty, $sum:ty, $sq:ty, $pow:ty, $abs:expr) => {
        #[expect(
            clippy::as_conversions,
            reason = "a build-time check; From is not const"
        )]
        const _: () = {
            let abs: u128 = $abs;
            assert!(
                BLOCK_WIDE * abs <= <$sum>::MAX as u128,
                "a block's sum fits its lane"
            );
            assert!(
                BLOCK_WIDE * 2 * abs * abs <= <$sq>::MAX as u128,
                "a block's squares and powers fit their lane"
            );
            assert!(
                2 * abs * abs <= <$pow>::MAX as u128,
                "a power fits its type"
            );
        };
        impl Narrow for $t {
            const LOW: Self = <$t>::MIN;
            const HIGH: Self = <$t>::MAX;
            type Sum = $sum;
            type Sq = $sq;
            type Pow = $pow;
            #[inline(always)]
            fn sum(self) -> $sum {
                <$sum>::from(self)
            }
            #[inline(always)]
            fn sq(self) -> ($sq, $sq) {
                let v = i32::from(self);
                (0, v.wrapping_mul(v).cast_unsigned().into())
            }
            #[inline(always)]
            fn pow(i: Self, q: Self) -> $pow {
                let (i, q) = (i32::from(i), i32::from(q));
                <$pow>::from(i.wrapping_mul(i).cast_unsigned())
                    .wrapping_add(q.wrapping_mul(q).cast_unsigned().into())
            }
            #[inline(always)]
            fn pow_parts(i: Self, q: Self) -> ($sq, $sq) {
                (0, <Self as Narrow>::pow(i, q).into())
            }
        }
    };
}

// i8: |x| <= 2^7, squares to 2^14; u8: < 2^8, squares under 2^16
narrow_small!(i8, i32, u32, u32, 1 << 7);
narrow_small!(u8, i32, u32, u32, 1 << 8);
// i16: |x| <= 2^15, a power at most 2^31 (a u32); u16: < 2^16, a power under 2^33
narrow_small!(i16, i32, u64, u32, 1 << 15);
narrow_small!(u16, i32, u64, u64, 1 << 16);

// 32 bits: a sum of 64 values under 2^38 in an i64; a square under 2^64 split in two parts, each under 2^33 for a
// power, so 64 of them under 2^39 in a u64.
impl Narrow for i32 {
    const LOW: Self = i32::MIN;
    const HIGH: Self = i32::MAX;
    type Sum = i64;
    type Sq = u64;
    /// At most `2 · 2^62 = 2^63`.
    type Pow = u64;
    #[inline(always)]
    fn sum(self) -> i64 {
        i64::from(self)
    }
    #[inline(always)]
    fn sq(self) -> (u64, u64) {
        split(sq64(i64::from(self)))
    }
    #[inline(always)]
    fn pow(i: Self, q: Self) -> u64 {
        sq64(i64::from(i)).wrapping_add(sq64(i64::from(q)))
    }
    #[inline(always)]
    fn pow_parts(i: Self, q: Self) -> (u64, u64) {
        split_pow(sq64(i64::from(i)), sq64(i64::from(q)))
    }
}

impl Narrow for u32 {
    const LOW: Self = u32::MIN;
    const HIGH: Self = u32::MAX;
    type Sum = i64;
    type Sq = u64;
    /// Under `2 · 2^64 = 2^65`.
    type Pow = u128;
    #[inline(always)]
    fn sum(self) -> i64 {
        i64::from(self)
    }
    #[inline(always)]
    fn sq(self) -> (u64, u64) {
        split(sq64(i64::from(self)))
    }
    #[inline(always)]
    fn pow(i: Self, q: Self) -> u128 {
        u128::from(sq64(i64::from(i))).wrapping_add(u128::from(sq64(i64::from(q))))
    }
    #[inline(always)]
    fn pow_parts(i: Self, q: Self) -> (u64, u64) {
        split_pow(sq64(i64::from(i)), sq64(i64::from(q)))
    }
}

/// The statistics of a batch of one channel's frames: per component (only the first for a real element) and per frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntBatch<T> {
    pub(crate) frames: u64,
    pub(crate) min: [T; 2],
    pub(crate) max: [T; 2],
    pub(crate) sum: [i128; 2],
    pub(crate) sq: [i128; 2],
    /// Sum of `I² + Q²` (complex frames).
    pub(crate) pow: i128,
    /// Largest `I² + Q²` (complex frames); 0 with no frames.
    pub(crate) pmax: i128,
    /// Frames with a component at or below `lo` or at or above `hi`.
    pub(crate) clipped: u64,
}

impl<T: Narrow> IntBatch<T> {
    pub(crate) const NEW: Self = IntBatch {
        frames: 0,
        min: [T::HIGH; 2],
        max: [T::LOW; 2],
        sum: [0; 2],
        sq: [0; 2],
        pow: 0,
        pmax: 0,
        clipped: 0,
    };
}

/// `high · 2^32 + low` of two part sums.
#[inline(always)]
fn join<L: Lane>(high: L, low: L) -> i128 {
    // a part sum is under 2^64: the total stays under 2^97
    (high.wide() << 32).wrapping_add(low.wide())
}

/// Add frames of `C` components (1, or 2 for complex) to `out`; a component counts as clipped at or below `lo` or at
/// or above `hi`.
#[inline(always)]
pub(crate) fn int_batch<T: Narrow, const C: usize>(
    frames: &[[T; C]],
    lo: T,
    hi: T,
    out: &mut IntBatch<T>,
) {
    for b in frames.chunks(BLOCK) {
        block(b, lo, hi, out);
    }
}

#[inline(always)]
fn block<T: Narrow, const C: usize>(b: &[[T; C]], lo: T, hi: T, out: &mut IntBatch<T>) {
    let mut mn = [T::HIGH; C];
    let mut mx = [T::LOW; C];
    let mut s = [<T::Sum as Lane>::ZERO; C];
    let mut qh = [<T::Sq as Lane>::ZERO; C];
    let mut ql = [<T::Sq as Lane>::ZERO; C];
    let (mut ph, mut pl) = (<T::Sq as Lane>::ZERO, <T::Sq as Lane>::ZERO);
    let mut pm = <T::Pow as Lane>::ZERO;
    // at most BLOCK frames each
    let (mut n, mut clipped) = (0u32, 0u32);
    for f in b {
        let mut clip = false;
        let lanes = mn
            .iter_mut()
            .zip(&mut mx)
            .zip(s.iter_mut().zip(qh.iter_mut().zip(&mut ql)));
        for (&x, ((mn, mx), (s, (qh, ql)))) in f.iter().zip(lanes) {
            *mn = (*mn).min(x);
            *mx = (*mx).max(x);
            *s = s.plus(x.sum());
            let (h, l) = x.sq();
            *qh = qh.plus(h);
            *ql = ql.plus(l);
            clip |= (x <= lo) | (x >= hi);
        }
        n = n.wrapping_add(1);
        clipped = clipped.wrapping_add(u32::from(clip));
        if let Some(&[i, q]) = f.first_chunk::<2>() {
            pm = pm.max(T::pow(i, q));
            let (h, l) = T::pow_parts(i, q);
            ph = ph.plus(h);
            pl = pl.plus(l);
        }
    }
    let parts = mn.iter().zip(&mx).zip(s.iter().zip(qh.iter().zip(&ql)));
    let outs = out
        .min
        .iter_mut()
        .zip(&mut out.max)
        .zip(out.sum.iter_mut().zip(&mut out.sq));
    for (((mn, mx), (s, (qh, ql))), ((omn, omx), (os, oq))) in parts.zip(outs) {
        *omn = (*omn).min(*mn);
        *omx = (*omx).max(*mx);
        // a batch is at most a node's 2^24 frames: the totals stay far inside an i128
        *os = os.wrapping_add(s.wide());
        *oq = oq.wrapping_add(join(*qh, *ql));
    }
    out.pow = out.pow.wrapping_add(join(ph, pl));
    out.pmax = out.pmax.max(pm.wide());
    out.clipped = out.clipped.wrapping_add(u64::from(clipped));
    out.frames = out.frames.wrapping_add(u64::from(n));
}

/// Which code [int_batch] runs as: portable, or compiled for a vector extension this CPU has.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Kernel {
    Portable,
    #[cfg(target_arch = "x86_64")]
    Avx2(x86::Avx2),
    #[cfg(target_arch = "x86_64")]
    Avx512(x86::Avx512),
}

impl Kernel {
    /// The fastest kernel this CPU runs.
    pub(crate) fn best() -> Self {
        Self::all().last().copied().unwrap_or(Kernel::Portable)
    }

    /// Every kernel this CPU runs, slowest first.
    pub(crate) fn all() -> Vec<Self> {
        #[cfg(target_arch = "x86_64")]
        let vector = x86::Avx2::detect()
            .map(Kernel::Avx2)
            .into_iter()
            .chain(x86::Avx512::detect().map(Kernel::Avx512));
        #[cfg(not(target_arch = "x86_64"))]
        let vector = core::iter::empty();
        core::iter::once(Kernel::Portable).chain(vector).collect()
    }

    /// [int_batch] in this kernel.
    #[inline]
    pub(crate) fn int_batch<T: Narrow, const C: usize>(
        self,
        frames: &[[T; C]],
        lo: T,
        hi: T,
        out: &mut IntBatch<T>,
    ) {
        match self {
            Kernel::Portable => int_batch(frames, lo, hi, out),
            #[cfg(target_arch = "x86_64")]
            Kernel::Avx2(k) => k.int_batch(frames, lo, hi, out),
            #[cfg(target_arch = "x86_64")]
            Kernel::Avx512(k) => k.int_batch(frames, lo, hi, out),
        }
    }
}

/// Frames gathered from an interleaved batch of several channels before they go through a kernel.
const GATHER: usize = 256;

/// Add the frames of one channel: `C` components at scalar `at` of each frame of `stride` scalars of `xs`.
pub(crate) fn int_channel<T: Narrow, const C: usize>(
    kernel: Kernel,
    xs: &[T],
    stride: usize,
    at: usize,
    lo: T,
    hi: T,
    out: &mut IntBatch<T>,
) {
    if stride == C && at == 0 {
        let (frames, _) = xs.as_chunks::<C>();
        kernel.int_batch(frames, lo, hi, out);
        return;
    }
    let mut buf = [[T::LOW; C]; GATHER];
    let mut frames = xs
        .chunks_exact(stride)
        .filter_map(|f| f.get(at..).and_then(|f| f.first_chunk::<C>()));
    loop {
        // buf first: a frame is taken only when there is room for it
        let n = buf
            .iter_mut()
            .zip(frames.by_ref())
            .map(|(slot, f)| *slot = *f)
            .count();
        if let Some(got) = buf.get(..n) {
            kernel.int_batch(got, lo, hi, out);
        }
        if n < GATHER {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
