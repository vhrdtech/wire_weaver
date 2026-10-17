//! Unit algebra: `*` and `/` between quantities whose product is in the table below, `+` and `-` within one unit,
//! scaling by a [Ratio](crate::units::Ratio) or an `f32`.
//!
//! The units are checked by the compiler: `Volt * Ampere` is a `Watt`, `Volt * AngularVelocity` does not build (no
//! `UnitMul` impl), nor does `Volt + Ampere`. Each table line is checked against the SI exponents of its units at
//! compile time, so the table can't be wrong.
//!
//! The numbers are computed in `f32` (in `f64` when an operand is stored as `F64`) through each unit's
//! [Scale](crate::Scale) (`Gram * Acceleration` gives newtons), and the result is stored as `F32` / `F64` without a
//! prefix; the operators need the `f32` feature. Putting it back into the representation an API documents is [Quantity::quantize].

#[allow(unused_imports)] // with no quantity group enabled
use crate::unit::{Unit, UnitDiv, UnitMul};
#[allow(unused_imports)]
use crate::units;
#[cfg(feature = "f32")]
use crate::{Prefix, Quantity};
#[cfg(feature = "f32")]
use core::ops::{Add, Div, Mul, Sub};
#[cfg(feature = "f32")]
use ww_numeric::NumericValue;

/// `a · b · k` in `f32`, or in `f64` when one of them is stored as `F64`.
#[cfg(feature = "f32")]
fn combine<A: Unit, B: Unit, O: Unit>(
    a: &Quantity<A>,
    b: &Quantity<B>,
    k: f64,
    f32_op: fn(f32, f32) -> f32,
    f64_op: fn(f64, f64) -> f64,
) -> Quantity<O> {
    #[cfg(feature = "f64")]
    if matches!(a.value, NumericValue::F64(_)) || matches!(b.value, NumericValue::F64(_)) {
        let x = f64_op(a.to_f64(), b.to_f64()) * k;
        return Quantity::from_parts(Prefix::Unit, NumericValue::F64(x));
    }
    let _ = f64_op;
    let x = f32_op(a.to_f32(), b.to_f32()) * k as f32;
    Quantity::from_parts(Prefix::Unit, NumericValue::F32(x))
}

/// `Volt * Ampere = Watt`, for the pairs in the product table and any unit times a `Ratio`.
#[cfg(feature = "f32")]
impl<A: UnitMul<B>, B: Unit> Mul<Quantity<B>> for Quantity<A> {
    type Output = Quantity<A::Output>;

    fn mul(self, rhs: Quantity<B>) -> Self::Output {
        let k = const { A::EXPR.factor() * B::EXPR.factor() / <A::Output as Unit>::EXPR.factor() };
        combine(&self, &rhs, k, |a, b| a * b, |a, b| a * b)
    }
}

/// `Watt / Volt = Ampere`, for the pairs in the product table and any unit divided by a `Ratio`.
#[cfg(feature = "f32")]
impl<A: UnitDiv<B>, B: Unit> Div<Quantity<B>> for Quantity<A> {
    type Output = Quantity<A::Output>;

    fn div(self, rhs: Quantity<B>) -> Self::Output {
        let k = const { A::EXPR.factor() / B::EXPR.factor() / <A::Output as Unit>::EXPR.factor() };
        combine(&self, &rhs, k, |a, b| a / b, |a, b| a / b)
    }
}

/// Sum of two values of the same unit, in the unit itself.
#[cfg(feature = "f32")]
impl<U: Unit> Add for Quantity<U> {
    type Output = Quantity<U>;

    fn add(self, rhs: Self) -> Self::Output {
        combine(&self, &rhs, 1.0, |a, b| a + b, |a, b| a + b)
    }
}

/// Difference of two values of the same unit, in the unit itself.
#[cfg(feature = "f32")]
impl<U: Unit> Sub for Quantity<U> {
    type Output = Quantity<U>;

    fn sub(self, rhs: Self) -> Self::Output {
        combine(&self, &rhs, 1.0, |a, b| a - b, |a, b| a - b)
    }
}

/// The value scaled by a plain number, in the unit itself.
#[cfg(feature = "f32")]
impl<U: Unit> Mul<f32> for Quantity<U> {
    type Output = Quantity<U>;

    fn mul(self, rhs: f32) -> Self::Output {
        #[cfg(feature = "f64")]
        if let NumericValue::F64(_) = self.value {
            return Quantity::from_parts(
                Prefix::Unit,
                NumericValue::F64(self.to_f64() * rhs as f64),
            );
        }
        Quantity::from_parts(Prefix::Unit, NumericValue::F32(self.to_f32() * rhs))
    }
}

/// The value divided by a plain number, in the unit itself.
#[cfg(feature = "f32")]
impl<U: Unit> Div<f32> for Quantity<U> {
    type Output = Quantity<U>;

    fn div(self, rhs: f32) -> Self::Output {
        #[cfg(feature = "f64")]
        if let NumericValue::F64(_) = self.value {
            return Quantity::from_parts(
                Prefix::Unit,
                NumericValue::F64(self.to_f64() / rhs as f64),
            );
        }
        Quantity::from_parts(Prefix::Unit, NumericValue::F32(self.to_f32() / rhs))
    }
}

/// A ratio keeps the unit: 48 V · 0.5 duty = 24 V.
#[cfg(feature = "dimensionless")]
impl<U: Unit> UnitMul<units::Ratio> for U {
    type Output = U;
}

/// A ratio keeps the unit: 24 V / 0.5 = 48 V.
#[cfg(feature = "dimensionless")]
impl<U: Unit> UnitDiv<units::Ratio> for U {
    type Output = U;
}

/// `A * B = C;` gives `A · B`, `B · A`, `C / A` and `C / B`, and checks at compile time that the SI exponents of `A`
/// and `B` add up to those of `C` and that none of them has an offset scale (°C).
macro_rules! products {
    ($($(#[$meta:meta])* $a:ident * $b:ident = $o:ident;)*) => {
        $(
            $(#[$meta])*
            impl UnitMul<units::$b> for units::$a {
                type Output = units::$o;
            }
            $(#[$meta])*
            impl UnitMul<units::$a> for units::$b {
                type Output = units::$o;
            }
            $(#[$meta])*
            impl UnitDiv<units::$a> for units::$o {
                type Output = units::$b;
            }
            $(#[$meta])*
            impl UnitDiv<units::$b> for units::$o {
                type Output = units::$a;
            }
            $(#[$meta])*
            const _: () = {
                assert!(
                    <units::$a as Unit>::DIM.mul(<units::$b as Unit>::DIM).same(<units::$o as Unit>::DIM),
                    concat!(stringify!($a * $b = $o), ": SI exponents differ")
                );
                assert!(
                    !<units::$a as Unit>::EXPR.has_offset()
                        && !<units::$b as Unit>::EXPR.has_offset()
                        && !<units::$o as Unit>::EXPR.has_offset(),
                    concat!(stringify!($a * $b = $o), ": a unit with an offset scale can't be in a product")
                );
            };
        )*
    };
}

products! {
    #[cfg(feature = "electrical")]
    Volt * Ampere = Watt;
    #[cfg(feature = "electrical")]
    Ampere * Ohm = Volt;
    #[cfg(feature = "electrical")]
    Henry * Ampere = Weber;
    #[cfg(feature = "electrical")]
    Farad * Volt = Coulomb;
    #[cfg(all(feature = "electrical", feature = "time"))]
    Ampere * Second = Coulomb;
    #[cfg(all(feature = "electrical", feature = "time"))]
    Volt * Second = Weber;
    #[cfg(all(feature = "electrical", feature = "time", feature = "mechanical"))]
    Watt * Second = Joule;
    #[cfg(all(feature = "electrical", feature = "mechanical", feature = "angle"))]
    Torque * AngularVelocity = Watt;
    #[cfg(all(feature = "angle", feature = "time"))]
    AngularVelocity * Second = Radian;
    #[cfg(all(feature = "mechanical", feature = "time"))]
    Speed * Second = Meter;
    #[cfg(all(feature = "mechanical", feature = "time"))]
    Acceleration * Second = Speed;
    #[cfg(feature = "mechanical")]
    Gram * Acceleration = Newton;
    #[cfg(feature = "mechanical")]
    Newton * Meter = Joule;
    #[cfg(all(feature = "time", feature = "dimensionless"))]
    Hertz * Second = Ratio;
    #[cfg(feature = "time")]
    HertzPerSecond * Second = Hertz;
}
