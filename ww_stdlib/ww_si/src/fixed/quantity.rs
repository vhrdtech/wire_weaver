//! A [Fixed] to and from a [Quantity]: the only place the compile-time [Fixed] meets the run-time prefix and
//! [NumericValue] of a [Quantity].

use super::{Bounds, DecimalPrefix, Fixed, FixedScale, Repr, Storage, rescale};
use crate::unit::Unit;
use crate::{ExactScale, Quantity};
use ww_numeric::NumericValue;

impl<U: Unit, P: FixedScale, T: Storage, B: Bounds<T>> Fixed<U, P, T, B> {
    /// The value of a [Quantity] in this unit, if it is exact (as [to](Self::to)) and within `B`.
    pub fn try_from_quantity(q: &Quantity<U>) -> Option<Self> {
        let scale = ExactScale::pow10(i8::from(q.prefix)).div(P::SCALE);
        let repr = if q.value.is_float() {
            Repr::Float(q.value.to_f64())
        } else {
            let x = q.value.to_integer()?;
            let m = i128::try_from(x.magnitude()).ok()?;
            Repr::Int(if x.is_negative() { -m } else { m })
        };
        let value = rescale(repr, scale)?;
        Self::try_new(value).ok()
    }
}

impl<U: Unit, P: DecimalPrefix, T: Storage, B: Bounds<T>> Fixed<U, P, T, B> {
    /// The same value as a [Quantity], prefix and number as they are. Only for number types `NumericValue` has
    /// enabled: a `u64` needs `ww_numeric`'s `u64` feature (on with `std`). A binary prefix is converted first
    /// ([to](Self::to) `One`).
    pub fn to_quantity(self) -> Quantity<U>
    where
        T: Into<NumericValue>,
    {
        Quantity::new(P::PREFIX, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefix::{Kibi, Milli};
    use crate::units::{Byte, Volt};
    use crate::{Micros, Millis};

    #[test]
    fn quantity_both_ways() {
        let t: Micros<u32> = Fixed::new(1500);
        assert_eq!(t.to_quantity(), crate::Second::micro(1500u32));
        assert_eq!(
            Micros::<u16>::try_from_quantity(&crate::Second::milli(3u8)),
            Some(Fixed::new(3000))
        );
        assert_eq!(
            Millis::<u16>::try_from_quantity(&crate::Second::micro(1500u32)),
            None
        );
        assert_eq!(
            Millis::<u8>::try_from_quantity(&crate::Second::base(1u8)),
            None
        );
        assert_eq!(
            Millis::<f32>::try_from_quantity(&crate::Second::from_f32(0.25)),
            Some(Fixed::new(250.0))
        );
        assert_eq!(
            Fixed::<Byte, Kibi, u16>::try_from_quantity(&crate::Quantity::<Byte>::kilo(2048u32)),
            Some(Fixed::new(2000))
        );
        assert_eq!(
            Fixed::<Volt, Milli, i16>::try_from_quantity(&crate::Volt::micro(-2000i32)),
            Some(Fixed::new(-2))
        );
    }
}
