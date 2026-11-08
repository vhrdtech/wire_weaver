//! A [Fixed] to and from a [Quantity]: the only place the compile-time [Fixed] meets the run-time prefix and
//! [NumericValue] of a [Quantity].

use crate::{Prefix, Quantity};
use core::fmt;
use exact_units::{Bounds, DecimalPrefix, Fixed, FixedScale, Repr, Storage, Unit};
use ww_numeric::{Magnitude, NumericValue};

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
    /// The value in the prefix `P` and storage `T` of a [Fixed], if it is exact (as [Fixed::to]) and within `B`: a
    /// float on either side too gives the exact value or `None` (exact_units NUM-30), so 516558.94 mV of an `f32` is no
    /// `f32` of µV. Into an integer storage where `P` has a run-time prefix (a named one, `Pow10`, `Pow2`,
    /// `FullScale`) on exact_units' integer kernel at the width of the enabled number types: no float code for an
    /// integer, an `f32` rounded once and checked against the exact product, no `f64`. Into a float storage, from an
    /// `f64`, or with any other scale (`Rational<33, 10>` over 4096), through [Fixed::from_scaled] and a
    /// [Magnitude](crate::Magnitude), a host's path.
    pub fn to_fixed<P: FixedScale, T: Storage, B: Bounds<T>>(&self) -> Option<Fixed<U, P, T, B>> {
        let exact = || Fixed::from_scaled(repr(&self.value)?, self.prefix.magnitude()?);
        let Some(to) = P::RUNTIME else {
            return exact();
        };
        if T::IS_FLOAT {
            return exact();
        }
        let ratio = self.prefix.ratio_to(to)?;
        let v = match self.value.to_integer() {
            Some(x) => {
                let m = ratio.apply::<Magnitude, Magnitude>(x.magnitude())?;
                // A no-op only when ww_numeric's `u128` makes `Magnitude` a u128.
                #[allow(clippy::useless_conversion)]
                let m = i128::try_from(m).ok()?;
                T::from_int(if x.is_negative() { -m } else { m })?
            }
            None => match self.value {
                #[cfg(feature = "f32")]
                NumericValue::F32(x) => f32_to_int::<T>(x, &ratio)?,
                // a host's
                #[cfg(feature = "f64")]
                NumericValue::F64(_) => return exact(),
                // `UN` / `IN`
                _ => return None,
            },
        };
        Fixed::try_new(v).ok()
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

/// `x · ratio` in the integer storage `T`, if it is a whole number that fits: rounded once to the nearest `f32` on
/// the integer kernel, then compared with the exact product there, so a firmware links no `f64` and, for a storage
/// of up to 32 bits, no 128-bit code.
#[cfg(feature = "f32")]
fn f32_to_int<T: Storage>(x: f32, ratio: &exact_units::PrefixRatio) -> Option<T> {
    let v = T::from_f32(ratio.apply_f32(x))?;
    let same = if T::BITS <= 32 {
        ratio.cmp_scaled_f32(x, v.to_i64())
    } else {
        match v.to_repr() {
            Repr::Int(i) => ratio.cmp_scaled_f32(x, i),
            Repr::Float(_) => None,
        }
    };
    (same == Some(core::cmp::Ordering::Equal)).then_some(v)
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
    use exact_units::{Dim, MarkerId, Provenance, UnitDef, UnitNames, UnitTable};
    use std::boxed::Box;

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
            // TODO(STD-74): a scale is no unit; a generic scale without an id (`Pow2<-15>`) is named by its NAME alone
            let scale = SIExprOwned::Def(
                UnitDef {
                    id: P::ID.unwrap_or(MarkerId {
                        crate_name: "",
                        name: P::NAME,
                    }),
                    names: UnitNames { symbol: P::SYMBOL },
                    dim: Dim::ONE,
                    quantity: None,
                    scale: P::SCALE,
                    provenance: Provenance::Defined,
                }
                .into(),
            );
            AnyQuantityOwned {
                prefix: Prefix::Unit,
                value: f.get().into(),
                unit: SIExprOwned::Mul(Box::new(scale), Box::new(U::EXPR.into())),
            }
        }

        /// The value as a [Fixed] of the unit `U`, if it is exact (as [Fixed::to]) and within `B`: from `U` itself, or
        /// from any unit of `U`'s dimension whose factor to it is exact (a scale [from_fixed](Self::from_fixed) put
        /// in, `Bit` for `Byte`), the unit's references looked up in `table`. `None` for another dimension, or a
        /// reference `table` doesn't list.
        pub fn to_fixed<
            U: Unit,
            P: FixedScale,
            T: Storage,
            B: Bounds<T>,
            Tb: UnitTable + ?Sized,
        >(
            &self,
            table: &Tb,
        ) -> Option<Fixed<U, P, T, B>> {
            if let Some(q) = self.to_quantity::<U>() {
                return q.to_fixed();
            }
            if self.unit.dim(table)? != U::EXPR.dim() {
                return None;
            }
            let to_u = self
                .unit
                .magnitude(table)?
                .checked_div(U::EXPR.magnitude()?)
                .ok()?;
            Fixed::from_scaled(
                repr(&self.value)?,
                self.prefix.magnitude()?.checked_mul(to_u).ok()?,
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
            Millis::<f32>::try_new(250.0).ok()
        );
        // a NaN quantity has no NaN-free float Fixed (NUM-19): no value, never a panic
        assert_eq!(
            crate::Second::from_f32(f32::NAN).to_fixed(),
            None::<Millis<f32>>
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
