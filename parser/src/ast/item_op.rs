use super::prelude::*;

#[derive(Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Negate,
    Minus,
    Plus,
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
    Dot,
    LParen,
    RParen,
}

impl BinaryOp {
    pub fn is_range_op(&self) -> bool {
        *self == BinaryOp::ClosedRange || *self == BinaryOp::OpenRange
    }

    pub fn from_rule(rule: Rule) -> Result<Self, ParseErrorSource> {
        match rule {
            Rule::op_not => Ok(BinaryOp::Negate),
            Rule::op_plus => Ok(BinaryOp::Plus),
            Rule::op_minus => Ok(BinaryOp::Minus),
            Rule::op_mul => Ok(BinaryOp::Mul),
            Rule::op_div => Ok(BinaryOp::Div),
            Rule::op_rem => Ok(BinaryOp::Rem),
            Rule::op_bool_and => Ok(BinaryOp::BoolAnd),
            Rule::op_bit_and => Ok(BinaryOp::BitAnd),
            Rule::op_bool_or => Ok(BinaryOp::BoolOr),
            Rule::op_bit_or => Ok(BinaryOp::BitOr),
            Rule::op_xor => Ok(BinaryOp::Xor),
            Rule::op_lsh => Ok(BinaryOp::Lsh),
            Rule::op_rsh => Ok(BinaryOp::Rsh),
            Rule::op_closed_range => Ok(BinaryOp::ClosedRange),
            Rule::op_open_range => Ok(BinaryOp::OpenRange),
            Rule::op_dot => Ok(BinaryOp::Dot),
            _ => Err(ParseErrorSource::internal())
        }
    }
}

impl<'i> Parse<'i> for BinaryOp {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let op = input.expect1(Rule::op_binary)?;
        Ok(BinaryOp::from_rule(op.as_rule())?)
    }
}