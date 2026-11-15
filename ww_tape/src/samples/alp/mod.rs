//! ALP: floats that began as decimals coded as integers, in groups of [ALP_GROUP] samples.
//!
//! Built from Afroozeh, Kuffó and Boncz, "ALP: Adaptive Lossless floating-Point Compression", SIGMOD 2024: section
//! 2.5 (a double as an integer and an exponent), 2.6 (the factor that cuts trailing zeros: `ALP_enc`, `ALP_dec`,
//! formulas 1 and 2), 3.1 (compression, Algorithm 1: one exponent and factor per vector, the fast rounding, exceptions
//! patched after decoding, frame of reference and bit-packing), 3.2 (two-level sampling), 3.3 (decompression,
//! Algorithm 2), 3.4 (`ALP_rd` for real doubles, Algorithm 3: front bits through a skewed dictionary, the rest
//! bit-packed), 4 (the sampling parameters) and 4.4 (the port to 32-bit floats).
//!
//! For data a device controls, fixed-point numbers with [Codec::DELTA](crate::Codec::DELTA) are the default: ALP
//! reaches the same integers by computation. ALP and the XOR codecs serve float sources nobody controls.
//!
//! Per group, lane by lane (`b` = bytes of the scalar, `n` = samples in the group), a scheme byte and its fields:
//!
//! ```text
//! decimal (0):  u8 e, u8 f (f <= e <= 21 for f64, 10 for f32), b base, u8 width, u8 x,
//!               n residuals at width (horizontal, LSB first), x u8 positions (ascending), x × b exception bits
//!               value = (base + residual) × 10^f × 10^-e in the element's arithmetic
//! rd (1):       u8 left bits l (at most 16), u8 zeros z (at most b·8 - l), u8 dictionary bits c (at most 3),
//!               2^c u16 dictionary, u8 x, n codes at c bits, n right parts at (b·8 - l - z) bits,
//!               x u8 positions (ascending), x u16 left parts
//!               value bits = dictionary[code] << (b·8 - l) | right << z, the exceptions' left parts patched in
//! ```
//!
//! An exception is a value the decimal form doesn't give back bit for bit (NaN, infinities, `-0.0`, more digits than
//! the exponent covers) or, in `rd`, a left part outside the dictionary; its slot in the packed values holds residual
//! 0 (the paper's Algorithm 1 puts the first encoded value there; with the frame of reference both cost no bits).
//!
//! Where this differs from the paper: a group is 128 samples (a [bitpack](crate::bitpack) group), not 1024, so
//! positions are a byte; the first sampling level runs per column and lane (the paper's row-group); the scheme is a
//! byte per lane and group, and the encoder takes per group the smallest of the decimal form, `rd` at the column's
//! left width and `rd` with no left part (the bits as they are), measured exactly, so a group never takes more than
//! 7 bytes over its raw size; the dictionary is per group and its size the one that gives the fewest bytes, not the
//! paper's 10 % exception rule. `rd` drops the trailing zero bits the group's right parts share (`z`, not in the
//! paper): an `f32` stored as `f64` keeps 29 zero bits at the bottom that `rd` would otherwise store. `f32` values decode in `f32` arithmetic with exponents up to 10 (the largest power
//! of ten an `f32` holds exactly); the encoder computes its integers in `f64` for both and keeps only what decodes
//! back.
//!
//! The encoder's choices are integer comparisons of bit counts, and its arithmetic is IEEE 754 multiplication, which
//! every platform and SIMD path rounds the same: the same values give the same bytes everywhere.

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

mod decode;
mod encode;
#[cfg(test)]
mod tests;

pub(super) use decode::decode;
pub(super) use encode::encode;

use super::{ENTRY, SampleError};

/// Samples per group: a [bitpack](crate::bitpack) group.
pub const ALP_GROUP: usize = crate::bitpack::GROUP;

/// Scheme byte of a decimal lane.
const DECIMAL: u8 = 0;
/// Scheme byte of an `rd` lane.
const RD: u8 = 1;
/// Widest left part of `rd`.
const LEFT_MAX: u32 = 16;
/// Widest dictionary of `rd`, in bits of the code.
const DICT_BITS_MAX: u32 = 3;
/// Entries of the widest dictionary.
const DICT_MAX: usize = 1 << DICT_BITS_MAX;

/// 10^i, exact up to 10^21.
const F10: [f64; 22] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21,
];
/// 10^-i, rounded.
const IF10: [f64; 22] = [
    1e0, 1e-1, 1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10, 1e-11, 1e-12, 1e-13, 1e-14,
    1e-15, 1e-16, 1e-17, 1e-18, 1e-19, 1e-20, 1e-21,
];
/// 10^i in `f32`, exact up to 10^10.
const F10_32: [f32; 11] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10];
/// 10^-i in `f32`, rounded.
const IF10_32: [f32; 11] = [
    1e0, 1e-1, 1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10,
];

/// What a float element brings to ALP: its width, its exponents and `ALP_dec` in its arithmetic.
trait Float {
    /// Bits of the scalar.
    const BITS: u32;
    /// Bytes of the scalar.
    const BYTES: usize = (Self::BITS / 8) as usize;
    /// Smallest and largest integer a decimal lane holds: the scalar's signed range.
    const D_MIN: i64;
    const D_MAX: i64;
    /// The multipliers of `ALP_dec`: 10^f and 10^-e.
    type M: Copy;

    /// The multipliers for exponent `e` and factor `f`, None when `f > e` or `e` is past the table.
    fn factors(e: u8, f: u8) -> Option<Self::M>;
    /// `ALP_dec`: `d × 10^f × 10^-e`, as the scalar's bits.
    fn dec(d: i64, m: Self::M) -> u64;
    /// The value of `bits` as an `f64`.
    fn widen(bits: u64) -> f64;
}

struct F32;
struct F64;

impl Float for F64 {
    const BITS: u32 = 64;
    const D_MIN: i64 = i64::MIN;
    const D_MAX: i64 = i64::MAX;
    type M = (f64, f64);

    #[inline(always)]
    fn factors(e: u8, f: u8) -> Option<Self::M> {
        if f > e {
            return None;
        }
        Some((*F10.get(usize::from(f))?, *IF10.get(usize::from(e))?))
    }

    #[inline(always)]
    fn dec(d: i64, (f, e): Self::M) -> u64 {
        (d as f64 * f * e).to_bits()
    }

    #[inline(always)]
    fn widen(bits: u64) -> f64 {
        f64::from_bits(bits)
    }
}

impl Float for F32 {
    const BITS: u32 = 32;
    const D_MIN: i64 = i32::MIN as i64;
    const D_MAX: i64 = i32::MAX as i64;
    type M = (f32, f32);

    #[inline(always)]
    fn factors(e: u8, f: u8) -> Option<Self::M> {
        if f > e {
            return None;
        }
        Some((*F10_32.get(usize::from(f))?, *IF10_32.get(usize::from(e))?))
    }

    #[inline(always)]
    fn dec(d: i64, (f, e): Self::M) -> u64 {
        u64::from((d as f32 * f * e).to_bits())
    }

    #[inline(always)]
    fn widen(bits: u64) -> f64 {
        f64::from(f32::from_bits(bits as u32))
    }
}

/// Largest exponent of `F`: the last entry of its tables.
const fn e_max<F: Float>() -> u8 {
    if F::BITS == 32 {
        (F10_32.len() - 1) as u8
    } else {
        (F10.len() - 1) as u8
    }
}

/// Bytes of a lane that keeps the bits as they are: `rd` with no left part, a one-entry dictionary, no exception.
const fn plain_lane_len(n: usize, bytes: usize) -> usize {
    4 + 2 + 1 + n * bytes
}

/// The most bytes `count` samples of `values` scalars of `w` bits code into: the table and, per group and lane,
/// [plain_lane_len], which the encoder never exceeds.
pub(super) fn max_len(count: usize, values: usize, w: u32) -> Option<usize> {
    let groups = count.div_ceil(ALP_GROUP);
    let lanes = values.checked_div(count).unwrap_or(0);
    let heads = groups
        .checked_mul(lanes)?
        .checked_mul(plain_lane_len(0, 0))?;
    let data = values.checked_mul((w / 8) as usize)?;
    groups
        .checked_mul(ENTRY)?
        .checked_add(heads)?
        .checked_add(data)
}

/// Check the group table of a column of `count` samples.
pub(super) fn check_table(bytes: &[u8], count: usize) -> Result<(), SampleError> {
    super::check_table(bytes, count.div_ceil(ALP_GROUP))
}
