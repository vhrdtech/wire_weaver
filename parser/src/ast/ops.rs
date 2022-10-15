use super::prelude::*;

pub struct BinaryOpParse(pub ast::ops::BinaryOp);

pub struct UnaryOpParse(pub ast::ops::UnaryOp);

pub fn binary_from_rule(rule: Rule) -> Result<ast::ops::BinaryOp, ParseErrorSource> {
    use ast::ops::BinaryOp::*;
    match rule {
        Rule::op_plus => Ok(Plus),
        Rule::op_minus => Ok(Minus),
        Rule::op_mul => Ok(Mul),
        Rule::op_div => Ok(Div),
        Rule::op_rem => Ok(Rem),
        Rule::op_bool_and => Ok(BoolAnd),
        Rule::op_bit_and => Ok(BitAnd),
        Rule::op_bool_or => Ok(BoolOr),
        Rule::op_bit_or => Ok(BitOr),
        Rule::op_xor => Ok(Xor),
        Rule::op_lsh => Ok(Lsh),
        Rule::op_rsh => Ok(Rsh),
        Rule::op_closed_range => Ok(ClosedRange),
        Rule::op_open_range => Ok(OpenRange),
        Rule::op_dot => Ok(Dot),
        Rule::op_eq => Ok(Eq),
        Rule::op_neq => Ok(Neq),
        Rule::op_gte => Ok(Gte),
        Rule::op_gt => Ok(Gt),
        Rule::op_lte => Ok(Lte),
        Rule::op_lt => Ok(Lt),
        Rule::op_path => Ok(Path),
        _ => Err(ParseErrorSource::internal("expected op_binary")),
    }
}

pub fn unary_from_rule(rule: Rule) -> Result<ast::ops::UnaryOp, ParseErrorSource> {
    use ast::ops::UnaryOp::*;
    match rule {
        Rule::op_minus => Ok(Minus),
        Rule::op_plus => Ok(Plus),
        Rule::op_not => Ok(Not),
        _ => Err(ParseErrorSource::internal("expected op_unary rule")),
    }
}

impl<'i> Parse<'i> for BinaryOpParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let op = input.expect1(Rule::op_binary)?;
        Ok(BinaryOpParse(binary_from_rule(
            op.into_inner()
                .next()
                .ok_or_else(|| ParseErrorSource::internal("wrong op_binary rule"))?
                .as_rule(),
        )?))
    }
}

impl<'i> Parse<'i> for UnaryOpParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let op = input.expect1(Rule::op_unary)?;
        Ok(UnaryOpParse(unary_from_rule(
            op.into_inner()
                .next()
                .ok_or_else(|| ParseErrorSource::internal("wrong op_unary rule"))?
                .as_rule(),
        )?))
    }
}