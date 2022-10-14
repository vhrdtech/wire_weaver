use std::fmt::{Display, Formatter};
use crate::{DiscreteTy, FixedTy, Identifier, Span, Ty};
use crate::ty::FloatTy;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lit {
    pub kind: LitKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LitKind {
    Bool(bool),
    Discrete(DiscreteLit),
    Fixed(FixedLit),
    Float(FloatLit),
    Char(char),
    String(String),
    Tuple(Vec<Lit>),
    Struct(StructLit),
    Enum(EnumLit),
    Array(ArrayLit),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscreteLit {
    pub val: u128,
    pub ty: DiscreteTy,
    /// true if provided by user, false if auto derived
    pub is_ty_forced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedLit {
    pub val: u128,
    pub ty: FixedTy,
    /// true if provided by user, false if auto derived
    pub is_ty_forced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloatLit {
    pub digits: String,
    pub ty: FloatTy,
    /// true if provided by user, false if auto derived
    pub is_ty_forced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructLit {
    pub typename: Identifier,
    pub items: Vec<StructLitItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructLitItem {
    pub name: Identifier,
    pub val: Lit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumLit {
    pub typename: Identifier,
    pub variant: Identifier,
    pub val: Option<EnumLitValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnumLitValue {
    Tuple(Vec<Lit>),
    Struct(Vec<StructLitItem>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArrayLit {
    Init {
        size: Box<Lit>,
        val: Box<Lit>,
    },
    List(Vec<Lit>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecLit(pub Vec<Lit>);

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            LitKind::Bool(val) => write!(f, "{}", val),
            LitKind::UDec { val, .. } => write!(f, "{}", val),
            LitKind::Float32(val) => write!(f, "{}f32", val),
            LitKind::Float64(val) => write!(f, "{}f64", val),
            LitKind::Char(c) => write!(f, "'{}'", c),
            LitKind::String(s) => write!(f, "\"{}\"", s),
        }
    }
}

impl Display for VecLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|lit| write!(f, "{}, ", lit))
    }
}
