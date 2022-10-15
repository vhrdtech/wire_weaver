use std::fmt::{Debug, Display, Formatter};
use crate::{NumBound, Expr, FnArguments, Generics, Identifier, AutoNumber, Span};

#[derive(Clone, Eq, PartialEq)]
pub struct Ty {
    pub kind: TyKind,
    pub span: Span,
}

impl Ty {
    pub fn new(kind: TyKind) -> Self {
        Ty {
            kind,
            span: Span::call_site()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TyKind {
    Unit,
    Boolean,
    Discrete(DiscreteTy),
    // DiscreteGeneric(Generics),
    Fixed(FixedTy),
    // FixedGeneric(Generics),
    Float(FloatTy),
    // FloatGeneric(Generics),
    Array {
        ty: Box<Ty>,
        len_bound: NumBound,
    },
    Tuple {
        types: Vec<Ty>,
    },
    Fn {
        args: FnArguments,
        ret_ty: Box<Ty>,
    },
    AutoNumber(AutoNumber),
    IndexTyOf(Expr),
    Generic {
        id: Identifier,
        params: Generics,
    },
    Char,
    String {
        len_bound: NumBound,
    },
    UserDefined(Identifier),
    Derive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscreteTy {
    pub is_signed: bool,
    pub bits: u32,
    pub num_bound: NumBound,
    pub unit: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedTy {
    pub is_signed: bool,
    pub m: u32,
    pub n: u32,
    pub num_bound: NumBound,
    pub unit: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloatTy {
    pub bits: u32,
    pub num_bound: NumBound,
    pub unit: (),
}

impl DiscreteTy {
    pub fn is_standard(&self) -> bool {
        if self.num_bound == NumBound::Unbound {
            [8, 16, 32, 64, 128].contains(&self.bits)
        } else {
            false
        }
    }
}

impl Ty {}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TyKind::Unit => write!(f, "()"),
            TyKind::Boolean => write!(f, "bool"),
            TyKind::Discrete(d) => write!(f, "{}", d),
            // TyKind::DiscreteGeneric(g) => write!(f, "ds{}", g),
            TyKind::Fixed(fixed) => write!(f, "{}", fixed),
            // TyKind::FixedGeneric(g) => write!(f, "fx{}", g),
            TyKind::Float(fl) => write!(f, "{:?}", fl),
            // TyKind::FloatGeneric(g) => write!(f, "f{}", g),
            TyKind::Array { ty, len_bound } => write!(f, "[{}; {}]", ty, len_bound),
            TyKind::Tuple { types } => write!(f, "({:?})", types),
            TyKind::Fn { args, ret_ty } => write!(f, "fn({}) -> {}", args, ret_ty),
            TyKind::AutoNumber(a) => write!(f, "{}", a),
            TyKind::IndexTyOf(expr) => write!(f, "index_ty_of<{}>", expr),
            TyKind::Generic { id, params } => write!(f, "{}{}", id, params),
            TyKind::Char => write!(f, "char"),
            TyKind::String { len_bound } => {
                if *len_bound == NumBound::Unbound {
                    write!(f, "str")
                } else {
                    write!(f, "str<{}>", len_bound)
                }
            },
            TyKind::UserDefined(id) => write!(f, "{}", id),
            TyKind::Derive => write!(f, "_"),
        }
    }
}

impl Display for DiscreteTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed { 'i' } else { 'u' };
        write!(f, "{}{}", sym, self.bits)
    }
}

impl Display for FixedTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed { "iq" } else { "uq" };
        write!(f, "{}<{}, {}>", sym, self.m, self.n)
    }
}

impl Display for FloatTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "f{}", self.bits)
    }
}

impl Debug for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}