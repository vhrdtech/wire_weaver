//! The demo's `f32` values against the `ww_si` quantities of its API: VESC's units (erpm, Ah, Wh) to SI and back,
//! and a value as the host shows it ([Human]).

use core::f32::consts::TAU;
use ww_si::{Quantity, Unit};

pub const SECONDS_PER_HOUR: f32 = 3600.0;

/// Revolutions per minute (electrical or mechanical) as rad/s.
pub fn rad_per_s(rpm: f32) -> f32 {
    rpm * TAU / 60.0
}

/// rad/s as revolutions per minute.
pub fn rpm(rad_per_s: f32) -> f32 {
    rad_per_s * 60.0 / TAU
}

/// The same value as the previous run, for change marks of the device at 25 kHz: a quantity by `same_repr`
/// (prefix and number, no cross-prefix comparison), anything else by `==`.
pub trait Same {
    fn same(&self, other: &Self) -> bool;
}

impl<U: Unit> Same for Quantity<U> {
    fn same(&self, other: &Self) -> bool {
        self.same_repr(other)
    }
}

/// A value as the host shows it, in `unit`: quantities in their base unit, or in rpm (`erpm`, `rpm`), Ah or Wh where
/// VESC Tool shows those.
pub trait Human {
    fn human(&self, unit: &str) -> String;
}

impl<U: Unit> Human for Quantity<U> {
    fn human(&self, unit: &str) -> String {
        let x = self.to_f64();
        match unit {
            // a tenth of an rpm, the speed's own resolution is ~0.01 erpm
            "erpm" | "rpm" => format!("{:.1}", x * 60.0 / core::f64::consts::TAU),
            "Ah" | "Wh" => format!("{}", (x / SECONDS_PER_HOUR as f64) as f32),
            // the number in the base unit, shortest form that reads back the same
            _ => format!("{}", x as f32),
        }
    }
}

macro_rules! human_debug {
    ($($ty:ty),*) => {
        $(
            impl Same for $ty {
                fn same(&self, other: &Self) -> bool {
                    self == other
                }
            }

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
    use ww_si::Prefix;
    use ww_si::ww_numeric::{NumericBaseType, NumericValue};
    use ww_si::{AngularVelocity, Coulomb};

    #[test]
    fn erpm() {
        // 12 000 erpm is 1 257 rad/s, the API's example
        let w = AngularVelocity::quantize(rad_per_s(12_000.0), Prefix::Milli, NumericBaseType::I32);
        assert_eq!(w.value, NumericValue::I32(1_256_637));
        assert!((rpm(w.to_f32()) - 12_000.0).abs() < 0.01);
        assert_eq!(w.human("erpm"), "12000.0");
        assert_eq!(Coulomb::new(Prefix::Unit, 7200u32).human("Ah"), "2");
    }
}
