use std::fmt::{Display, Formatter};
use crate::ast::expr::{TryEvaluateInto, VecExpr};
use crate::set::Set;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumBound {
    Unbound,
    MaxBound(usize),
    Set(TryEvaluateInto<VecExpr, Set>),
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