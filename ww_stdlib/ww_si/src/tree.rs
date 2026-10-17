//! One walk over a unit tree: symbol, type name, dimension, factor, comparison and a quantity's `Display` are
//! written once, for [UnitExpr](crate::UnitExpr), [SIExpr](crate::SIExpr) and its owned form, and for a client's own
//! unit model (`wire_weaver_client`'s `DynUnit`): a unit known at run time or only from introspection data is shown
//! exactly like the same unit known from a type.
//!
//! A unit model implements [Tree], the functions here do the rest:
//!
//! ```
//! use ww_si::tree::{self, Node, Tree};
//! use ww_si::Prefix;
//! use ww_si::ww_numeric::NumericValue;
//!
//! /// A unit as some client keeps it: names it only knows, and quotients of them.
//! enum MyUnit {
//!     Name(&'static str),
//!     Per(Box<MyUnit>, Box<MyUnit>),
//! }
//!
//! impl Tree for &MyUnit {
//!     fn node(&self) -> Option<Node<'_, Self>> {
//!         Some(match *self {
//!             MyUnit::Name(n) => Node::Named { name: n, symbol: n, dim: None, scale: None },
//!             MyUnit::Per(a, b) => Node::Div(&**a, &**b),
//!         })
//!     }
//! }
//!
//! let u = MyUnit::Per(Box::new(MyUnit::Name("rpm")), Box::new(MyUnit::Name("s")));
//! assert_eq!(tree::symbol(&&u).to_string(), "rpm/s");
//! assert_eq!(tree::name(&&u).to_string(), "Per<rpm, s>");
//! assert_eq!(tree::dim(&&u), None);
//! let v = NumericValue::U8(12);
//! assert_eq!(tree::quantity(Prefix::Kilo, &v, &&u).to_string(), "12 k(rpm/s)");
//! ```

#[cfg(feature = "f32")]
use crate::ops::scale_f32;
use crate::ops::scale_f64;
use crate::unit::{Dim, Scale, write_superscript};
use crate::{Prefix, UnitExpr};
use core::fmt;
use ww_numeric::NumericValue;

/// Deepest nesting of a unit tree that is followed. A run-time unit comes from another device, so its depth is
/// bounded here and not by the stack: a deeper part is shown as `…` and has no dimension or factor.
pub const MAX_UNIT_DEPTH: usize = 32;

/// One node of a unit tree, its children as `E`.
pub enum Node<'a, E> {
    /// A named unit. `dim` and `scale` are `None` for a unit known by its name only (a user's marker that a client
    /// sees in introspection data): it is shown by name, without a dimension or a conversion to SI.
    Named {
        /// The marker's name: `Volt`.
        name: &'a str,
        /// As written after a number: `V`; a unit known by name only has its name here.
        symbol: &'a str,
        dim: Option<Dim>,
        scale: Option<Scale>,
    },
    /// `A · B`
    Mul(E, E),
    /// `A / B`
    Div(E, E),
    /// `A` to an integer power
    Pow(E, i8),
}

impl<'a, E> Node<'a, E> {
    /// The same node with its children turned into another tree's (a named node has none).
    pub fn map<F>(self, mut f: impl FnMut(E) -> F) -> Node<'a, F> {
        match self {
            Node::Named {
                name,
                symbol,
                dim,
                scale,
            } => Node::Named {
                name,
                symbol,
                dim,
                scale,
            },
            Node::Mul(a, b) => Node::Mul(f(a), f(b)),
            Node::Div(a, b) => Node::Div(f(a), f(b)),
            Node::Pow(a, n) => Node::Pow(f(a), n),
        }
    }
}

/// A unit tree: [UnitExpr](crate::UnitExpr) (`&'static` children), [SIExpr](crate::SIExpr) (children decoded on the
/// way), or a client's own model. Children are `Self`, usually a reference into the tree.
pub trait Tree: Sized {
    /// The node, `None` when it can't be read (children that don't decode).
    fn node(&self) -> Option<Node<'_, Self>>;
}

/// The symbol as written after a number: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`, `m²`.
pub fn symbol<E: Tree>(e: &E) -> impl fmt::Display + '_ {
    Show(e, |e, f, depth| write_symbol(*e, f, depth))
}

/// The type name: `Volt`, `Per<Torque, AngularVelocity>`, `Pow<Meter, 2>`.
pub fn name<E: Tree>(e: &E) -> impl fmt::Display + '_ {
    Show(e, |e, f, depth| write_name(*e, f, depth))
}

/// [name] of a tree that is a reference itself (`&UnitExpr`), for an `impl Display` that outlives the call.
pub(crate) fn name_of<E: Tree>(e: E) -> impl fmt::Display {
    Show(e, write_name)
}

/// SI exponents of the whole tree; `None` when an exponent leaves -8..=7, a part is known by name only or doesn't
/// decode, or the tree is deeper than [MAX_UNIT_DEPTH].
pub fn dim<E: Tree>(e: &E) -> Option<Dim> {
    dim_at(e, 0)
}

/// Factor to coherent SI of the whole tree (offsets left out: a product with °C is not affine); `None` like [dim].
pub fn factor<E: Tree>(e: &E) -> Option<f64> {
    factor_at(e, 0)
}

/// The same tree: names, symbols, dimensions and scales of every named unit, and the same shape.
pub fn same<A: Tree, B: Tree>(a: &A, b: &B) -> bool {
    same_at(a, b, 0)
}

/// A quantity as `Display` writes it: `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit
/// itself, `3.3 V`. The text of a typed [Quantity](crate::Quantity) of the same unit.
pub fn quantity<'a, E: Tree>(
    prefix: Prefix,
    value: &'a NumericValue,
    unit: &'a E,
) -> impl fmt::Display + 'a {
    struct Q<'a, E>(Prefix, &'a NumericValue, &'a E);
    impl<E: Tree> fmt::Display for Q<'_, E> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Q(prefix, value, unit) = *self;
            if f.alternate() {
                match *value {
                    #[cfg(feature = "f32")]
                    NumericValue::F32(x) => write!(f, "{}", scale_f32(x, prefix.into()))?,
                    _ => write!(f, "{}", scale_f64(value.to_f64(), prefix.into()))?,
                }
                write_unit(Prefix::Unit, unit, f)
            } else {
                write_number(value, f)?;
                write_unit(prefix, unit, f)
            }
        }
    }
    Q(prefix, value, unit)
}

/// The number as stored: integers exactly, `f32` as `f32` (3.3, not 3.299999952316284).
pub(crate) fn write_number(value: &NumericValue, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => write!(f, "{x}"),
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => write!(f, "{x}"),
        value => match value.to_i128() {
            Some(x) => write!(f, "{x}"),
            // u128 above i128::MAX, UN / IN
            None => write!(f, "{}", value.to_f64()),
        },
    }
}

impl Tree for &UnitExpr {
    fn node(&self) -> Option<Node<'_, Self>> {
        Some(match **self {
            UnitExpr::Named {
                name,
                symbol,
                dim,
                scale,
            } => Node::Named {
                name,
                symbol,
                dim: Some(dim),
                scale: Some(scale),
            },
            UnitExpr::Mul(a, b) => Node::Mul(a, b),
            UnitExpr::Div(a, b) => Node::Div(a, b),
            UnitExpr::Pow(a, n) => Node::Pow(a, n),
        })
    }
}

/// Needs parentheses when it stands next to `·` or `/`.
fn is_compound<E>(node: &Node<'_, E>) -> bool {
    match node {
        Node::Named { symbol, .. } => symbol.contains(['/', '·']),
        // `m²`, `(m/s)²`: the power binds tighter than `·` and `/`
        Node::Pow(..) => false,
        _ => true,
    }
}

/// The symbol: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`, `m²`.
fn write_symbol<E: Tree>(e: &E, f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    if depth >= MAX_UNIT_DEPTH {
        return f.write_str("…");
    }
    let Some(node) = e.node() else {
        return f.write_str("?");
    };
    let side = |f: &mut fmt::Formatter<'_>, e: &E, parens: &dyn Fn(&Node<'_, E>) -> bool| {
        let parens = e.node().is_some_and(|n| parens(&n));
        if parens {
            f.write_str("(")?;
        }
        write_symbol(e, f, depth + 1)?;
        if parens {
            f.write_str(")")?;
        }
        Ok(())
    };
    match &node {
        Node::Named { symbol, .. } => f.write_str(symbol),
        Node::Mul(a, b) => {
            let parens = |n: &Node<'_, E>| is_compound(n) && !matches!(n, Node::Mul(..));
            side(f, a, &parens)?;
            f.write_str("·")?;
            side(f, b, &parens)
        }
        Node::Div(a, b) => {
            side(f, a, &|n| matches!(n, Node::Div(..)))?;
            f.write_str("/")?;
            side(f, b, &is_compound)
        }
        Node::Pow(a, n) => {
            side(f, a, &|n| is_compound(n) || matches!(n, Node::Pow(..)))?;
            write_superscript(f, *n)
        }
    }
}

/// The type name: `Volt`, `Per<Torque, AngularVelocity>`, `Pow<Meter, 2>`.
fn write_name<E: Tree>(e: &E, f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    if depth >= MAX_UNIT_DEPTH {
        return f.write_str("…");
    }
    let Some(node) = e.node() else {
        return f.write_str("?");
    };
    let pair = |f: &mut fmt::Formatter<'_>, ty: &str, a: &E, b: &E| {
        write!(f, "{ty}<")?;
        write_name(a, f, depth + 1)?;
        f.write_str(", ")?;
        write_name(b, f, depth + 1)?;
        f.write_str(">")
    };
    match &node {
        Node::Named { name, .. } => f.write_str(name),
        Node::Mul(a, b) => pair(f, "Times", a, b),
        Node::Div(a, b) => pair(f, "Per", a, b),
        Node::Pow(a, n) => {
            f.write_str("Pow<")?;
            write_name(a, f, depth + 1)?;
            write!(f, ", {n}>")
        }
    }
}

/// SI exponents of the whole tree, `None` when an exponent leaves -8..=7, a part doesn't decode or it is too deep.
fn dim_at<E: Tree>(e: &E, depth: usize) -> Option<Dim> {
    if depth >= MAX_UNIT_DEPTH {
        return None;
    }
    let d = depth + 1;
    match e.node()? {
        Node::Named { dim, .. } => dim,
        Node::Mul(a, b) => dim_at(&a, d)?.checked_mul(dim_at(&b, d)?),
        Node::Div(a, b) => dim_at(&a, d)?.checked_div(dim_at(&b, d)?),
        Node::Pow(a, n) => dim_at(&a, d)?.checked_pow(n),
    }
}

/// Factor to coherent SI of the whole tree (offsets left out), `None` when a part doesn't decode or it is too deep.
fn factor_at<E: Tree>(e: &E, depth: usize) -> Option<f64> {
    if depth >= MAX_UNIT_DEPTH {
        return None;
    }
    let d = depth + 1;
    match e.node()? {
        Node::Named { scale, .. } => Some(scale?.factor()),
        Node::Mul(a, b) => Some(factor_at(&a, d)? * factor_at(&b, d)?),
        Node::Div(a, b) => Some(factor_at(&a, d)? / factor_at(&b, d)?),
        Node::Pow(a, n) => Some(crate::ops::powi_f64(factor_at(&a, d)?, n)),
    }
}

/// Offset to coherent SI: only a named unit with an `Affine` scale (°C) has one.
pub fn offset<E: Tree>(e: &E) -> f64 {
    match e.node() {
        Some(Node::Named {
            scale: Some(scale), ..
        }) => scale.offset(),
        _ => 0.0,
    }
}

/// A named unit without a symbol ([Ratio](crate::units::Ratio)): a prefix is written as %, ‰, ppm.
pub fn is_ratio<E: Tree>(e: &E) -> bool {
    matches!(e.node(), Some(Node::Named { symbol: "", .. }))
}

/// A named unit (no parentheses after a prefix).
fn is_named<E: Tree>(e: &E) -> bool {
    matches!(e.node(), Some(Node::Named { .. }))
}

fn same_at<A: Tree, B: Tree>(a: &A, b: &B, depth: usize) -> bool {
    if depth >= MAX_UNIT_DEPTH {
        return false;
    }
    let (Some(a), Some(b)) = (a.node(), b.node()) else {
        return false;
    };
    let d = depth + 1;
    match (a, b) {
        (
            Node::Named {
                name: n1,
                symbol: s1,
                dim: d1,
                scale: c1,
            },
            Node::Named {
                name: n2,
                symbol: s2,
                dim: d2,
                scale: c2,
            },
        ) => n1 == n2 && s1 == s2 && d1 == d2 && c1 == c2,
        (Node::Mul(a1, b1), Node::Mul(a2, b2)) | (Node::Div(a1, b1), Node::Div(a2, b2)) => {
            same_at(&a1, &a2, d) && same_at(&b1, &b2, d)
        }
        (Node::Pow(a1, n1), Node::Pow(a2, n2)) => n1 == n2 && same_at(&a1, &a2, d),
        _ => false,
    }
}

/// The prefix and the unit's symbol, as written after a number: ` mV`, ` kg`, ` %` for a ratio in `Centi`,
/// ` k(N·m/(rad/s))` for a compound unit.
pub(crate) fn write_unit<E: Tree>(
    prefix: Prefix,
    unit: &E,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    if is_ratio(unit) {
        return match prefix {
            Prefix::Unit => Ok(()),
            Prefix::Centi => f.write_str(" %"),
            Prefix::Milli => f.write_str(" ‰"),
            Prefix::Micro => f.write_str(" ppm"),
            p => write!(f, "e{}", i8::from(p)),
        };
    }
    f.write_str(" ")?;
    match (prefix, is_named(unit)) {
        (Prefix::Unit, _) => write_symbol(unit, f, 0),
        (p, true) => {
            f.write_str(p.symbol())?;
            write_symbol(unit, f, 0)
        }
        (p, false) => {
            write!(f, "{}(", p.symbol())?;
            write_symbol(unit, f, 0)?;
            f.write_str(")")
        }
    }
}

/// Writes a tree with one of the functions above, for `impl Display` returns.
struct Show<T>(T, fn(&T, &mut fmt::Formatter<'_>, usize) -> fmt::Result);

impl<T> fmt::Display for Show<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.1)(&self.0, f, 0)
    }
}
