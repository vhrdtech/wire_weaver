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
    Path,
}

impl BinaryOp {
    pub fn is_range_op(&self) -> bool {
        *self == BinaryOp::ClosedRange || *self == BinaryOp::OpenRange
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
            Path => (23, 24),
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
            BinaryOp::Path => "::",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UnaryOp {
    Minus,
    Plus,
    Not,
}

impl UnaryOp {
    pub fn binding_power(&self) -> ((), u8) {
        match self {
            UnaryOp::Plus | UnaryOp::Minus | UnaryOp::Not => ((), 23),
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            UnaryOp::Minus => "-",
            UnaryOp::Plus => "+",
            UnaryOp::Not => "!",
        }
    }
}
