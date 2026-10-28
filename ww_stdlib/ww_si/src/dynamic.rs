//! Values whose unit is only known at run time: [AnyQuantity], a number with an [SIExpr].

#[cfg(feature = "std")]
use crate::SIExprOwned;
use crate::tree;
use crate::{Prefix, Quantity, SIExpr, Unit};
use core::fmt;
use shrink_wrap::prelude::*;
use ww_numeric::NumericValue;

#[cfg(feature = "std")]
mod owned {
    use super::*;

    impl AnyQuantityOwned {
        /// Like [AnyQuantity::to_f64].
        pub fn to_f64(&self) -> f64 {
            self.prefix.scale_f64(self.value.to_f64())
        }

        /// Like [AnyQuantity::to_si_f64].
        pub fn to_si_f64(&self) -> Option<f64> {
            Some(self.to_f64() * self.unit.factor()? + self.unit.offset())
        }

        /// Like [AnyQuantity::to_quantity].
        pub fn to_quantity<U: Unit>(&self) -> Option<Quantity<U>> {
            (self.unit == *U::EXPR).then(|| Quantity::from_parts(self.prefix, self.value))
        }
    }

    /// Like `Display` of [AnyQuantity].
    impl fmt::Display for AnyQuantityOwned {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&tree::quantity(self.prefix, &self.value, &&self.unit), f)
        }
    }

    impl<U: Unit> From<Quantity<U>> for AnyQuantityOwned {
        fn from(q: Quantity<U>) -> Self {
            AnyQuantityOwned {
                prefix: q.prefix,
                value: q.value,
                unit: U::EXPR.into(),
            }
        }
    }

    impl From<&AnyQuantity<'_>> for AnyQuantityOwned {
        fn from(q: &AnyQuantity<'_>) -> Self {
            AnyQuantityOwned {
                prefix: q.prefix,
                value: q.value,
                unit: SIExprOwned::from(&q.unit),
            }
        }
    }
}

/// A value whose unit is only known at run time: a prefix, a number in whatever storage the device picks, and the
/// unit as an [SIExpr]. Shown, compared to SI and converted like a [Quantity] of the same unit; a typed quantity
/// becomes one with `From`, and back with [to_quantity](Self::to_quantity).
///
/// On the wire the prefix, the value with its number type, and the unit: `3300 mV` is `12 e4 0c` (as `Volt`) and
/// then the `SIExpr` of `Volt`.
///
/// ```
/// use ww_si::{AnyQuantity, Volt};
///
/// let v = AnyQuantity::from(Volt::milli(3300u16));
/// assert_eq!(format!("{v}"), "3300 mV");
/// assert_eq!(format!("{v:#}"), "3.3 V");
/// assert_eq!(v.to_quantity::<ww_si::units::Volt>(), Some(Volt::milli(3300u16)));
/// ```
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, PartialEq),
    derive_owned(Debug)
)]
pub struct AnyQuantity<'i> {
    pub prefix: Prefix,
    pub value: NumericValue,
    pub unit: SIExpr<'i>,
}

impl<'i> AnyQuantity<'i> {
    /// A value of `value` in units of `prefix`, in `unit`.
    pub fn new(prefix: Prefix, value: impl Into<NumericValue>, unit: SIExpr<'i>) -> Self {
        AnyQuantity {
            prefix,
            value: value.into(),
            unit,
        }
    }

    /// The value in the unit itself (no prefix) as `f64`, like [Quantity::to_f64].
    pub fn to_f64(&self) -> f64 {
        self.prefix.scale_f64(self.value.to_f64())
    }

    /// The value in coherent SI through the unit's scale, like [Quantity::to_si_f64]; `None` when the unit's factor
    /// isn't known ([SIExpr::factor]).
    pub fn to_si_f64(&self) -> Option<f64> {
        Some(self.to_f64() * self.unit.factor()? + self.unit.offset())
    }

    /// The typed quantity, if the unit is `U`'s (the same tree, [SIExpr] `==` [UnitExpr](crate::UnitExpr)).
    pub fn to_quantity<U: Unit>(&self) -> Option<Quantity<U>> {
        (self.unit == *U::EXPR).then(|| Quantity::from_parts(self.prefix, self.value))
    }
}

impl<U: Unit> From<Quantity<U>> for AnyQuantity<'static> {
    fn from(q: Quantity<U>) -> Self {
        AnyQuantity {
            prefix: q.prefix,
            value: q.value,
            unit: U::SI_EXPR.clone(),
        }
    }
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`. As a [Quantity] of
/// the same unit writes it.
impl fmt::Display for AnyQuantity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::quantity(self.prefix, &self.value, &self.unit), f)
    }
}

/// `AnyQuantity(3300 mV U16)`.
impl fmt::Debug for AnyQuantity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AnyQuantity(")?;
        tree::write_number(&self.value, f)?;
        tree::write_unit(self.prefix, &self.unit, f)?;
        write!(f, " {:?})", self.value.ty())
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl defmt::Format for AnyQuantity<'_> {
    fn format(&self, f: defmt::Formatter<'_>) {
        crate::quantity::defmt_number(&self.value, f);
        if let SIExpr::Named { symbol: "", .. } = self.unit {
            Prefix::defmt_ratio(self.prefix, f);
        } else {
            defmt::write!(f, " {=str}{}", self.prefix.symbol(), self.unit);
        }
    }
}
