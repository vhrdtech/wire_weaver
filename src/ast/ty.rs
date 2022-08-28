use crate::span::Span;

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