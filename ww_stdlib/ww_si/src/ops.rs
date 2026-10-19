//! Number helpers behind quantities: powers of ten, exact comparison across prefixes.

use crate::*;
use core::cmp::Ordering;

const POW10_F64: [f64; 31] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28, 1e29, 1e30,
];

/// `10^exp` in `const` code (for [Scale::Pow10](crate::Scale::Pow10)).
pub(crate) const fn pow10_f64(exp: i8) -> f64 {
    let mut x = 1.0;
    let mut i = 0;
    while i < exp.unsigned_abs() {
        x *= 10.0;
        i += 1;
    }
    if exp < 0 { 1.0 / x } else { x }
}

/// `x^n` in `const` code (for [UnitExpr::Pow](crate::UnitExpr::Pow)).
pub(crate) const fn powi_f64(x: f64, n: i8) -> f64 {
    let mut y = 1.0;
    let mut i = 0;
    while i < n.unsigned_abs() {
        y *= x;
        i += 1;
    }
    if n < 0 { 1.0 / y } else { y }
}

/// `x * 10^exp`, for the powers prefixes have (-60..=60 between two of them).
pub(crate) fn scale_f64(x: f64, exp: i8) -> f64 {
    let mut x = x;
    let mut exp = exp as i32;
    while exp > 30 {
        x *= 1e30;
        exp -= 30;
    }
    while exp < -30 {
        x /= 1e30;
        exp += 30;
    }
    if exp >= 0 {
        x * POW10_F64[exp as usize]
    } else {
        x / POW10_F64[(-exp) as usize]
    }
}

/// Powers of ten that `f32` holds exactly (up to 10^10).
const POW10_F32: [f32; 11] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10];

/// `x * 10^exp` in `f32`, without `powi` (not in `core`): one multiplication or division by an exact power of ten for
/// the usual prefixes, so 2 m· is 0.002 rounded once, not three times.
pub(crate) fn scale_f32(x: f32, exp: i8) -> f32 {
    let mut x = x;
    let mut exp = exp as i32;
    while exp > 10 {
        x *= 1e10;
        exp -= 10;
    }
    while exp < -10 {
        x /= 1e10;
        exp += 10;
    }
    if exp >= 0 {
        x * POW10_F32[exp as usize]
    } else {
        x / POW10_F32[(-exp) as usize]
    }
}

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

/// Compares `a * 10^a_exp` with `b * 10^b_exp`: exactly for integers, in no wider type than the enabled number types
/// need (`u32` with types up to 32 bits); through `f64` when one is a float.
pub(crate) fn compare(
    a_exp: i8,
    a: &NumericValue,
    b_exp: i8,
    b: &NumericValue,
) -> Option<Ordering> {
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
pub(crate) fn to_i128_scaled(value: &NumericValue, exp: i8) -> Option<i128> {
    if let Some(x) = value.to_i128() {
        return scale_i128(x, exp);
    }
    if !value.is_float() {
        return None;
    }
    let x = scale_f64(value.to_f64(), exp);
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

/// `value` in units of `10^exp` as `i32`: exact for integers (truncated toward zero for a negative `exp`), rounded for
/// floats; `None` when it doesn't fit, for `NaN` and `UN` / `IN`.
pub(crate) fn to_i32_scaled(value: &NumericValue, exp: i8) -> Option<i32> {
    match value.to_integer() {
        Some(x) => x.checked_scale_pow10(exp)?.to_i32(),
        None => rounded_float(value, exp)?.to_i32(),
    }
}

/// `value` in units of `10^exp` as `u32`, like [to_i32_scaled].
pub(crate) fn to_u32_scaled(value: &NumericValue, exp: i8) -> Option<u32> {
    match value.to_integer() {
        Some(x) => x.checked_scale_pow10(exp)?.to_u32(),
        None => rounded_float(value, exp)?.to_u32(),
    }
}

/// `value` in units of `10^exp` modulo `2^32` (two's complement below zero); floats rounded and from -2^31 up to
/// below 2^32 only.
pub(crate) fn to_u32_wrapping_scaled(value: &NumericValue, exp: i8) -> Option<u32> {
    match value.to_integer() {
        Some(x) => Some(x.to_u32_wrapping_pow10(exp)),
        None => {
            let r = rounded_float(value, exp)?;
            r.to_u32().or_else(|| r.to_i32().map(|x| x as u32))
        }
    }
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
                    ops::compare(0, &self.value, 0, &other.value)
                }
            }
        )*
    };
}

unprefixed_quantity_ops!("logarithmic" DeciBel, "logarithmic" DeciBelmW);
