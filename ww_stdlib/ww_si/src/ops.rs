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

/// Compares `a * 10^a_exp` with `b * 10^b_exp`: exactly for integers, through `f64` otherwise.
pub(crate) fn compare(
    a_exp: i8,
    a: &NumericValue,
    b_exp: i8,
    b: &NumericValue,
) -> Option<Ordering> {
    if let (Some(a), Some(b)) = (a.to_i128(), b.to_i128()) {
        // scale the one with the bigger prefix down to the smaller prefix, nothing is lost
        return Some(if a_exp >= b_exp {
            match scale_i128(a, a_exp - b_exp) {
                Some(a) => a.cmp(&b),
                // |a * 10^k| is beyond i128, so beyond any b
                None => a.cmp(&0),
            }
        } else {
            match scale_i128(b, b_exp - a_exp) {
                Some(b) => a.cmp(&b),
                None => 0.cmp(&b),
            }
        });
    }
    scale_f64(a.to_f64(), a_exp).partial_cmp(&scale_f64(b.to_f64(), b_exp))
}

/// `value` in units of `10^exp`, rounded to the nearest integer for floats; `None` for `NaN`, out of range values and
/// `UN` / `IN`.
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

            /// Compares the values across number types: exactly when both are integers, through `f64` otherwise.
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
