//! Constructors, conversions and comparisons shared by every quantity.

use crate::*;
use core::cmp::Ordering;

const POW10_F64: [f64; 31] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28, 1e29, 1e30,
];

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

/// `x * 10^exp` in `f32`, without `powi` (not in `core`).
#[allow(dead_code)] // with only `logarithmic`, which has no prefix
pub(crate) fn scale_f32(x: f32, exp: i8) -> f32 {
    let mut x = x;
    let mut exp = exp;
    while exp > 0 {
        x *= 10.0;
        exp -= 1;
    }
    while exp < 0 {
        x /= 10.0;
        exp += 1;
    }
    x
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
#[allow(dead_code)] // with only `logarithmic`, which has no prefix
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

/// Common methods of every quantity with a prefix.
macro_rules! quantity_ops {
    ($($feature:literal $ty:ident),* $(,)?) => {
        $(
            #[cfg(feature = $feature)]
            impl $ty {
                /// A quantity of `value` (any number type that is enabled, `u16`, `f32`, `UVlq32`, ..) in units of
                /// `prefix`.
                pub fn new(prefix: Prefix, value: impl Into<NumericValue>) -> Self {
                    $ty { prefix, value: value.into() }
                }

                /// The value in the base unit (no prefix) as `f64`, rounded where `f64` can't hold it exactly.
                pub fn to_f64(&self) -> f64 {
                    ops::scale_f64(self.value.to_f64(), self.prefix.into())
                }

                /// The value in the base unit (no prefix) as `f32`.
                pub fn to_f32(&self) -> f32 {
                    ops::scale_f32(self.value.as_f32(), self.prefix.into())
                }

                /// The value in units of `prefix` as `f64` (`to_f64_in(Prefix::Milli)` of 3.3 V is 3300.0).
                pub fn to_f64_in(&self, prefix: Prefix) -> f64 {
                    let from: i8 = self.prefix.into();
                    let to: i8 = prefix.into();
                    ops::scale_f64(self.value.to_f64(), from - to)
                }

                /// The value in units of `prefix` as an integer: exact for integer storage (truncated toward zero
                /// when `prefix` is bigger than the stored one), rounded for floats. `None` when it doesn't fit
                /// in `i128` or is `NaN`.
                pub fn to_i128_in(&self, prefix: Prefix) -> Option<i128> {
                    let from: i8 = self.prefix.into();
                    let to: i8 = prefix.into();
                    ops::to_i128_scaled(&self.value, from - to)
                }
            }

            /// Equal when the values are, whatever prefix and number type each one is stored in
            /// (`1000 mV == 1 V`).
            #[cfg(feature = $feature)]
            impl PartialEq for $ty {
                fn eq(&self, other: &Self) -> bool {
                    self.partial_cmp(other) == Some(Ordering::Equal)
                }
            }

            /// Compares the values across prefixes and number types: exactly when both are integers, through `f64`
            /// otherwise.
            #[cfg(feature = $feature)]
            impl PartialOrd for $ty {
                fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                    ops::compare(self.prefix.into(), &self.value, other.prefix.into(), &other.value)
                }
            }
        )*
    };
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

/// Named constructors: `$name(value)` is `new(Prefix::$prefix, value)`, `$name_f32(value)` an `f32` in the base unit.
macro_rules! named_constructors {
    ($feature:literal $ty:ident, $unit:literal, $base_f32:ident, $($name:ident $prefix:ident $symbol:literal),* $(,)?) => {
        #[cfg(feature = $feature)]
        impl $ty {
            $(
                #[doc = concat!("A value in ", $symbol, ", any number type that is enabled: `", stringify!($ty), "::",
                    stringify!($name), "(3300u16)`.")]
                #[allow(clippy::self_named_constructors)]
                pub fn $name(value: impl Into<NumericValue>) -> Self {
                    Self::new(Prefix::$prefix, value)
                }
            )*

            #[doc = concat!("A value in ", $unit, " stored as `f32`.")]
            #[cfg(feature = "f32")]
            pub fn $base_f32(value: f32) -> Self {
                Self::new(Prefix::Unit, NumericValue::F32(value))
            }
        }
    };
}

named_constructors!("electrical" Volt, "V", volts_f32,
    volts Unit "V", millivolts Milli "mV", microvolts Micro "µV", kilovolts Kilo "kV");
named_constructors!("electrical" Ampere, "A", amperes_f32,
    amperes Unit "A", milliamperes Milli "mA", microamperes Micro "µA", nanoamperes Nano "nA");
named_constructors!("time" Hertz, "Hz", hertz_f32,
    hertz Unit "Hz", millihertz Milli "mHz", kilohertz Kilo "kHz", megahertz Mega "MHz", gigahertz Giga "GHz");
named_constructors!("time" Second, "s", seconds_f32,
    seconds Unit "s", milliseconds Milli "ms", microseconds Micro "µs", nanoseconds Nano "ns",
    picoseconds Pico "ps");

#[cfg(feature = "time")]
impl Second {
    /// The time in seconds as `f64`, same as [to_f64](Self::to_f64). For a timestamp with fixed-width storage that
    /// is the wrapped value: [Unwrapper] (feature `host`) turns it into the full one first.
    pub fn to_f64_seconds(&self) -> f64 {
        self.to_f64()
    }
}

quantity_ops!(
    "time" Second,
    "time" Hertz,
    "time" HertzPerSecond,
    "electrical" Ampere,
    "electrical" Volt,
    "electrical" Ohm,
    "electrical" Farad,
    "electrical" Siemens,
    "electrical" Henry,
    "electrical" Coulomb,
    "electrical" Watt,
    "electrical" Weber,
    "electrical" Tesla,
    "mechanical" Meter,
    "mechanical" KiloGram,
    "mechanical" Newton,
    "mechanical" Pascal,
    "mechanical" Joule,
    "mechanical" Litre,
    "mechanical" VolumetricFlow,
    "mechanical" Speed,
    "mechanical" Acceleration,
    "mechanical" Jerk,
    "mechanical" Snap,
    "mechanical" Yank,
    "angle" Radian,
    "angle" Degree,
    "angle" Steradian,
    "angle" AngularVelocity,
    "angle" AngularAcceleration,
    "thermal" Kelvin,
    "thermal" DegreeCelsius,
    "chemical" Mole,
    "chemical" Katal,
    "photometric" Candela,
    "photometric" Lumen,
    "photometric" Lux,
    "radiation" Becquerel,
    "radiation" Gray,
    "radiation" Sievert,
    "radiation" ElectronVolt,
);

unprefixed_quantity_ops!("logarithmic" DeciBel, "logarithmic" DeciBelmW);
