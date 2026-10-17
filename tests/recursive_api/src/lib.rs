//! Self-referential user types in an API: an expression tree through `RefBox` and a tree through `RefVec`, as
//! method arguments and return values, a property and stream items.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
trait Recursive {
    /// Value of the expression, `None` when it is nested too deep or has a variant from a newer version
    fn eval(expr: Expr<'i>) -> Option<i32>;
    /// The expression with its outermost negation removed
    fn unwrap_neg(expr: Expr<'i>) -> Option<Expr<'i>>;
    property!(rw tree: Tree<'i>);
    stream!(exprs: Expr<'i>);
}

/// An integer expression
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone),
    ww_repr = u4
)]
pub enum Expr<'i> {
    Add(RefBox<'i, Expr<'i>>, RefBox<'i, Expr<'i>>),
    Mul {
        a: RefBox<'i, Expr<'i>>,
        b: RefBox<'i, Expr<'i>>,
    },
    Neg(RefBox<'i, Expr<'i>>),
    Lit(i32),
}

/// A labelled tree, children in a list
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Tree<'i> {
    pub label: &'i str,
    pub children: RefVec<'i, Tree<'i>>,
}
