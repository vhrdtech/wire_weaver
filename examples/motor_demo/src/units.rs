//! The demo's `f32` values against the `ww_si` quantities of its API: VESC's units (erpm, Ah, Wh) to SI and back,
//! a `f32` as a fixed point number of a prefix, and a value as the host shows it ([Human]).

use core::f32::consts::TAU;
use ww_si::Prefix;
use ww_si::ww_numeric::NumericValue;

pub const SECONDS_PER_HOUR: f32 = 3600.0;

/// Revolutions per minute (electrical or mechanical) as rad/s.
pub fn rad_per_s(rpm: f32) -> f32 {
    rpm * TAU / 60.0
}

/// rad/s as revolutions per minute.
pub fn rpm(rad_per_s: f32) -> f32 {
    rad_per_s * 60.0 / TAU
}

/// `x * 10^-exp` rounded, in `f64` so an `i32` / `u32` of it is exact.
fn scaled(x: f32, prefix: Prefix) -> f64 {
    let exp: i8 = prefix.into();
    (x as f64 * 10f64.powi(-(exp as i32))).round()
}

/// An `f32` in the base unit as an `I32` count of a prefix.
pub trait I32In {
    /// Rounded, saturating at the ends of `i32`, `NaN` is 0.
    fn i32_in(self, prefix: Prefix) -> NumericValue;
}

impl I32In for f32 {
    fn i32_in(self, prefix: Prefix) -> NumericValue {
        NumericValue::I32(scaled(self, prefix) as i32)
    }
}

/// An `f32` in the base unit as a `U32` count of a prefix.
pub trait U32In {
    /// Rounded, negative values are 0, saturating at `u32::MAX`.
    fn u32_in(self, prefix: Prefix) -> NumericValue;
}

impl U32In for f32 {
    fn u32_in(self, prefix: Prefix) -> NumericValue {
        NumericValue::U32(scaled(self, prefix) as u32)
    }
}

/// A value as the host shows it, in `unit`: quantities in their base unit, or in rpm (`erpm`, `rpm`), Ah or Wh where
/// VESC Tool shows those.
pub trait Human {
    fn human(&self, unit: &str) -> String;
}

macro_rules! human_quantity {
    ($($ty:ident),*) => {
        $(
            impl Human for ww_si::$ty {
                fn human(&self, unit: &str) -> String {
                    let x = self.to_f64();
                    match unit {
                        // a tenth of an rpm, the speed's own resolution is ~0.01 erpm
                        "erpm" | "rpm" => format!("{:.1}", x * 60.0 / core::f64::consts::TAU),
                        "Ah" | "Wh" => format!("{}", (x / SECONDS_PER_HOUR as f64) as f32),
                        // shortest form that reads back the same, without f64's tail of digits
                        _ => format!("{}", x as f32),
                    }
                }
            }
        )*
    };
}

human_quantity!(
    Ampere,
    AngularVelocity,
    Coulomb,
    Degree,
    Joule,
    Radian,
    Ratio,
    Second,
    Torque,
    Volt,
    Watt,
    Weber
);

macro_rules! human_debug {
    ($($ty:ty),*) => {
        $(
            impl Human for $ty {
                fn human(&self, _unit: &str) -> String {
                    format!("{self:?}")
                }
            }
        )*
    };
}

human_debug!(
    i32,
    u32,
    motor_demo_api::ControlMode,
    motor_demo_api::SensorMode,
    motor_demo_api::Fault
);

#[cfg(test)]
mod tests {
    use super::*;
    use ww_si::{AngularVelocity, Coulomb};

    #[test]
    fn fixed_point() {
        assert_eq!(1.2345f32.i32_in(Prefix::Milli), NumericValue::I32(1235));
        assert_eq!((-1.4e-6f32).i32_in(Prefix::Micro), NumericValue::I32(-1));
        assert_eq!(1e12f32.i32_in(Prefix::Milli), NumericValue::I32(i32::MAX));
        assert_eq!((-3.0f32).u32_in(Prefix::Unit), NumericValue::U32(0));
        let c = Coulomb::new(Prefix::Unit, (2.0 * SECONDS_PER_HOUR).u32_in(Prefix::Unit));
        assert_eq!(c.to_f32() / SECONDS_PER_HOUR, 2.0);
    }

    #[test]
    fn erpm() {
        // 12 000 erpm is 1 257 rad/s, the API's example
        let w = AngularVelocity::new(Prefix::Milli, rad_per_s(12_000.0).i32_in(Prefix::Milli));
        assert_eq!(w.value, NumericValue::I32(1_256_637));
        assert!((rpm(w.to_f32()) - 12_000.0).abs() < 0.01);
        assert_eq!(w.human("erpm"), "12000.0");
        assert_eq!(Coulomb::new(Prefix::Unit, 7200u32).human("Ah"), "2");
    }
}
