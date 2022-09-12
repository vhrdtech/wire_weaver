use std::fmt::{Display, Formatter};
use parser::ast::ty::Ty as TyParser;
use parser::ast::ty::TyKind as TyKindParser;
use parser::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ty {
    pub kind: TyKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TyKind {
    Unit,
    Boolean,
    Discrete(DiscreteTy),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscreteTy {
    pub is_signed: bool,
    pub bits: u32,
    pub shift: u128,
}

impl DiscreteTy {
    pub fn is_standard(&self) -> bool {
        if self.shift != 0 {
            false
        } else {
            [8, 16, 32, 64, 128].contains(&self.bits)
        }
    }
}

impl<'i> From<TyParser<'i>> for Ty {
    fn from(ty: TyParser<'i>) -> Self {
        Ty {
            kind: ty.kind.into(),
            span: ty.span.into(),
        }
    }
}

impl<'i> From<TyKindParser<'i>> for TyKind {
    fn from(kind: TyKindParser<'i>) -> Self {
        match kind {
            TyKindParser::Boolean => TyKind::Boolean,
            TyKindParser::Discrete {
                is_signed,
                bits,
                shift,
            } => TyKind::Discrete(DiscreteTy {
                is_signed,
                bits,
                shift,
            }),
            _ => todo!(), // TyKindParser::FixedPoint { is_signed, m, n, shift } => TyKind::
                          // TyKindParser::FloatingPoint { bits } => {}
                          // TyKindParser::Array { ty, num_bound } => {}
                          // TyKindParser::Tuple(_) => {}
                          // TyKindParser::Fn { arguments, ret_ty } => {}
                          // TyKindParser::AutoNumber(_) => {}
                          // TyKindParser::IndexOf(_) => {}
                          // TyKindParser::Generic { name, params } => {}
                          // TyKindParser::Char => {}
                          // TyKindParser::String => {}
                          // TyKindParser::Sequence => {}
                          // TyKindParser::UserDefined(_) => {}
                          // TyKindParser::Derive => {}
        }
    }
}

impl Ty {
    pub fn is_sized(&self) -> bool {
        match self.kind {
            TyKind::Unit => true,
            TyKind::Boolean => true,
            TyKind::Discrete(_) => true,
        }
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TyKind::Unit => write!(f, "()"),
            TyKind::Boolean => write!(f, "bool"),
            TyKind::Discrete(d) => write!(f, "{}", d),
        }
    }
}

impl Display for DiscreteTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed {
            'i'
        } else {
            'u'
        };
        if self.shift == 0 {
            write!(f, "{}{}", sym, self.bits)
        } else {
            write!(f, "{}{}{{{}}}", sym, self.bits, self.shift)
        }
    }
}