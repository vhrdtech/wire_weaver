use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangeSet {
    Discrete,
    FixedPoint,
    FloatingPoint,
    Char(char, char),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Set {}

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