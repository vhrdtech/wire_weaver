use crate::ty::FloatTy;
use crate::{DiscreteTy, FixedTy, Identifier, Path, Span};
use std::fmt::{Debug, Display, Formatter};

#[derive(Clone, Eq, PartialEq)]
pub struct Lit {
    pub kind: LitKind,
    pub span: Span,
}

#[derive(Clone, Eq, PartialEq)]
pub struct NumberLit {
    pub kind: NumberLitKind,
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
pub enum NumberLitKind {
    Discrete(DiscreteLit),
    Fixed(FixedLit),
    Float(FloatLit),
}

impl Lit {
    pub fn is_a_number(&self) -> bool {
        use LitKind::*;
        match self.kind {
            Discrete(_) => true,
            Fixed(_) => true,
            Float(_) => true,
            _ => false,
        }
    }

    pub fn is_same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(&self.kind) == std::mem::discriminant(&other.kind)
    }
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
    pub path: Path,
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
    Init { size: Box<Lit>, val: Box<Lit> },
    List(Vec<Lit>),
}

#[derive(Clone, Eq, PartialEq)]
pub struct VecLit(pub Vec<Lit>);

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            LitKind::Bool(val) => write!(f, "{}", val),
            LitKind::Discrete(ds) => write!(f, "{:?}", ds),
            LitKind::Fixed(fx) => write!(f, "{:?}", fx),
            LitKind::Float(fl) => write!(f, "{:?}", fl),
            LitKind::Char(c) => write!(f, "'{}'", c),
            LitKind::String(s) => write!(f, "\"{}\"", s),
            LitKind::Tuple(t) => write!(f, "{:?}", t),
            LitKind::Struct(s) => write!(f, "{:?}", s),
            LitKind::Enum(e) => write!(f, "{:?}", e),
            LitKind::Array(a) => write!(f, "{:?}", a),
        }
    }
}

impl Display for NumberLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            NumberLitKind::Discrete(ds) => write!(f, "{:?}", ds),
            NumberLitKind::Fixed(fx) => write!(f, "{:?}", fx),
            NumberLitKind::Float(fl) => write!(f, "{:?}", fl),
        }
    }
}

impl Display for VecLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|lit| write!(f, "{}, ", lit))
    }
}

impl Debug for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Debug for NumberLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Debug for VecLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
