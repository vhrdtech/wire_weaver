#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangeSet {
    Discrete,
    FixedPoint,
    FloatingPoint,
    Char(char, char)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Set {

}