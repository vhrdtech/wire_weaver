use crate::ast::bound::NumBound;
use crate::ast::expr::Expr;
use crate::ast::fn_def::FnArguments;
use crate::ast::generics::Generics;
use crate::ast::identifier::Identifier;
use crate::ast::number::AutoNumber;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use crate::{Identifier, Span};
use crate::num_bound::NumBound;

#[derive(Clone, Debug, Eq, PartialEq)]
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
    DiscreteGeneric(Generics),
    Fixed(FixedTy),
    FixedGeneric(Generics),
    Float {
        bits: u32,
    },
    FloatGeneric(Generics),
    Array {
        ty: Box<Ty>,
        len_bound: NumBound,
    },
    Tuple {
        types: Vec<Ty>,
    },
    Fn {
        args: FnArguments,
        ret_ty: Option<Box<Ty>>,
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
    // pub shift: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedTy {
    pub is_signed: bool,
    pub m: u32,
    pub n: u32,
    pub num_bound: NumBound,
    pub unit: (),
    // pub shift: i128,
}

pub struct FloatTy {
    pub bits: u32,
    pub num_bound: NumBound,
    pub unit: (),
}

impl DiscreteTy {
    pub fn is_standard(&self) -> bool {
        if self.shift != 0 { // or numbound present
            false
        } else {
            [8, 16, 32, 64, 128].contains(&self.bits)
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
            TyKind::DiscreteGeneric(g) => write!(f, "ds{}", g),
            TyKind::Fixed(fixed) => write!(f, "{}", fixed),
            TyKind::FixedGeneric(g) => write!(f, "fx{}", g),
            TyKind::Float { bits } => write!(f, "f{}", bits),
            TyKind::FloatGeneric(g) => write!(f, "f{}", g),
            TyKind::Array { ty, len_bound } => write!(f, "[{}; {}]", ty, len_bound),
            TyKind::Tuple { types } => write!(f, "({:?})", types),
            TyKind::Fn { args, ret_ty } => {
                match ret_ty {
                    Some(ret_ty) => {
                        write!(f, "fn({}) -> {}", args, ret_ty)
                    }
                    None => {
                        write!(f, "fn({})", args)
                    }
                }
            }
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
        if self.shift == 0 {
            write!(f, "{}{}", sym, self.bits)
        } else {
            write!(f, "{}{}{{{}}}", sym, self.bits, self.shift)
        }
    }
}

impl Display for FixedTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed { "iq" } else { "uq" };
        if self.shift == 0 {
            write!(f, "{}<{}, {}>", sym, self.m, self.n)
        } else {
            let sign_sym = if self.shift > 0 {
                '+'
            } else {
                '-'
            };
            write!(f, "{}<{}, {}, {}{}>", sym, self.m, self.n, sign_sym, self.shift.abs())
        }
    }
}