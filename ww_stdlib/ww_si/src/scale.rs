//! The numbers behind quantities on exact_units' integer kernel: conversions and comparisons across run-time
//! prefixes through [PrefixRatio], exact for integers whatever the prefixes (SI, binary, `Pow10`, `Pow2`,
//! `FullScale`), rounded and settled by a named [Rounding] and [Overflow].
//!
//! An integer value is an [Integer], sign and [Magnitude] at the widest enabled number type, and the kernel works on
//! the magnitude at that width (or at the target's, when that is wider): one instantiation per firmware, `u32` limbs
//! and `u64` products, no `f32`, `f64` or 128-bit code unless a float or 128-bit type is enabled (D2, D3 of the ww_si
//! on exact_units plan). Float values have their own arms, each behind its feature: an `F32` is scaled by the kernel
//! in integers too ([PrefixRatio::apply_f32]), so `f32` brings no `f64` code either.

use crate::Prefix;
use crate::bounds::Limit;
use core::cmp::Ordering;
use exact_units::num::Int;
use exact_units::{Overflow, PrefixRatio, Rounding};
use ww_numeric::{Integer, Magnitude, NumericValue};

mod sealed {
    pub trait Sealed {}
}

/// An unsigned primitive integer the kernel computes a magnitude in. Implementation detail of [ToIn].
#[doc(hidden)]
pub trait Wide: Int + Copy {
    /// 0.
    const ZERO: Self;
    /// `m`, which is never wider than `Self` where it is called ([Wider] picks `Self`).
    fn of(m: Magnitude) -> Self;
    /// The value, zero-extended.
    fn to_u128(self) -> u128;
}

macro_rules! wide {
    ($($t:ty),*) => {$(
        impl Wide for $t {
            const ZERO: Self = 0;

            #[inline(always)]
            #[allow(clippy::unnecessary_cast)]
            fn of(m: Magnitude) -> Self {
                m as $t
            }

            #[inline(always)]
            #[allow(clippy::unnecessary_cast)]
            fn to_u128(self) -> u128 {
                self as u128
            }
        }
    )*};
}

wide!(u32, u64, u128);

/// The wider of two unsigned widths: what a conversion to a target type computes in, at least the [Magnitude] of
/// the enabled number types. Implementation detail of [ToIn].
#[doc(hidden)]
pub trait Wider<B> {
    type Out: Wide;
}

macro_rules! wider {
    ($($a:ty, $b:ty => $out:ty;)*) => {$(
        impl Wider<$b> for $a {
            type Out = $out;
        }
    )*};
}

wider! {
    u32, u32 => u32; u32, u64 => u64; u32, u128 => u128;
    u64, u32 => u64; u64, u64 => u64; u64, u128 => u128;
    u128, u32 => u128; u128, u64 => u128; u128, u128 => u128;
}

/// The number types [Quantity::to_in](crate::Quantity::to_in) gives: the primitive integers `u8` .. `u128` and
/// `i8` .. `i128`, and `f32` / `f64` with ww_si's `f32` / `f64` features (no float target without them, so a stray
/// float conversion on a device is a compile error, not code in its ELF). Sealed.
pub trait ToIn: Copy + sealed::Sealed {
    #[doc(hidden)]
    fn of_value(
        value: &NumericValue,
        from: Prefix,
        to: Prefix,
        r: Rounding,
        o: Overflow,
    ) -> Option<Self>;
}

/// A magnitude scaled by the kernel: the sign, the magnitude (its low bits when it overflowed and the conversion
/// wraps, else 0 when it overflowed), and whether it overflowed `W`.
#[derive(Copy, Clone)]
pub(crate) struct Scaled<W> {
    pub(crate) neg: bool,
    pub(crate) mag: W,
    pub(crate) overflowed: bool,
}

/// The rounding of `-v` that gives `-(rounding of v)`: on a magnitude, floor and ceiling swap below zero.
#[inline(always)]
fn on_magnitude(r: Rounding, neg: bool) -> Rounding {
    match (r, neg) {
        (Rounding::Floor, true) => Rounding::Ceil,
        (Rounding::Ceil, true) => Rounding::Floor,
        (r, _) => r,
    }
}

/// `±m · ratio` rounded by `r`, the magnitude in `W`; `None` past the kernel's room, which no prefix ratio reaches.
#[inline(always)]
fn scale_magnitude<X: Int, W: Wide>(
    neg: bool,
    m: X,
    ratio: &PrefixRatio,
    r: Rounding,
    wrap: bool,
) -> Option<Scaled<W>> {
    let r = on_magnitude(r, neg);
    Some(match ratio.requantize::<X, W>(m, r, Overflow::Checked) {
        Some(mag) => Scaled {
            neg,
            mag,
            overflowed: false,
        },
        None if wrap => Scaled {
            neg,
            mag: ratio.requantize::<X, W>(m, r, Overflow::Wrap)?,
            overflowed: true,
        },
        None => Scaled {
            neg,
            mag: W::ZERO,
            overflowed: true,
        },
    })
}

/// `value` in units of `from` scaled to units of `to`, rounded by `r`, the magnitude in `W`. `None` for
/// [Prefix::Unknown], NaN and `UN` / `IN`.
#[inline]
pub(crate) fn scale_value<W: Wide>(
    value: &NumericValue,
    from: Prefix,
    to: Prefix,
    r: Rounding,
    wrap: bool,
) -> Option<Scaled<W>> {
    let x = value.to_integer();
    // the same prefix, the common case on a device: nothing to scale (a scale from a newer version isn't known to be
    // the same, it converts to nothing)
    if from == to
        && !matches!(from, Prefix::Unknown(_))
        && let Some(x) = x
    {
        return Some(Scaled {
            neg: x.is_negative(),
            mag: W::of(x.magnitude()),
            overflowed: false,
        });
    }
    let ratio = from.ratio_to(to)?;
    match x {
        Some(x) => scale_magnitude(x.is_negative(), W::of(x.magnitude()), &ratio, r, wrap),
        None => scale_float(value, &ratio, r, wrap),
    }
}

/// A float value scaled by `ratio` to the nearest float (one rounding of the exact product), then rounded to an
/// integer by `r`: exactly, in integers. `None` for NaN, `UN` / `IN`, and without float types.
#[allow(unused_variables)]
#[inline(never)]
fn scale_float<W: Wide>(
    value: &NumericValue,
    ratio: &PrefixRatio,
    r: Rounding,
    wrap: bool,
) -> Option<Scaled<W>> {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => {
            let bits = ratio.apply_f32(x).to_bits();
            let (exp, frac) = ((bits >> 23) & 0xFF, bits & 0x7F_FFFF);
            let neg = bits >> 31 == 1;
            match (exp, frac) {
                (0xFF, 0) => Some(infinite(neg)),
                (0xFF, _) => None,
                (0, _) => scale_pow2::<u32, W>(neg, frac, -149, r, wrap),
                (e, f) => scale_pow2::<u32, W>(neg, f | 1 << 23, e as i32 - 150, r, wrap),
            }
        }
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => {
            let bits = ratio.apply_f64(x).to_bits();
            let (exp, frac) = ((bits >> 52) & 0x7FF, bits & ((1 << 52) - 1));
            let neg = bits >> 63 == 1;
            match (exp, frac) {
                (0x7FF, 0) => Some(infinite(neg)),
                (0x7FF, _) => None,
                (0, _) => scale_pow2::<u64, W>(neg, frac, -1074, r, wrap),
                (e, f) => scale_pow2::<u64, W>(neg, f | 1 << 52, e as i32 - 1075, r, wrap),
            }
        }
        _ => None,
    }
}

/// ±∞: beyond every integer, nothing to wrap.
#[cfg(any(feature = "f32", feature = "f64"))]
fn infinite<W: Wide>(neg: bool) -> Scaled<W> {
    Scaled {
        neg,
        mag: W::ZERO,
        overflowed: true,
    }
}

/// `±m · 2^e` rounded by `r`, exactly: the float's own value, `m` its significand.
#[cfg(any(feature = "f32", feature = "f64"))]
fn scale_pow2<X: Int, W: Wide>(
    neg: bool,
    m: X,
    e: i32,
    r: Rounding,
    wrap: bool,
) -> Option<Scaled<W>> {
    // `Prefix::from_pow2` takes -63..=63, a ratio between prefixes no more
    let pow2 = |e: i32| Prefix::from_pow2(e as i8)?.ratio_to(Prefix::Unit);
    if (-63..=63).contains(&e) {
        return scale_magnitude(neg, m, &pow2(e)?, r, wrap);
    }
    if e < 0 {
        // a significand of at most 53 bits times 2^-64 or less: above 0 and below a half
        let up = on_magnitude(r, neg) == Rounding::Ceil;
        let mag = if up {
            scale_magnitude::<u8, W>(false, 1, &PrefixRatio::ONE, r, wrap)?.mag
        } else {
            W::ZERO
        };
        return Some(Scaled {
            neg,
            mag,
            overflowed: false,
        });
    }
    // a whole number: 2^63 first, then the rest, the low bits kept on the way when the result wraps
    let s = scale_magnitude::<X, W>(neg, m, &pow2(63)?, r, wrap)?;
    if s.overflowed && !wrap {
        return Some(s);
    }
    let rest = scale_magnitude::<W, W>(false, s.mag, &pow2(e - 63)?, r, wrap)?;
    Some(Scaled {
        neg,
        mag: rest.mag,
        overflowed: s.overflowed || rest.overflowed,
    })
}

macro_rules! to_in_int {
    ($($t:ty, $unsigned:ty, $signed:literal;)*) => {$(
        impl sealed::Sealed for $t {}

        impl ToIn for $t {
            #[inline]
            fn of_value(
                value: &NumericValue,
                from: Prefix,
                to: Prefix,
                r: Rounding,
                o: Overflow,
            ) -> Option<Self> {
                let s = scale_value::<<$unsigned as Wider<Magnitude>>::Out>(
                    value,
                    from,
                    to,
                    r,
                    o == Overflow::Wrap,
                )?;
                let m = s.mag.to_u128();
                let neg = s.neg && m != 0;
                // two's complement of the magnitude's low bits: the value modulo 2^BITS
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let wrapped = if neg { (m as $t).wrapping_neg() } else { m as $t };
                #[allow(clippy::cast_sign_loss, unused_comparisons)]
                let max = <$t>::MAX as u128;
                let fits = !s.overflowed && if neg { $signed && m <= max + 1 } else { m <= max };
                if fits {
                    return Some(wrapped);
                }
                match o {
                    Overflow::Checked => None,
                    Overflow::Saturate => Some(if s.neg { <$t>::MIN } else { <$t>::MAX }),
                    Overflow::Wrap => Some(wrapped),
                }
            }
        }
    )*};
}

to_in_int! {
    u8, u32, false; u16, u32, false; u32, u32, false; u64, u64, false; u128, u128, false;
    i8, u32, true; i16, u32, true; i32, u32, true; i64, u64, true; i128, u128, true;
}

/// The integer of an [Integer] as `f32`, the nearest one.
#[cfg(feature = "f32")]
fn integer_f32(x: Integer) -> f32 {
    let m = x.magnitude() as f32;
    if x.is_negative() { -m } else { m }
}

/// A float target is the float nearest the exact product (±∞ past its range): `Rounding` and `Overflow` are for
/// integer targets. An integer value is taken to the nearest `f32` first.
#[cfg(feature = "f32")]
impl sealed::Sealed for f32 {}

#[cfg(feature = "f32")]
impl ToIn for f32 {
    fn of_value(
        value: &NumericValue,
        from: Prefix,
        to: Prefix,
        _r: Rounding,
        _o: Overflow,
    ) -> Option<Self> {
        let ratio = from.ratio_to(to)?;
        match *value {
            NumericValue::F32(x) => Some(ratio.apply_f32(x)),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => Some(ratio.apply_f64(x) as f32),
            v => Some(ratio.apply_f32(integer_f32(v.to_integer()?))),
        }
    }
}

/// Like `f32`'s: the nearest `f64`, an integer value taken to the nearest `f64` first.
#[cfg(feature = "f64")]
impl sealed::Sealed for f64 {}

#[cfg(feature = "f64")]
impl ToIn for f64 {
    fn of_value(
        value: &NumericValue,
        from: Prefix,
        to: Prefix,
        _r: Rounding,
        _o: Overflow,
    ) -> Option<Self> {
        let ratio = from.ratio_to(to)?;
        let x = match *value {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => f64::from(x),
            NumericValue::F64(x) => x,
            v => {
                let x = v.to_integer()?;
                let m = x.magnitude() as f64;
                if x.is_negative() { -m } else { m }
            }
        };
        Some(ratio.apply_f64(x))
    }
}

/// `x` (an integer in units of `from`) in units of `to`, if that is a whole number that fits: scaling to a finer
/// prefix that the coarser is a whole multiple of always is, if it fits.
pub(crate) fn exact_between(x: Integer, from: Prefix, to: Prefix) -> Option<Integer> {
    let m = from
        .ratio_to(to)?
        .apply::<Magnitude, Magnitude>(x.magnitude())?;
    Some(Integer::new(x.is_negative(), m))
}

/// The finer of two prefixes (the smaller scale), `a` when they are equal; `b` when either is
/// [Unknown](Prefix::Unknown), and the conversion to it is refused.
pub(crate) fn finer(a: Prefix, b: Prefix) -> Prefix {
    match a.cmp_scale(b) {
        Some(Ordering::Less | Ordering::Equal) => a,
        _ => b,
    }
}

/// -1, 0 or 1.
fn sign(x: Integer) -> i8 {
    if x.magnitude() == 0 {
        0
    } else if x.is_negative() {
        -1
    } else {
        1
    }
}

/// Compares `x` in units of `a` with `y` in units of `b`, exactly; `None` when one prefix is
/// [Unknown](Prefix::Unknown) and the other isn't the same.
fn cmp_integer(a: Prefix, x: Integer, b: Prefix, y: Integer) -> Option<Ordering> {
    if a == b {
        return Some(x.cmp(&y));
    }
    let (sx, sy) = (sign(x), sign(y));
    if sx != sy || sx == 0 {
        return Some(sx.cmp(&sy));
    }
    let o = a.cmp_scaled::<Magnitude>(x.magnitude(), b, y.magnitude())?;
    Some(if sx < 0 { o.reverse() } else { o })
}

/// Compares the float `x` in units of `a` with the integer `y` in units of `b`, exactly (the float's own value);
/// `None` for NaN.
#[cfg(feature = "f32")]
fn cmp_f32_integer(a: Prefix, x: f32, b: Prefix, y: Integer) -> Option<Ordering> {
    let abs = f32::from_bits(x.to_bits() & 0x7FFF_FFFF);
    if abs != abs {
        return None;
    }
    let sx = if abs == 0.0 {
        0
    } else if x.is_sign_negative() {
        -1
    } else {
        1
    };
    let sy = sign(y);
    if sx != sy || sx == 0 {
        return Some(sx.cmp(&sy));
    }
    let o = a.cmp_scaled_f32::<Magnitude>(abs, b, y.magnitude())?;
    Some(if sx < 0 { o.reverse() } else { o })
}

/// Compares two floats in units of `a` and `b`: `x` scaled to `b` (the nearest `f32` of the exact product) against
/// `y`, which orders them exactly unless the scaled `x` equals `y`; that tie is settled exactly too where `x` scales
/// by a power of two without leaving the normal floats.
#[cfg(feature = "f32")]
fn cmp_f32(a: Prefix, x: f32, b: Prefix, y: f32) -> Option<Ordering> {
    if a == b {
        return x.partial_cmp(&y);
    }
    let ratio = a.ratio_to(b)?;
    let o = ratio.apply_f32(x).partial_cmp(&y)?;
    if o != Ordering::Equal || y == 0.0 || !y.is_finite() {
        return Some(o);
    }
    // y = my · 2^ey: x · ratio against y is x · 2^-ey · ratio against my, both exact where x · 2^-ey stays normal
    let bits = y.to_bits();
    let (exp, frac) = ((bits >> 23) & 0xFF, bits & 0x7F_FFFF);
    let (my, ey) = if exp == 0 {
        (frac, -149)
    } else {
        (frac | 1 << 23, exp as i32 - 150)
    };
    let shifted = (-63..=63)
        .contains(&-ey)
        .then(|| Prefix::from_pow2((-ey) as i8))
        .flatten()
        .and_then(|p| Prefix::Unit.ratio_to(p))
        .map(|r| r.apply_f32(x))
        .filter(|s| s.is_normal() && x.is_normal());
    let Some(s) = shifted else {
        return Some(o);
    };
    let abs = f32::from_bits(s.to_bits() & 0x7FFF_FFFF);
    let o = ratio.cmp_scaled_f32::<u32>(abs, my)?;
    Some(if y < 0.0 { o.reverse() } else { o })
}

/// Compares `a` in units of `a_prefix` with `b` in units of `b_prefix`: exactly for integers whatever the prefixes,
/// on the kernel at the width of the enabled number types; an `F32` against an integer exactly too, two `F32`s
/// through the scaled float ([cmp_f32]); an `F64` through `f64` (hosts). `None` for NaN, `UN` / `IN`, and a
/// [Prefix::Unknown] against another prefix.
pub(crate) fn compare(
    a_prefix: Prefix,
    a: &NumericValue,
    b_prefix: Prefix,
    b: &NumericValue,
) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (a.to_integer(), b.to_integer()) {
        return cmp_integer(a_prefix, x, b_prefix, y);
    }
    compare_float(a_prefix, a, b_prefix, b)
}

#[allow(unused_variables)]
#[inline(never)]
fn compare_float(
    a_prefix: Prefix,
    a: &NumericValue,
    b_prefix: Prefix,
    b: &NumericValue,
) -> Option<Ordering> {
    #[cfg(feature = "f64")]
    if matches!(a, NumericValue::F64(_)) || matches!(b, NumericValue::F64(_)) {
        let (x, y) = (a.to_f64(), b.to_f64());
        if a_prefix == b_prefix {
            return x.partial_cmp(&y);
        }
        return a_prefix.ratio_to(b_prefix)?.apply_f64(x).partial_cmp(&y);
    }
    #[cfg(feature = "f32")]
    match (*a, *b) {
        (NumericValue::F32(x), NumericValue::F32(y)) => return cmp_f32(a_prefix, x, b_prefix, y),
        (NumericValue::F32(x), b) => {
            return cmp_f32_integer(a_prefix, x, b_prefix, b.to_integer()?);
        }
        (a, NumericValue::F32(y)) => {
            return cmp_f32_integer(b_prefix, y, a_prefix, a.to_integer()?).map(Ordering::reverse);
        }
        _ => {}
    }
    None
}

/// Compares `value` in units of `prefix` with a bound's `limit`, as [compare] does; `None` also for a limit whose
/// exponent no prefix has (beyond ±63).
#[inline]
pub(crate) fn cmp_limit(prefix: Prefix, value: &NumericValue, limit: &Limit) -> Option<Ordering> {
    let limit_prefix = Prefix::from_pow10(limit.exp)?;
    if let Some(x) = value.to_integer() {
        return cmp_integer(prefix, x, limit_prefix, limit.value);
    }
    cmp_limit_float(prefix, value, limit_prefix, limit.value)
}

#[allow(unused_variables)]
#[inline(never)]
fn cmp_limit_float(
    prefix: Prefix,
    value: &NumericValue,
    limit_prefix: Prefix,
    limit: Integer,
) -> Option<Ordering> {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => cmp_f32_integer(prefix, x, limit_prefix, limit),
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => {
            let m = limit.magnitude() as f64;
            let l = if limit.is_negative() { -m } else { m };
            prefix.ratio_to(limit_prefix)?.apply_f64(x).partial_cmp(&l)
        }
        _ => None,
    }
}

/// Common methods of quantities without a prefix (`DeciBel`).
macro_rules! unprefixed_quantity_ops {
    ($($feature:literal $ty:ident),* $(,)?) => {
        $(
            #[cfg(feature = $feature)]
            impl crate::$ty {
                /// A quantity of `value`, any number type that is enabled.
                pub fn new(value: impl Into<NumericValue>) -> Self {
                    crate::$ty { value: value.into() }
                }

                /// The value as `f64`.
                pub fn to_f64(&self) -> f64 {
                    self.value.to_f64()
                }

                /// The value as `f32`.
                pub fn to_f32(&self) -> f32 {
                    self.value.as_f32()
                }
            }

            #[cfg(feature = $feature)]
            impl PartialEq for crate::$ty {
                fn eq(&self, other: &Self) -> bool {
                    self.partial_cmp(other) == Some(Ordering::Equal)
                }
            }

            /// Compares the values across number types: exactly when both are integers, and an `f32` against an
            /// integer.
            #[cfg(feature = $feature)]
            impl PartialOrd for crate::$ty {
                fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                    compare(Prefix::Unit, &self.value, Prefix::Unit, &other.value)
                }
            }
        )*
    };
}

unprefixed_quantity_ops!("logarithmic" DeciBel, "logarithmic" DeciBelmW);
