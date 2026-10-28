//! Number helpers behind quantities: powers of ten, exact comparison across prefixes.
//!
//! Between two SI prefixes a value is rescaled by a power of ten, the fast path. A binary or `Exact` prefix takes the
//! slow path: the ratio of the two scales as a [Magnitude], applied exactly to integers (refused for an irrational
//! ratio) and through `f64` to floats. The slow path lives in functions of its own, so the 104-byte `Magnitude`s
//! stay out of the fast path's stack frame.

use crate::bounds::Limit;
use crate::*;
use core::cmp::Ordering;
use exact_units::Magnitude;
use ww_numeric::Integer;

pub(crate) use exact_units::num::{scale_f32, scale_f64};

/// `x * 10^exp` exactly, `None` on overflow; a negative `exp` truncates toward zero.
#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
pub(crate) fn scale_i128(x: i128, exp: i8) -> Option<i128> {
    if x == 0 {
        Some(0)
    } else if exp >= 0 {
        10i128
            .checked_pow(exp as u32)
            .and_then(|p| x.checked_mul(p))
    } else {
        match 10i128.checked_pow(exp.unsigned_abs() as u32) {
            Some(p) => Some(x / p),
            None => Some(0),
        }
    }
}

/// The power of ten that takes a value in units of `from` to units of `to`, when both are SI prefixes.
#[inline]
pub(crate) fn pow10_between(from: Prefix, to: Prefix) -> Option<i8> {
    from.si_pow10()?.checked_sub(to.si_pow10()?)
}

/// `from`'s scale over `to`'s: what a value in units of `from` is multiplied by to be in units of `to`. `None` out of
/// `Magnitude`'s range, and for a scale from a newer version (`Prefix::Unknown`).
#[inline(never)]
pub(crate) fn ratio_between(from: Prefix, to: Prefix) -> Option<Magnitude> {
    from.magnitude()?.checked_div(to.magnitude()?).ok()
}

/// `x · num / den` for the rational `ratio`, truncated toward zero; `None` for an irrational ratio or on overflow.
fn apply_truncating(ratio: &Magnitude, x: i128) -> Option<i128> {
    let (num, den) = ratio.to_ratio()?;
    let (num, den) = (i128::try_from(num).ok()?, i128::try_from(den).ok()?);
    x.checked_mul(num).map(|x| x / den)
}

/// A value rescaled on the slow path: integers exactly (truncated toward zero), floats through `f64`, not rounded.
pub(crate) enum Rescaled {
    Int(i128),
    Float(f64),
}

/// `value` in units of `from` rescaled to units of `to` on the slow path (see the module docs). `None` for `UN` / `IN`,
/// an integer under an irrational ratio, and on overflow.
#[inline(never)]
pub(crate) fn rescale_slow(value: &NumericValue, from: Prefix, to: Prefix) -> Option<Rescaled> {
    let ratio = ratio_between(from, to)?;
    if let Some(x) = value.to_i128() {
        return apply_truncating(&ratio, x).map(Rescaled::Int);
    }
    value
        .is_float()
        .then(|| Rescaled::Float(ratio.apply_f64(value.to_f64())))
}

/// `x` in units of `from` in units of `to`, as `f64`.
pub(crate) fn scale_f64_between(x: f64, from: Prefix, to: Prefix) -> f64 {
    match pow10_between(from, to) {
        Some(exp) => scale_f64(x, exp),
        None => scale_f64_slow(x, from, to),
    }
}

#[inline(never)]
fn scale_f64_slow(x: f64, from: Prefix, to: Prefix) -> f64 {
    match ratio_between(from, to) {
        Some(ratio) => ratio.apply_f64(x),
        // the ratio is out of `Magnitude`'s range, the scales themselves aren't
        None => from.scale_f64(x) / to.scale_f64(1.0),
    }
}

/// `x` in units of `from` in units of `to`, as `f32`: `f32` arithmetic between SI prefixes, `f64` otherwise.
pub(crate) fn scale_f32_between(x: f32, from: Prefix, to: Prefix) -> f32 {
    match pow10_between(from, to) {
        Some(exp) => scale_f32(x, exp),
        None => scale_f64_slow(x as f64, from, to) as f32,
    }
}

/// `x` (an integer in units of `from`) in units of `to` if that is a whole number that fits: scaling to a finer
/// prefix that the coarser is a whole multiple of always is, if it fits.
pub(crate) fn exact_between(x: Integer, from: Prefix, to: Prefix) -> Option<Integer> {
    match pow10_between(from, to) {
        Some(exp) => x.exact_scale_pow10(exp),
        None => exact_slow(x, from, to),
    }
}

#[inline(never)]
fn exact_slow(x: Integer, from: Prefix, to: Prefix) -> Option<Integer> {
    let m = i128::try_from(x.magnitude()).ok()?;
    let x = if x.is_negative() { -m } else { m };
    let y = ratio_between(from, to)?.apply_int(x)?;
    // A no-op only when ww_numeric's `u128` makes `Magnitude` a u128.
    #[allow(clippy::useless_conversion)]
    let magnitude = y.unsigned_abs().try_into().ok()?;
    Some(Integer::new(y < 0, magnitude))
}

/// The finer of two prefixes (the smaller scale), `a` when they are equal.
pub(crate) fn finer(a: Prefix, b: Prefix) -> Prefix {
    let a_finer = match (a.si_pow10(), b.si_pow10()) {
        (Some(a), Some(b)) => a <= b,
        // NaN for `Unknown`: `b`, and the conversion to it is refused
        _ => a.scale_f64(1.0) <= b.scale_f64(1.0),
    };
    if a_finer { a } else { b }
}

/// Compares `a` in units of `a_prefix` with `b` in units of `b_prefix`: exactly for integers, in no wider type than
/// the enabled number types need between SI prefixes (`u32` with types up to 32 bits), in `i128` for a rational ratio
/// of other scales; through `f64` when one is a float or the ratio is irrational.
pub(crate) fn compare(
    a_prefix: Prefix,
    a: &NumericValue,
    b_prefix: Prefix,
    b: &NumericValue,
) -> Option<Ordering> {
    match (a_prefix.si_pow10(), b_prefix.si_pow10()) {
        (Some(a_exp), Some(b_exp)) => compare_pow10(a_exp, a, b_exp, b),
        _ => compare_slow(a_prefix, a, b_prefix, b),
    }
}

#[inline(never)]
fn compare_slow(
    a_prefix: Prefix,
    a: &NumericValue,
    b_prefix: Prefix,
    b: &NumericValue,
) -> Option<Ordering> {
    // one scale, also one from a newer version (`Prefix::Unknown`): the numbers alone
    if a_prefix == b_prefix {
        return compare_pow10(0, a, 0, b);
    }
    if let (Some(x), Some(y)) = (a.to_i128(), b.to_i128()) {
        // a · ma <=> b · mb is a · num <=> b · den for ma / mb = num / den
        let exact = ratio_between(a_prefix, b_prefix)
            .and_then(|r| r.to_ratio())
            .and_then(|(num, den)| {
                let (num, den) = (i128::try_from(num).ok()?, i128::try_from(den).ok()?);
                Some(x.checked_mul(num)?.cmp(&y.checked_mul(den)?))
            });
        if exact.is_some() {
            return exact;
        }
    } else if !(a.is_float() || b.is_float()) {
        // UN / IN
        return None;
    }
    a_prefix
        .scale_f64(a.to_f64())
        .partial_cmp(&b_prefix.scale_f64(b.to_f64()))
}

/// Compares `value` in units of `prefix` with a bound's `limit`: exactly for integers (the SI fast path in
/// [Integer::cmp_pow10]), through `f64` for floats and an irrational scale; `None` for `NaN` and `UN` / `IN`.
#[inline]
pub(crate) fn cmp_limit(prefix: Prefix, value: &NumericValue, limit: &Limit) -> Option<Ordering> {
    if let Some(x) = value.to_integer() {
        return match prefix.si_pow10() {
            Some(exp) => Some(x.cmp_pow10(exp, limit.value, limit.exp)),
            None => cmp_limit_slow(prefix, x, limit),
        };
    }
    if value.is_float() {
        return prefix
            .scale_f64(value.to_f64())
            .partial_cmp(&limit_f64(limit));
    }
    None
}

#[inline(never)]
fn cmp_limit_slow(prefix: Prefix, x: Integer, limit: &Limit) -> Option<Ordering> {
    // x · m <=> l · 10^exp is x · num <=> l · den for m / 10^exp = num / den
    let exact = Magnitude::checked_pow10(limit.exp)
        .and_then(|l| prefix.magnitude()?.checked_div(l).ok())
        .and_then(|r| r.to_ratio())
        .and_then(|(num, den)| {
            let (num, den) = (i128::try_from(num).ok()?, i128::try_from(den).ok()?);
            let (x, l) = (integer_i128(x)?, integer_i128(limit.value)?);
            Some(x.checked_mul(num)?.cmp(&l.checked_mul(den)?))
        });
    if exact.is_some() {
        return exact;
    }
    prefix
        .scale_f64(integer_f64(x))
        .partial_cmp(&limit_f64(limit))
}

fn integer_i128(x: Integer) -> Option<i128> {
    let m = i128::try_from(x.magnitude()).ok()?;
    Some(if x.is_negative() { -m } else { m })
}

fn integer_f64(x: Integer) -> f64 {
    let m = x.magnitude() as f64;
    if x.is_negative() { -m } else { m }
}

fn limit_f64(limit: &Limit) -> f64 {
    scale_f64(integer_f64(limit.value), limit.exp)
}

/// `x` in units of `from` in units of `to`, exactly, truncated toward zero; `None` on overflow and for an
/// irrational ratio.
#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
pub(crate) fn scale_i128_between(x: i128, from: Prefix, to: Prefix) -> Option<i128> {
    match pow10_between(from, to) {
        Some(exp) => scale_i128(x, exp),
        None => scale_i128_slow(x, from, to),
    }
}

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
#[inline(never)]
fn scale_i128_slow(x: i128, from: Prefix, to: Prefix) -> Option<i128> {
    apply_truncating(&ratio_between(from, to)?, x)
}

/// Compares `a * 10^a_exp` with `b * 10^b_exp`: exactly for integers, in no wider type than the enabled number types
/// need (`u32` with types up to 32 bits); through `f64` when one is a float.
fn compare_pow10(a_exp: i8, a: &NumericValue, b_exp: i8, b: &NumericValue) -> Option<Ordering> {
    if let (Some(a), Some(b)) = (a.to_integer(), b.to_integer()) {
        return Some(a.cmp_pow10(a_exp, b, b_exp));
    }
    // `is_float` is `false` without float types: no float code then
    if a.is_float() || b.is_float() {
        return scale_f64(a.to_f64(), a_exp).partial_cmp(&scale_f64(b.to_f64(), b_exp));
    }
    // UN / IN
    None
}

/// `value` in units of `10^exp`, rounded to the nearest integer for floats; `None` for `NaN`, out of range values and
/// `UN` / `IN`.
#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
pub(crate) fn to_i128_scaled(value: &NumericValue, from: Prefix, to: Prefix) -> Option<i128> {
    let Some(exp) = pow10_between(from, to) else {
        return match rescale_slow(value, from, to)? {
            Rescaled::Int(x) => Some(x),
            Rescaled::Float(x) => round_f64_to_i128(x),
        };
    };
    if let Some(x) = value.to_i128() {
        return scale_i128(x, exp);
    }
    if !value.is_float() {
        return None;
    }
    round_f64_to_i128(scale_f64(value.to_f64(), exp))
}

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
fn round_f64_to_i128(x: f64) -> Option<i128> {
    // `as` saturates, values beyond i128 are refused instead
    if x.is_nan() || !(-1.7e38..=1.7e38).contains(&x) {
        return None;
    }
    let rounded = if x >= 0.0 { x + 0.5 } else { x - 0.5 };
    Some(rounded as i128)
}

/// A float scaled by `10^exp` and rounded to the nearest integer (halves away from zero), as `f32` for an `F32`, no
/// `f64` code on a device; `None` for an integer.
#[derive(Copy, Clone)]
pub(crate) enum Rounded {
    #[cfg(feature = "f32")]
    F32(f32),
    F64(f64),
}

pub(crate) fn rounded_float(value: &NumericValue, exp: i8) -> Option<Rounded> {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => {
            let x = scale_f32(x, exp);
            Some(Rounded::F32(if x >= 0.0 { x + 0.5 } else { x - 0.5 }))
        }
        v if v.is_float() => {
            let x = scale_f64(v.to_f64(), exp);
            Some(Rounded::F64(if x >= 0.0 { x + 0.5 } else { x - 0.5 }))
        }
        _ => None,
    }
}

impl Rounded {
    // `as` truncates toward zero (the halves are added already) and saturates: the range is checked first, `NaN`
    // fails every comparison. No f32 lies between -2^31 - 1 and -2^31, so `>= -2^31` is the same bound there.

    pub(crate) fn to_i32(self) -> Option<i32> {
        match self {
            #[cfg(feature = "f32")]
            Rounded::F32(r) => (-2_147_483_648.0..2_147_483_648.0)
                .contains(&r)
                .then_some(r as i32),
            Rounded::F64(r) => (r > -2_147_483_649.0 && r < 2_147_483_648.0).then_some(r as i32),
        }
    }

    pub(crate) fn to_u32(self) -> Option<u32> {
        match self {
            #[cfg(feature = "f32")]
            Rounded::F32(r) => (r > -1.0 && r < 4_294_967_296.0).then_some(r as u32),
            Rounded::F64(r) => (r > -1.0 && r < 4_294_967_296.0).then_some(r as u32),
        }
    }
}

fn round_f64(x: f64) -> Rounded {
    Rounded::F64(if x >= 0.0 { x + 0.5 } else { x - 0.5 })
}

/// `value` in units of `from` as `i32` in units of `to`: exact for integers (truncated toward zero), rounded for
/// floats; `None` when it doesn't fit, for `NaN` and `UN` / `IN`.
pub(crate) fn to_i32_scaled(value: &NumericValue, from: Prefix, to: Prefix) -> Option<i32> {
    let Some(exp) = pow10_between(from, to) else {
        return match rescale_slow(value, from, to)? {
            Rescaled::Int(x) => i32::try_from(x).ok(),
            Rescaled::Float(x) => round_f64(x).to_i32(),
        };
    };
    match value.to_integer() {
        Some(x) => x.checked_scale_pow10(exp)?.to_i32(),
        None => rounded_float(value, exp)?.to_i32(),
    }
}

/// `value` in units of `from` as `u32` in units of `to`, like [to_i32_scaled].
pub(crate) fn to_u32_scaled(value: &NumericValue, from: Prefix, to: Prefix) -> Option<u32> {
    let Some(exp) = pow10_between(from, to) else {
        return match rescale_slow(value, from, to)? {
            Rescaled::Int(x) => u32::try_from(x).ok(),
            Rescaled::Float(x) => round_f64(x).to_u32(),
        };
    };
    match value.to_integer() {
        Some(x) => x.checked_scale_pow10(exp)?.to_u32(),
        None => rounded_float(value, exp)?.to_u32(),
    }
}

/// `value` in units of `from` in units of `to` modulo `2^32` (two's complement below zero); floats rounded and from
/// -2^31 up to below 2^32 only.
pub(crate) fn to_u32_wrapping_scaled(
    value: &NumericValue,
    from: Prefix,
    to: Prefix,
) -> Option<u32> {
    let r = match pow10_between(from, to) {
        Some(exp) => match value.to_integer() {
            Some(x) => return Some(x.to_u32_wrapping_pow10(exp)),
            None => rounded_float(value, exp)?,
        },
        None => match rescale_slow(value, from, to)? {
            Rescaled::Int(x) => return Some(x as u32),
            Rescaled::Float(x) => round_f64(x),
        },
    };
    r.to_u32().or_else(|| r.to_i32().map(|x| x as u32))
}

/// Common methods of quantities without a prefix (`DeciBel`).
macro_rules! unprefixed_quantity_ops {
    ($($feature:literal $ty:ident),* $(,)?) => {
        $(
            #[cfg(feature = $feature)]
            impl $ty {
                /// A quantity of `value`, any number type that is enabled.
                pub fn new(value: impl Into<NumericValue>) -> Self {
                    $ty { value: value.into() }
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
            impl PartialEq for $ty {
                fn eq(&self, other: &Self) -> bool {
                    self.partial_cmp(other) == Some(Ordering::Equal)
                }
            }

            /// Compares the values across number types: exactly when both are integers, through `f64` when one is a
            /// float.
            #[cfg(feature = $feature)]
            impl PartialOrd for $ty {
                fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                    ops::compare(Prefix::Unit, &self.value, Prefix::Unit, &other.value)
                }
            }
        )*
    };
}

unprefixed_quantity_ops!("logarithmic" DeciBel, "logarithmic" DeciBelmW);
