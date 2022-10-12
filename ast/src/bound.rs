use std::fmt::{Display, Formatter};
use crate::ast::expr::{TryEvaluateInto, VecExpr};
use parser::ast::num_bound::NumBound as NumBoundParser;
use crate::ast::set::Set;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumBound {
    Unbound,
    MaxBound(usize),
    // Set(TryEvaluateInto<VecExpr, VecLit>),
    Set(TryEvaluateInto<VecExpr, Set>),
}

impl<'i> From<NumBoundParser<'i>> for NumBound {
    fn from(n: NumBoundParser<'i>) -> Self {
        match n {
            NumBoundParser::Unbound => NumBound::Unbound,
            NumBoundParser::MaxBound(max) => NumBound::MaxBound(max),
            NumBoundParser::Set(exprs) => NumBound::Set(TryEvaluateInto::NotResolved(exprs.into())),
        }
    }
}

impl Display for NumBound {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            NumBound::Unbound => write!(f, "?"),
            NumBound::MaxBound(max) => write!(f, "max {}", max),
            NumBound::Set(subsets) => write!(f, "{}", subsets),
        }
    }
}