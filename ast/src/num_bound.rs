use std::fmt::{Display, Formatter};
use util::color;
use crate::{Set, TryEvaluateInto, VecExpr};
use crate::lit::NumberLit;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumBound {
    Unbound,
    MinBound(Box<NumberLit>),
    // TODO: print warning if numbound is placed on this number, unit expr is fine though?
    MaxBound(Box<NumberLit>),
    Set(TryEvaluateInto<VecExpr, Set>),
}

impl Display for NumBound {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", color::BLUE)?;
        match self {
            NumBound::Unbound => write!(f, "@{{?}}"),
            NumBound::MinBound(min) => write!(f, "@{{min {}}}", min),
            NumBound::MaxBound(max) => write!(f, "@{{max {}}}", max),
            NumBound::Set(subsets) => write!(f, "@{{ {} }}", subsets),
        }?;
        write!(f, "{}", color::DEFAULT)
    }
}