use super::prelude::*;

#[derive(Debug, Eq, PartialEq)]
pub enum ItemOp {
    Minus,
    Plus,
    Negate,
    Mul,
    Div,
    Rem,
    BoolAnd,
    BitAnd,
    BoolOr,
    BitOr,
    Xor,
    Lsh,
    Rsh,
    ClosedRange,
    OpenRange,
    Dot
}

impl ItemOp {
    pub fn from_str(s: &str) -> Self {
        use ItemOp::*;
        match s {
            "-" => Minus,
            "+" => Plus,
            "!" => Negate,
            "*" => Mul,
            "/" => Div,
            "%" => Rem,
            "&&" => BoolAnd,
            "||" => BoolOr,
            "&" => BitAnd,
            "|" => BitOr,
            "^" => Xor,
            "<<" => Lsh,
            ">>" => Rsh,
            "..=" => ClosedRange,
            ".." => OpenRange,
            "." => Dot,
            _ => unreachable!()
        }
    }

    pub fn is_range(&self) -> bool {
        *self == ItemOp::ClosedRange || *self == ItemOp::OpenRange
    }
}

impl<'i> Parse<'i> for ItemOp {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.next() {
            Some(op) => {
                if op.as_rule() == Rule::unary_op || op.as_rule() == Rule::binary_op {
                    Ok(ItemOp::from_str(op.as_str()))
                } else {
                    Err(ParseErrorSource::InternalError)
                }
            },
            None => {
                Err(ParseErrorSource::InternalError)
            }
        }
    }
}