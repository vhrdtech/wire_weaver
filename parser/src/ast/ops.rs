use super::prelude::*;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum BinaryOp {
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
    Eq,
    Neq,
    Gte,
    Gt,
    Lte,
    Lt,
}

impl BinaryOp {
    pub fn is_range_op(&self) -> bool {
        *self == BinaryOp::ClosedRange || *self == BinaryOp::OpenRange
    }

    pub fn from_rule(rule: Rule) -> Result<Self, ParseErrorSource> {
        match rule {
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
            Rule::op_eq => Ok(BinaryOp::Eq),
            Rule::op_neq => Ok(BinaryOp::Neq),
            Rule::op_gte => Ok(BinaryOp::Gte),
            Rule::op_gt => Ok(BinaryOp::Gt),
            Rule::op_lte => Ok(BinaryOp::Lte),
            Rule::op_lt => Ok(BinaryOp::Lt),
            _ => Err(ParseErrorSource::internal(""))
        }
    }

    pub fn binding_power(&self) -> (u8, u8) {
        use BinaryOp::*;
        match self {
            OpenRange | ClosedRange => (1, 2),
            BoolOr => (3, 4),
            BoolAnd => (5, 6),
            Eq | Neq | Gte | Gt | Lte | Lt => (7, 8),
            BitOr => (9, 10),
            Xor => (11, 12),
            BitAnd => (13, 14),
            Lsh | Rsh => (15, 16),
            Plus | Minus => (17, 18),
            Mul | Div | Rem => (19, 20),
            Dot => (21, 22),
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            BinaryOp::Minus => "-",
            BinaryOp::Plus => "+",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::BoolAnd => "&&",
            BinaryOp::BitAnd => "&",
            BinaryOp::BoolOr => "||",
            BinaryOp::BitOr => "|",
            BinaryOp::Xor => "^",
            BinaryOp::Lsh => "<<",
            BinaryOp::Rsh => ">>",
            BinaryOp::ClosedRange => "..=",
            BinaryOp::OpenRange => "..",
            BinaryOp::Dot => ".",
            BinaryOp::Eq => "==",
            BinaryOp::Neq => "!=",
            BinaryOp::Gte => ">=",
            BinaryOp::Gt => ">",
            BinaryOp::Lte => "<=",
            BinaryOp::Lt => "<",
        }
    }
}

impl<'i> Parse<'i> for BinaryOp {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let op = input.expect1(Rule::op_binary)?;
        Ok(BinaryOp::from_rule(op
            .into_inner()
            .next()
            .ok_or_else(|| ParseErrorSource::internal(""))?
            .as_rule()
        )?)
    }
}

pub enum UnaryOp {
    Minus,
    Plus,
    Not,
}

impl UnaryOp {
    pub fn from_rule(rule: Rule) -> Result<Self, ParseErrorSource> {
        match rule {
            Rule::op_minus => Ok(UnaryOp::Minus),
            Rule::op_plus => Ok(UnaryOp::Plus),
            Rule::op_not => Ok(UnaryOp::Not),
            _ => Err(ParseErrorSource::internal(""))
        }
    }

    pub fn binding_power(&self) -> ((), u8) {
        match self {
            UnaryOp::Plus | UnaryOp::Minus | UnaryOp::Not => ((), 23),
        }
    }
}

impl<'i> Parse<'i> for UnaryOp {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let op = input.expect1(Rule::op_unary)?;
        Ok(UnaryOp::from_rule(op.as_rule())?)
    }
}