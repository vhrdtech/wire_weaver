//! A [Fixed] to and from a [Quantity]: the only place the compile-time [Fixed] meets the run-time prefix and
//! [NumericValue] of a [Quantity].

use crate::{Prefix, Quantity};
use core::fmt;
use exact_units::{Bounds, DecimalPrefix, Fixed, FixedScale, Repr, Storage, Unit};
use ww_numeric::NumericValue;

/// A [Fixed]'s scale that no run-time [Prefix] stands for in this build: without `std`, a scale that is neither a named
/// prefix nor one of the computed [ExactScale](exact_units::ExactScale)s (`Rational<33, 10>` over 4096, an ADC step);
/// with `std`, only once the interner of exact scales is full.
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
        let repr = if self.value.is_float() {
            Repr::Float(self.value.to_f64())
        } else {
            let x = self.value.to_integer()?;
            let m = i128::try_from(x.magnitude()).ok()?;
            Repr::Int(if x.is_negative() { -m } else { m })
        };
        Fixed::from_scaled(repr, self.prefix.magnitude())
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
    /// `Kibi`), else [Prefix::Exact] (`Pow10<-4>`, `FullScaleMax<One, 16>`, a BAM), so the value is exact and converts
    /// back with [to_fixed](Self::to_fixed). [UnknownScale] only where no prefix stands for the scale, see there.
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
