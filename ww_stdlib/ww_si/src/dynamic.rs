//! Values whose unit is only known at run time: [AnyQuantity], a number with an [SIExpr].

#[cfg(feature = "std")]
use crate::SIExprOwned;
use crate::tree::{self, Tree};
use crate::{CompactError, Prefix, Quantity, SIExpr, Unit, UnitTable};
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
        pub fn to_si_f64<T: UnitTable + ?Sized>(&self, table: &T) -> Option<f64> {
            Some(self.to_f64() * self.unit.factor(table)?)
        }

        /// Like [AnyQuantity::to_quantity].
        pub fn to_quantity<U: Unit>(&self) -> Option<Quantity<U>> {
            let u = SIExprOwned::from(U::EXPR);
            self.unit
                .same(&u, U::EXPR)
                .then(|| Quantity::from_parts(self.prefix, self.value))
        }

        /// Like [AnyQuantity::display].
        pub fn display<'a, T: UnitTable + ?Sized>(
            &'a self,
            table: &'a T,
        ) -> impl fmt::Display + 'a {
            Shown(
                self.prefix,
                &self.value,
                tree::Resolved::new(&self.unit, table),
            )
        }

        /// The same value with each unit `table` lists as a [Ref](SIExpr::Ref), as [SIExprOwned::compact]: what a
        /// value sent beside the API that lists its units carries.
        pub fn compact<T: UnitTable + ?Sized>(
            &self,
            table: &T,
        ) -> Result<AnyQuantityOwned, CompactError<'_>> {
            Ok(AnyQuantityOwned {
                prefix: self.prefix,
                value: self.value,
                unit: self.unit.compact(table)?,
            })
        }

        /// The same value with each unit `table` lists in full, as [SIExprOwned::resolve]: it reads alone after.
        pub fn resolve<T: UnitTable + ?Sized>(
            &self,
            table: &T,
        ) -> Result<AnyQuantityOwned, CompactError<'_>> {
            Ok(AnyQuantityOwned {
                prefix: self.prefix,
                value: self.value,
                unit: self.unit.resolve(table)?,
            })
        }
    }

    /// Like `Display` of [AnyQuantity]: references through [units::TABLE](crate::units::TABLE).
    impl fmt::Display for AnyQuantityOwned {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.display(&crate::units::TABLE), f)
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
/// The unit's named parts are [Def](SIExpr::Def)s from `From` (the value reads alone anywhere), and
/// [Ref](SIExpr::Ref)s once [compacted](Self::compact_in) against a [UnitTable] that lists them: a value sent beside
/// its API names the API's units by identity, and the reader looks them up in the API model it has. The walks
/// ([to_si_f64](Self::to_si_f64), [display](Self::display)) take that table; `Display` reads through
/// [units::TABLE](crate::units::TABLE), so a `Ref` to a unit of another crate shows by its name.
///
/// On the wire the prefix, the value with its number type, and the unit: `3300 mV` is `12 e4 0c` (as `Volt`) and
/// then the `SIExpr` of `Volt`, its whole definition (31 bytes) or a reference to it (17 bytes).
///
/// ```
/// use ww_si::{AnyQuantity, SIExpr, Volt, units};
///
/// let v = AnyQuantity::from(Volt::milli(3300u16));
/// assert_eq!(format!("{v}"), "3300 mV");
/// assert_eq!(format!("{v:#}"), "3.3 V");
/// assert_eq!(v.to_quantity::<units::Volt>(), Some(Volt::milli(3300u16)));
///
/// let mut arena = [];
/// let r = v.compact_in(&units::TABLE, &mut arena).unwrap();
/// assert!(matches!(r.unit, SIExpr::Ref(_)));
/// assert_eq!(r.to_si_f64(&units::TABLE), Some(3.3));
/// assert_eq!(r.to_si_f64(&()), None);
/// assert_eq!(r.to_quantity::<units::Volt>(), Some(Volt::milli(3300u16)));
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

    /// The value in coherent SI through the unit's scale, like [Quantity::to_si_f64], the unit's references looked
    /// up in `table`; `None` when the unit's factor isn't known ([SIExpr::factor]).
    pub fn to_si_f64<T: UnitTable + ?Sized>(&self, table: &T) -> Option<f64> {
        Some(self.to_f64() * self.unit.factor(table)?)
    }

    /// The typed quantity, if the unit is `U`'s: the same unit as [SIExpr::same] says, a reference to one of `U`'s
    /// named units read as `U` defines it.
    pub fn to_quantity<U: Unit>(&self) -> Option<Quantity<U>> {
        self.unit
            .same(U::SI_EXPR, U::EXPR)
            .then(|| Quantity::from_parts(self.prefix, self.value))
    }

    /// The text of `Display`, the unit's references looked up in `table`: a reference to a device's own unit shows
    /// its symbol only through a table that lists it (the API model).
    pub fn display<'a, T: UnitTable + ?Sized>(&'a self, table: &'a T) -> impl fmt::Display + 'a {
        Shown(self.prefix, &self.value, self.unit.resolved(table))
    }

    /// The same value with each unit `table` lists as a [Ref](SIExpr::Ref), as [SIExpr::compact_in]: what a value
    /// sent beside the API that lists its units carries. `arena` takes the nodes rebuilt above a changed leaf; a
    /// named unit needs none.
    pub fn compact_in<'b, T: UnitTable + ?Sized>(
        &self,
        table: &T,
        arena: &'b mut [Option<SIExpr<'b>>],
    ) -> Result<AnyQuantity<'b>, CompactError<'b>>
    where
        'i: 'b,
    {
        Ok(AnyQuantity {
            prefix: self.prefix,
            value: self.value,
            unit: self.unit.compact_in(table, arena)?,
        })
    }
}

/// A quantity with its unit, as `Display` and [AnyQuantity::display] write it.
struct Shown<'a, E>(Prefix, &'a NumericValue, E);

impl<'a, E: Tree<'a>> fmt::Display for Shown<'_, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::quantity(self.0, self.1, &self.2), f)
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
/// the same unit writes it; references through [units::TABLE](crate::units::TABLE), see [display](AnyQuantity::display).
impl fmt::Display for AnyQuantity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.display(&crate::units::TABLE), f)
    }
}

/// `AnyQuantity(3300 mV U16)`.
impl fmt::Debug for AnyQuantity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AnyQuantity(")?;
        tree::write_number(&self.value, f)?;
        tree::write_unit(self.prefix, &self.unit.resolved(&crate::units::TABLE), f)?;
        write!(f, " {:?})", self.value.ty())
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl defmt::Format for AnyQuantity<'_> {
    fn format(&self, f: defmt::Formatter<'_>) {
        crate::quantity_impl::defmt_number(&self.value, f);
        if let SIExpr::Def(def) = &self.unit
            && def.names.symbol.is_empty()
        {
            Prefix::defmt_ratio(self.prefix, f);
        } else {
            defmt::write!(f, " {=str}{}", self.prefix.symbol(), self.unit);
        }
    }
}
