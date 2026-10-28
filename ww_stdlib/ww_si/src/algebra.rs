//! Unit algebra: `*` and `/` between quantities whose product is in `exact_units`' table ([UnitMul], [UnitDiv]),
//! `+` and `-` within one unit, scaling by a [Ratio](crate::units::Ratio) or an `f32`.
//!
//! The units are checked by the compiler: `Volt * Ampere` is a `Watt`, `Volt * AngularVelocity` does not build (no
//! `UnitMul` impl), nor does `Volt + Ampere`.
//!
//! The numbers are computed in `f32` (in `f64` when an operand is stored as `F64`) through each unit's
//! [Scale](crate::Scale) (`Gram * Acceleration` gives newtons), and the result is stored as `F32` / `F64` without a
//! prefix; the operators need the `f32` feature. Putting it back into the representation an API documents is [Quantity::quantize].

#[cfg(feature = "f32")]
use crate::{Prefix, Quantity};
#[cfg(feature = "f32")]
use core::ops::{Add, Div, Mul, Sub};
#[allow(unused_imports)] // with no quantity group enabled
use exact_units::{Unit, UnitDiv, UnitMul};
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
