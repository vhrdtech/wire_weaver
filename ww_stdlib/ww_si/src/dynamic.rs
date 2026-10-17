//! Units known only at run time: [SIExpr] and [AnyQuantity], the same model as the markers of [units](crate::units)
//! put on the wire.

use crate::ops::scale_f64;
use crate::tree::{self, Node, Tree};
use crate::unit::{Dim, Scale, Unit, UnitExpr};
use crate::{Prefix, Quantity};
use core::fmt;
use shrink_wrap::prelude::*;
use ww_numeric::NumericValue;

/// A unit as a tree, on the wire: what [UnitExpr] is for a unit known from a type, for a unit known only at run time
/// (a configurable sensor channel, an ADC input whose unit is set by a jumper). A type-level unit maps one to one onto
/// an `SIExpr` ([Unit::SI_EXPR], [SIExpr::named]), so a dynamic client shows both the same way.
///
/// Children are [RefBox]es: decoding an `SIExpr` reads only its top node, the rest is decoded when walked. Every
/// walk stops at [MAX_UNIT_DEPTH](crate::MAX_UNIT_DEPTH) levels.
///
/// On the wire a nibble for the variant, then its fields: `Volt` is the name, the symbol, its [Dim] and [Scale].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Debug, PartialEq),
    ww_repr = u4,
    // a unit description: a reader can't show a unit kind it doesn't know
    no_future_version
)]
pub enum SIExpr<'i> {
    /// A named unit: `name` is the marker's (`"Volt"`, a device's own `"Rpm"`), `symbol` as written after a number.
    Named {
        name: &'i str,
        symbol: &'i str,
        dim: Dim,
        scale: Scale,
    },
    /// `A · B`, [Times](crate::units::Times).
    Mul(RefBox<'i, SIExpr<'i>>, RefBox<'i, SIExpr<'i>>),
    /// `A / B`, [Per](crate::units::Per).
    Div(RefBox<'i, SIExpr<'i>>, RefBox<'i, SIExpr<'i>>),
    /// `A` to an integer power.
    Pow(RefBox<'i, SIExpr<'i>>, i8),
}

impl SIExpr<'static> {
    /// The unit `U` on the wire: `SIExpr::of::<units::Volt>()`, [Unit::SI_EXPR].
    pub fn of<U: Unit>() -> SIExpr<'static> {
        U::SI_EXPR.clone()
    }

    /// The run-time form of a named [UnitExpr], in `const` code. Panics (a compile error in `const` code) for a
    /// compound one: those have no `'static` children to point to, [Unit::SI_EXPR] of `Per` / `Times` has them.
    pub const fn named(e: &'static UnitExpr) -> SIExpr<'static> {
        match *e {
            UnitExpr::Named {
                name,
                symbol,
                dim,
                scale,
            } => SIExpr::Named {
                name,
                symbol,
                dim,
                scale,
            },
            _ => panic!("SIExpr::named of a compound UnitExpr"),
        }
    }
}

impl<'i> SIExpr<'i> {
    /// SI base-unit exponents of the whole tree; `None` when one leaves -8..=7, a part doesn't decode or the tree is
    /// deeper than [MAX_UNIT_DEPTH](crate::MAX_UNIT_DEPTH).
    pub fn dim(&self) -> Option<Dim> {
        tree::dim(self)
    }

    /// The factor to coherent SI of the whole tree (offsets left out, as in [UnitExpr::factor]); `None` like
    /// [dim](Self::dim).
    pub fn factor(&self) -> Option<f64> {
        tree::factor(self)
    }

    /// The offset to coherent SI: only a named unit with an `Affine` scale (°C) has one.
    pub fn offset(&self) -> f64 {
        tree::offset(self)
    }

    /// The symbol as written after a number: `V`, `N·m/(rad/s)`, empty for a ratio. Same as `Display`.
    pub fn symbol(&self) -> impl fmt::Display + '_ {
        self
    }

    /// The type name, as the typed unit is called: `Volt`, `Per<Torque, AngularVelocity>`.
    pub fn name(&self) -> impl fmt::Display + '_ {
        tree::name(self)
    }

    /// The named unit of this crate this is, if it is one (same name, symbol, dimension and scale as in
    /// [by_name](crate::units::by_name)).
    pub fn known(&self) -> Option<&'static UnitExpr> {
        let SIExpr::Named { name, .. } = self else {
            return None;
        };
        crate::units::by_name(name).filter(|known| self == *known)
    }
}

impl<'i> Tree for SIExpr<'i> {
    fn node(&self) -> Option<Node<'_, Self>> {
        Some(match self {
            SIExpr::Named {
                name,
                symbol,
                dim,
                scale,
            } => Node::Named {
                name,
                symbol,
                dim: Some(*dim),
                scale: Some(*scale),
            },
            SIExpr::Mul(a, b) => Node::Mul(a.read().ok()?, b.read().ok()?),
            SIExpr::Div(a, b) => Node::Div(a.read().ok()?, b.read().ok()?),
            SIExpr::Pow(a, n) => Node::Pow(a.read().ok()?, *n),
        })
    }
}

/// The same unit tree: every named unit with the same name, symbol, dimension and scale, in the same shape.
impl PartialEq<UnitExpr> for SIExpr<'_> {
    fn eq(&self, other: &UnitExpr) -> bool {
        tree::same(self, &other)
    }
}

/// The symbol, written as [UnitExpr] writes the same unit: `V`, `N·m/(rad/s)`.
impl fmt::Display for SIExpr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::symbol(self), f)
    }
}

#[cfg(feature = "std")]
mod owned {
    use super::*;
    use std::boxed::Box;
    use std::string::ToString;

    impl Tree for &SIExprOwned {
        fn node(&self) -> Option<Node<'_, Self>> {
            Some(match *self {
                SIExprOwned::Named {
                    name,
                    symbol,
                    dim,
                    scale,
                } => Node::Named {
                    name,
                    symbol,
                    dim: Some(*dim),
                    scale: Some(*scale),
                },
                SIExprOwned::Mul(a, b) => Node::Mul(&**a, &**b),
                SIExprOwned::Div(a, b) => Node::Div(&**a, &**b),
                SIExprOwned::Pow(a, n) => Node::Pow(&**a, *n),
            })
        }
    }

    /// The run-time form of any [UnitExpr], compound ones too (a host builds it from a type's unit).
    impl From<&UnitExpr> for SIExprOwned {
        fn from(e: &UnitExpr) -> Self {
            match *e {
                UnitExpr::Named {
                    name,
                    symbol,
                    dim,
                    scale,
                } => SIExprOwned::Named {
                    name: name.to_string(),
                    symbol: symbol.to_string(),
                    dim,
                    scale,
                },
                UnitExpr::Mul(a, b) => SIExprOwned::Mul(Box::new(a.into()), Box::new(b.into())),
                UnitExpr::Div(a, b) => SIExprOwned::Div(Box::new(a.into()), Box::new(b.into())),
                UnitExpr::Pow(a, n) => SIExprOwned::Pow(Box::new(a.into()), n),
            }
        }
    }

    impl SIExprOwned {
        /// Like [SIExpr::dim].
        pub fn dim(&self) -> Option<Dim> {
            tree::dim(&self)
        }

        /// Like [SIExpr::factor].
        pub fn factor(&self) -> Option<f64> {
            tree::factor(&self)
        }

        /// Like [SIExpr::offset].
        pub fn offset(&self) -> f64 {
            tree::offset(&self)
        }

        /// Like [SIExpr::name].
        pub fn name(&self) -> impl fmt::Display + '_ {
            tree::name_of(self)
        }

        /// Like [SIExpr::known].
        pub fn known(&self) -> Option<&'static UnitExpr> {
            let SIExprOwned::Named { name, .. } = self else {
                return None;
            };
            crate::units::by_name(name).filter(|known| self == *known)
        }
    }

    impl PartialEq<UnitExpr> for SIExprOwned {
        fn eq(&self, other: &UnitExpr) -> bool {
            tree::same(&self, &other)
        }
    }

    impl fmt::Display for SIExprOwned {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&tree::symbol(&self), f)
        }
    }

    impl AnyQuantityOwned {
        /// Like [AnyQuantity::to_f64].
        pub fn to_f64(&self) -> f64 {
            scale_f64(self.value.to_f64(), self.prefix.into())
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

    impl From<&SIExpr<'_>> for SIExprOwned {
        fn from(e: &SIExpr<'_>) -> Self {
            fn conv(e: &SIExpr<'_>, depth: usize) -> SIExprOwned {
                // past the depth limit or not decoding: kept as an unknown named unit, shown as `?`
                let unknown = || SIExprOwned::Named {
                    name: "?".to_string(),
                    symbol: "?".to_string(),
                    dim: Dim::ONE,
                    scale: Scale::Coherent,
                };
                if depth >= crate::MAX_UNIT_DEPTH {
                    return unknown();
                }
                let child = |r: &RefBox<'_, SIExpr<'_>>| match r.read() {
                    Ok(e) => Box::new(conv(&e, depth + 1)),
                    Err(_) => Box::new(unknown()),
                };
                match e {
                    SIExpr::Named {
                        name,
                        symbol,
                        dim,
                        scale,
                    } => SIExprOwned::Named {
                        name: name.to_string(),
                        symbol: symbol.to_string(),
                        dim: *dim,
                        scale: *scale,
                    },
                    SIExpr::Mul(a, b) => SIExprOwned::Mul(child(a), child(b)),
                    SIExpr::Div(a, b) => SIExprOwned::Div(child(a), child(b)),
                    SIExpr::Pow(a, n) => SIExprOwned::Pow(child(a), *n),
                }
            }
            conv(e, 0)
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
        scale_f64(self.value.to_f64(), self.prefix.into())
    }

    /// The value in coherent SI through the unit's scale, like [Quantity::to_si_f64]; `None` when the unit's factor
    /// isn't known ([SIExpr::factor]).
    pub fn to_si_f64(&self) -> Option<f64> {
        Some(self.to_f64() * self.unit.factor()? + self.unit.offset())
    }

    /// The typed quantity, if the unit is `U`'s (the same tree, [SIExpr] `==` [UnitExpr]).
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

/// The symbol, like `Display`; a compound unit in parentheses per level (`(N·m/(rad/s))`), as for a typed quantity.
#[cfg(feature = "defmt")]
impl defmt::Format for SIExpr<'_> {
    fn format(&self, f: defmt::Formatter<'_>) {
        DefmtExpr(self, 0).format(f)
    }
}

#[cfg(feature = "defmt")]
struct DefmtExpr<'a, 'i>(&'a SIExpr<'i>, usize);

#[cfg(feature = "defmt")]
impl defmt::Format for DefmtExpr<'_, '_> {
    fn format(&self, f: defmt::Formatter<'_>) {
        let depth = self.1 + 1;
        if depth > crate::MAX_UNIT_DEPTH {
            return defmt::write!(f, "…");
        }
        let child = |r: &RefBox<'_, SIExpr<'_>>, f: defmt::Formatter<'_>| match r.read() {
            Ok(e) => DefmtExpr(&e, depth).format(f),
            Err(_) => defmt::write!(f, "?"),
        };
        match self.0 {
            SIExpr::Named { symbol, .. } => defmt::write!(f, "{=str}", symbol),
            SIExpr::Mul(a, b) => {
                defmt::write!(f, "(");
                child(a, f);
                defmt::write!(f, "·");
                child(b, f);
                defmt::write!(f, ")");
            }
            SIExpr::Div(a, b) => {
                defmt::write!(f, "(");
                child(a, f);
                defmt::write!(f, "/");
                child(b, f);
                defmt::write!(f, ")");
            }
            SIExpr::Pow(a, n) => {
                defmt::write!(f, "(");
                child(a, f);
                defmt::write!(f, "^{=i8})", n);
            }
        }
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl defmt::Format for AnyQuantity<'_> {
    fn format(&self, f: defmt::Formatter<'_>) {
        crate::quantity::defmt_number(&self.value, f);
        if let SIExpr::Named { symbol: "", .. } = self.unit {
            crate::quantity::defmt_ratio_prefix(self.prefix, f);
        } else {
            defmt::write!(f, " {=str}{}", self.prefix.symbol(), self.unit);
        }
    }
}
