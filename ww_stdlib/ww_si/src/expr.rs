//! One walk over a unit tree for [UnitExpr](crate::UnitExpr), [SIExpr](crate::SIExpr) and its owned form: symbol,
//! type name, dimension, factor and comparison are written once, so a unit known at run time is shown exactly like
//! the same unit known from a type.

use crate::unit::{Dim, Scale, write_superscript};
use core::fmt;

/// Deepest nesting of a unit tree that is followed. A run-time unit comes from another device, so its depth is
/// bounded here and not by the stack: a deeper part is shown as `…` and has no dimension or factor.
pub const MAX_UNIT_DEPTH: usize = 32;

/// One node of a unit tree, its children as `E`.
pub(crate) enum Node<'a, E> {
    Named {
        name: &'a str,
        symbol: &'a str,
        dim: Dim,
        scale: Scale,
    },
    Mul(E, E),
    Div(E, E),
    Pow(E, i8),
}

/// A unit tree: [UnitExpr](crate::UnitExpr) (`&'static` children) or [SIExpr](crate::SIExpr) (children decoded on
/// the way).
pub(crate) trait Tree: Sized {
    /// The node, `None` when its children don't decode.
    fn node(&self) -> Option<Node<'_, Self>>;
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
pub(crate) fn write_symbol<E: Tree>(
    e: &E,
    f: &mut fmt::Formatter<'_>,
    depth: usize,
) -> fmt::Result {
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
pub(crate) fn write_name<E: Tree>(e: &E, f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
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
pub(crate) fn dim<E: Tree>(e: &E, depth: usize) -> Option<Dim> {
    if depth >= MAX_UNIT_DEPTH {
        return None;
    }
    match e.node()? {
        Node::Named { dim, .. } => Some(dim),
        Node::Mul(a, b) => dim(&a, depth + 1)?.checked_mul(dim(&b, depth + 1)?),
        Node::Div(a, b) => dim(&a, depth + 1)?.checked_div(dim(&b, depth + 1)?),
        Node::Pow(a, n) => dim(&a, depth + 1)?.checked_pow(n),
    }
}

/// Factor to coherent SI of the whole tree (offsets left out), `None` when a part doesn't decode or it is too deep.
pub(crate) fn factor<E: Tree>(e: &E, depth: usize) -> Option<f64> {
    if depth >= MAX_UNIT_DEPTH {
        return None;
    }
    match e.node()? {
        Node::Named { scale, .. } => Some(scale.factor()),
        Node::Mul(a, b) => Some(factor(&a, depth + 1)? * factor(&b, depth + 1)?),
        Node::Div(a, b) => Some(factor(&a, depth + 1)? / factor(&b, depth + 1)?),
        Node::Pow(a, n) => Some(crate::ops::powi_f64(factor(&a, depth + 1)?, n)),
    }
}

/// Offset to coherent SI: only a named unit with an `Affine` scale (°C) has one.
pub(crate) fn offset<E: Tree>(e: &E) -> f64 {
    match e.node() {
        Some(Node::Named { scale, .. }) => scale.offset(),
        _ => 0.0,
    }
}

/// A named unit without a symbol ([Ratio](crate::units::Ratio)): a prefix is written as %, ‰, ppm.
pub(crate) fn is_ratio<E: Tree>(e: &E) -> bool {
    matches!(e.node(), Some(Node::Named { symbol: "", .. }))
}

/// A named unit (no parentheses after a prefix).
pub(crate) fn is_named<E: Tree>(e: &E) -> bool {
    matches!(e.node(), Some(Node::Named { .. }))
}

/// The same tree: names, symbols, dimensions and scales of every named unit, and the same shape.
pub(crate) fn same<A: Tree, B: Tree>(a: &A, b: &B, depth: usize) -> bool {
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
            same(&a1, &a2, d) && same(&b1, &b2, d)
        }
        (Node::Pow(a1, n1), Node::Pow(a2, n2)) => n1 == n2 && same(&a1, &a2, d),
        _ => false,
    }
}

/// The prefix and the unit's symbol, as written after a number: ` mV`, ` kg`, ` %` for a ratio in `Centi`,
/// ` k(N·m/(rad/s))` for a compound unit.
pub(crate) fn write_unit<E: Tree>(
    prefix: crate::Prefix,
    unit: &E,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    use crate::Prefix;
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
pub(crate) struct Show<'a, E>(
    pub &'a E,
    pub fn(&E, &mut fmt::Formatter<'_>, usize) -> fmt::Result,
);

impl<E> fmt::Display for Show<'_, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.1)(self.0, f, 0)
    }
}
