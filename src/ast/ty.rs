use crate::span::Span;
use parser::ast::ty::Ty as TyParser;
use parser::ast::ty::TyKind as TyKindParser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ty {
    kind: TyKind,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TyKind {
    Boolean,
    Discrete {
        is_signed: bool,
        bits: u32,
        shift: u128,
    },
}

impl<'i> From<TyParser<'i>> for Ty {
    fn from(ty: TyParser<'i>) -> Self {
        Ty {
            kind: ty.kind.into(),
            span: ty.span.into()
        }
    }
}

impl<'i> From<TyKindParser<'i>> for TyKind {
    fn from(kind: TyKindParser<'i>) -> Self {
        match kind {
            TyKindParser::Boolean => TyKind::Boolean,
            TyKindParser::Discrete { is_signed, bits, shift } => TyKind::Discrete { is_signed, bits, shift },
            _ => todo!()
            // TyKindParser::FixedPoint { is_signed, m, n, shift } => TyKind::
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