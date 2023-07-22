use crate::{Attrs, AutoNumber, Expr, FnArguments, Generics, NumBound, Path, Span};
use std::fmt::{Debug, Display, Formatter};

#[derive(Clone, Eq, PartialEq)]
pub struct Ty {
    pub attrs: Option<Attrs>,
    pub kind: TyKind,
    pub span: Span,
}

impl Ty {
    pub fn new(kind: TyKind) -> Self {
        Ty {
            attrs: None,
            kind,
            span: Span::call_site(),
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
    Array { ty: Box<Ty>, len_bound: NumBound },
    Tuple { types: Vec<Ty> },
    Fn { args: FnArguments, ret_ty: Box<Ty> },
    AutoNumber(AutoNumber),
    IndexTyOf(Expr),
    Generic { path: Path, params: Generics },
    Char,
    String { len_bound: NumBound },
    Ref(Path),
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

#[derive(Default)]
pub struct TyTraits {
    pub is_copy: bool,
    pub is_clone: bool,
    pub is_eq: bool,
    pub is_partial_eq: bool,
}

impl TyTraits {
    pub fn ccep_true() -> Self {
        TyTraits {
            is_copy: true,
            is_clone: true,
            is_eq: true,
            is_partial_eq: true,
        }
    }
}

impl core::ops::BitAnd for TyTraits {
    type Output = TyTraits;

    fn bitand(self, rhs: Self) -> Self::Output {
        TyTraits {
            is_copy: self.is_copy & rhs.is_copy,
            is_clone: self.is_clone & rhs.is_clone,
            is_eq: self.is_eq & rhs.is_eq,
            is_partial_eq: self.is_partial_eq & rhs.is_partial_eq,
        }
    }
}

impl Ty {
    pub fn ty_traits(&self) -> TyTraits {
        use TyKind::*;
        match &self.kind {
            Unit | Boolean => TyTraits::ccep_true(),
            Discrete(_) | Fixed(_) => TyTraits::ccep_true(),
            Float(_) => TyTraits {
                is_copy: true,
                is_clone: true,
                is_eq: false,
                is_partial_eq: true,
            },
            Array { ty, .. } => ty.ty_traits(),
            Tuple { types } => {
                let mut traits = TyTraits::ccep_true();
                for ty in types {
                    traits = traits & ty.ty_traits();
                }
                traits
            }
            Char => TyTraits::ccep_true(),
            String { .. } => TyTraits {
                is_copy: false,
                is_clone: true,
                is_eq: true,
                is_partial_eq: true,
            },
            Ref(_) => todo!("process AST and pre resolve ty traits for refs"),
            Fn { .. } => unimplemented!(),
            AutoNumber(_) | IndexTyOf(_) | Generic { .. } | Derive => {
                panic!("Ty::ty_traits() called on unprocessed AST")
            }
        }
    }
}

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
            TyKind::Generic { path, params } => write!(f, "{}{}", path, params),
            TyKind::Char => write!(f, "char"),
            TyKind::String { len_bound } => {
                if *len_bound == NumBound::Unbound {
                    write!(f, "str")
                } else {
                    write!(f, "str<{}>", len_bound)
                }
            }
            TyKind::Ref(path) => write!(f, "{}", path),
            TyKind::Derive => write!(f, "_"),
        }
    }
}

impl Display for DiscreteTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed { 'i' } else { 'u' };
        write!(f, "{}{}", sym, self.bits)?;
        if self.num_bound != NumBound::Unbound {
            write!(f, " {}", self.num_bound)?;
        }
        Ok(())
    }
}

impl Display for FixedTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let sym = if self.is_signed { "iq" } else { "uq" };
        write!(f, "{}<{}, {}>", sym, self.m, self.n)?;
        if self.num_bound != NumBound::Unbound {
            write!(f, " {}", self.num_bound)?;
        }
        Ok(())
    }
}

impl Display for FloatTy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "f{}", self.bits)?;
        if self.num_bound != NumBound::Unbound {
            write!(f, " {}", self.num_bound)?;
        }
        Ok(())
    }
}

impl Debug for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
