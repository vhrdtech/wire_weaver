use std::fmt::{Display, Formatter};
use util::color;
use crate::{Set, TryEvaluateInto, VecExpr};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumBound {
    Unbound,
    MaxBound(usize),
    Set(TryEvaluateInto<VecExpr, Set>),
}

impl Display for NumBound {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", color::BLUE)?;
        match self {
            NumBound::Unbound => write!(f, "@{{?}}"),
            NumBound::MaxBound(max) => write!(f, "@{{max {}}}", max),
            NumBound::Set(subsets) => write!(f, "@{{ {} }}", subsets),
        }?;
        write!(f, "{}", color::DEFAULT)
    }
}