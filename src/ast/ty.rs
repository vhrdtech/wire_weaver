use crate::ast::bound::NumBound;
use crate::ast::expr::Expr;
use crate::ast::fn_def::FnArguments;
use crate::ast::generics::Generics;
use crate::ast::identifier::Identifier;
use crate::ast::number::AutoNumber;
use parser::ast::ty::Ty as TyParser;
use parser::ast::ty::TyKind as TyKindParser;
use parser::span::Span;
use std::fmt::{Display, Formatter};
use std::ops::Deref;

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
    Fixed(FixedTy),
    Float {
        bits: u32,
    },
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
    // TODO: Convert into appropriate type
    IndexTyOf(Expr),
    // TODO: resolve
    Generic {
        // TODO: Change to resolved/not resolved
        id: Identifier,
        params: Generics,
    },
    Char,
    String {
        len_bound: NumBound,
    },
    UserDefined(Identifier),
    // TODO: Change to resolved/not resolved
    Derive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscreteTy {
    pub is_signed: bool,
    pub bits: u32,
    pub shift: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedTy {
    pub is_signed: bool,
    pub m: u32,
    pub n: u32,
    pub shift: i128,
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
            TyKindParser::FixedPoint {
                is_signed,
                m,
                n,
                shift,
            } => TyKind::Fixed(FixedTy {
                is_signed,
                m,
                n,
                shift,
            }),
            TyKindParser::FloatingPoint { bits } => TyKind::Float { bits },
            TyKindParser::Array { ty, num_bound } => TyKind::Array {
                ty: Box::new(ty.deref().clone().into()),
                len_bound: num_bound.into(),
            },
            TyKindParser::Tuple(types) => TyKind::Tuple {
                types: types.iter().map(|t| t.clone().into()).collect(),
            },
            TyKindParser::Fn { arguments, ret_ty } => TyKind::Fn {
                args: arguments.into(),
                ret_ty: ret_ty.map(|t| Box::new(t.0.into())),
            },
            TyKindParser::AutoNumber(au) => TyKind::AutoNumber(au.into()),
            TyKindParser::IndexOf(expr) => TyKind::IndexTyOf(expr.into()),
            TyKindParser::Generic { name, params } => TyKind::Generic {
                id: name.into(),
                params: params.into(),
            },
            TyKindParser::Char => TyKind::Char,
            TyKindParser::String => TyKind::String {
                len_bound: NumBound::Unbound,
            },
            TyKindParser::Sequence => todo!(),
            TyKindParser::UserDefined(id) => TyKind::UserDefined(id.into()),
            TyKindParser::Derive => TyKind::Derive,
        }
    }
}

impl Ty {
    pub fn is_sized(&self) -> bool {
        // match self.kind {
        //     TyKind::Unit => true,
        //     TyKind::Boolean => true,
        //     TyKind::Discrete(_) => true,
        // }
        true
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TyKind::Unit => write!(f, "()"),
            TyKind::Boolean => write!(f, "bool"),
            TyKind::Discrete(d) => write!(f, "{}", d),
            TyKind::Fixed(fixed) => write!(f, "{}", fixed),
            TyKind::Float { bits } => write!(f, "f{}", bits),
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