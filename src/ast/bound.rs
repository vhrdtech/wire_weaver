use std::fmt::{Display, Formatter};
use crate::ast::expr::{TryEvaluateInto, VecExpr};
use crate::ast::lit::VecLit;
use parser::ast::num_bound::NumBound as NumBoundParser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumBound {
    Unbound,
    MaxBound(u128),
    Set(TryEvaluateInto<VecExpr, VecLit>)
}

impl<'i> From<NumBoundParser<'i>> for NumBound {
    fn from(n: NumBoundParser<'i>) -> Self {
        match n {
            NumBoundParser::Unbound => NumBound::Unbound,
            NumBoundParser::MaxBound(max) => NumBound::MaxBound(max),
            NumBoundParser::Set(_) => unimplemented!()
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