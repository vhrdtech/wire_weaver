use crate::lit::{DiscreteLit, FixedLit, FloatLit};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscreteRange {
    pub start: DiscreteLit,
    pub end: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedRange {
    pub start: FixedLit,
    pub end: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloatingRange {
    pub start: FloatLit,
    pub end: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharRange {
    pub start: char,
    pub end: char,
}