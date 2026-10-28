//! A [Fixed] to and from a [Quantity]: the only place the compile-time [Fixed] meets the run-time prefix and
//! [NumericValue] of a [Quantity].

use crate::{Prefix, Quantity};
use core::fmt;
use exact_units::{Bounds, DecimalPrefix, Fixed, FixedScale, Repr, Storage, Unit};
use ww_numeric::NumericValue;

/// A [Fixed]'s scale that no run-time [Prefix] stands for: neither a named prefix nor `Pow10`, `Pow2` or `FullScale`
/// (`Rational<33, 10>` over 4096, an ADC step; a binary angle, π·2⁻¹⁵ rad). Such a value goes into an
/// [AnyQuantity](crate::AnyQuantity) exactly instead, the scale in its unit (`AnyQuantityOwned::from_fixed`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnknownScale;

impl fmt::Display for UnknownScale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a scale without a run-time prefix in this build")
    }
}

impl core::error::Error for UnknownScale {}

impl<U: Unit> Quantity<U> {
    /// The value in the prefix `P` and storage `T` of a [Fixed], if it is exact (as [Fixed::to]) and within `B`.
    pub fn to_fixed<P: FixedScale, T: Storage, B: Bounds<T>>(&self) -> Option<Fixed<U, P, T, B>> {
        Fixed::from_scaled(repr(&self.value)?, self.prefix.magnitude())
    }

    /// The same value as a [Fixed] has it, prefix and number as they are. Only for number types `NumericValue` has
    /// enabled: a `u64` needs `ww_numeric`'s `u64` feature (on with `std`). A binary prefix is converted first
    /// ([Fixed::to] `One`).
    pub fn from_fixed<P: DecimalPrefix, T: Storage + Into<NumericValue>, B: Bounds<T>>(
        f: Fixed<U, P, T, B>,
    ) -> Self {
        Quantity::new(P::PREFIX, f.get())
    }

    /// The same value with any scale, prefix and number as they are: a named prefix where the scale has one (`Milli`,
    /// `Kibi`), else [Prefix::Pow10] (`Pow10<-4>`), [Prefix::Pow2] (`Pow2<-15>`) or [Prefix::FullScale]
    /// (`FullScaleMax<One, 16>`), so the value is exact and converts back with [to_fixed](Self::to_fixed).
    /// [UnknownScale] for any other scale, see there.
    pub fn try_from_fixed<P: FixedScale, T: Storage + Into<NumericValue>, B: Bounds<T>>(
        f: Fixed<U, P, T, B>,
    ) -> Result<Self, UnknownScale> {
        let prefix = match P::RUNTIME {
            Some(prefix) => prefix,
            None => Prefix::from_magnitude(&P::SCALE).ok_or(UnknownScale)?,
        };
        Ok(Quantity::new(prefix, f.get()))
    }
}

/// A number as [Fixed::from_scaled] takes it.
fn repr(value: &NumericValue) -> Option<Repr> {
    Some(if value.is_float() {
        Repr::Float(value.to_f64())
    } else {
        let x = value.to_integer()?;
        let m = i128::try_from(x.magnitude()).ok()?;
        Repr::Int(if x.is_negative() { -m } else { m })
    })
}

#[cfg(feature = "std")]
mod any {
    use super::*;
    use crate::{AnyQuantityOwned, SIExprOwned};
    use exact_units::{Dim, Scale};
    use std::boxed::Box;
    use std::string::ToString;

    impl AnyQuantityOwned {
        /// Any [Fixed] exactly, whatever its scale: as [Quantity::try_from_fixed] where a run-time [Prefix] stands
        /// for it, else with [Prefix::Unit] and the scale in the unit, a dimensionless factor named after the scale
        /// marker times `U` (`3·Bam16Rad·rad`, a binary angle). [to_fixed](Self::to_fixed) gives it back.
        pub fn from_fixed<U: Unit, P: FixedScale, T: Storage + Into<NumericValue>, B: Bounds<T>>(
            f: Fixed<U, P, T, B>,
        ) -> Self {
            if let Ok(q) = Quantity::try_from_fixed(f) {
                return q.into();
            }
            let scale = SIExprOwned::Named {
                name: P::NAME.to_string(),
                symbol: P::SYMBOL.to_string(),
                dim: Dim::ONE.into(),
                scale: Scale::Exact(P::SCALE),
            };
            AnyQuantityOwned {
                prefix: Prefix::Unit,
                value: f.get().into(),
                unit: SIExprOwned::Mul(Box::new(scale), Box::new(U::EXPR.into())),
            }
        }

        /// The value as a [Fixed] of the unit `U`, if it is exact (as [Fixed::to]) and within `B`: from `U` itself, or
        /// from any unit of `U`'s dimension whose factor to it is exact (a scale [from_fixed](Self::from_fixed) put
        /// in, `Bit` for `Byte`). `None` for another dimension or an `Affine` unit other than `U`.
        pub fn to_fixed<U: Unit, P: FixedScale, T: Storage, B: Bounds<T>>(
            &self,
        ) -> Option<Fixed<U, P, T, B>> {
            if let Some(q) = self.to_quantity::<U>() {
                return q.to_fixed();
            }
            if self.unit.dim()? != U::EXPR.dim() {
                return None;
            }
            let to_u = self
                .unit
                .magnitude()?
                .checked_div(U::EXPR.magnitude()?)
                .ok()?;
            Fixed::from_scaled(
                repr(&self.value)?,
                self.prefix.magnitude().checked_mul(to_u).ok()?,
            )
        }
    }
}

/// [Quantity::from_fixed].
impl<U: Unit, P: DecimalPrefix, T: Storage + Into<NumericValue>, B: Bounds<T>>
    From<Fixed<U, P, T, B>> for Quantity<U>
{
    fn from(f: Fixed<U, P, T, B>) -> Self {
        Quantity::from_fixed(f)
    }
}

#[cfg(test)]
mod tests {
    use crate::prefix::{Kibi, Milli};
    use crate::units::{Byte, Volt};
    use crate::{Fixed, Micros, Millis, Quantity};

    #[test]
    fn quantity_both_ways() {
        let t: Micros<u32> = Fixed::new(1500);
        assert_eq!(Quantity::from(t), crate::Second::micro(1500u32));
        assert_eq!(
            crate::Second::milli(3u8).to_fixed(),
            Some(Micros::<u16>::new(3000))
        );
        assert_eq!(
            crate::Second::micro(1500u32).to_fixed::<Milli, u16, _>(),
            None::<Millis<u16>>
        );
        assert_eq!(crate::Second::base(1u8).to_fixed(), None::<Millis<u8>>);
        assert_eq!(
            crate::Second::from_f32(0.25).to_fixed(),
            Some(Millis::<f32>::new(250.0))
        );
        assert_eq!(
            Quantity::<Byte>::kilo(2048u32).to_fixed(),
            Some(Fixed::<Byte, Kibi, u16>::new(2000))
        );
        assert_eq!(
            crate::Volt::micro(-2000i32).to_fixed(),
            Some(Fixed::<Volt, Milli, i16>::new(-2))
        );
    }

    #[test]
    fn scales_without_a_prefix() {
        use crate::prefix::Pow10;
        assert_eq!(
            crate::Second::micro(300u16).to_fixed(),
            Some(Fixed::<crate::units::Second, Pow10<-4>, u32>::new(3))
        );
    }

    #[test]
    fn exact_angles() {
        assert!((crate::Turn::base(1u8).to_si_f64() - core::f64::consts::TAU).abs() < 1e-15);
    }

    #[test]
    fn cycles_are_hertz_by_name() {
        use crate::units::{Cycle, Per, Second};
        let f = Quantity::<Per<Cycle, Second>>::new(crate::Prefix::Mega, 8u8);
        assert_eq!(f.to_hertz(), crate::Hertz::new(crate::Prefix::Mega, 8u8));
    }
}
