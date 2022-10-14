use std::fmt::{Display, Formatter};
use crate::range::{CharRange, DiscreteRange, FixedRange, FloatingRange};


#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Set {
    Discrete(Vec<DiscreteRange>),
    Fixed(Vec<FixedRange>),
    Float(Vec<FloatingRange>),
    Char(Vec<CharRange>),
}

impl Set {
    pub fn max_len(&self) -> usize {
        todo!()
    }
}

impl Display for Set {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Set(todo)")
    }
}