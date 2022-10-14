use std::fmt::{Debug, Display, Formatter};
use parser::ast::generics::{Generics as GenericsParser, GenericParam as GenericParamParser};
use crate::expr::Expr;
use crate::Ty;

#[derive(Clone, Eq, PartialEq)]
pub struct Generics {
    pub params: Vec<GenericParam>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenericParam {
    Ty(Ty),
    Expr(Expr)
}

impl<'i> From<GenericsParser<'i>> for Generics {
    fn from(g: GenericsParser<'i>) -> Self {
        Generics {
            params: g.0.iter().map(|p| p.clone().into()).collect()
        }
    }
}

impl Display for Generics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "<")?;
        self.params.iter().try_for_each(|p| {
            match p {
                GenericParam::Ty(ty) => write!(f, "{}", ty),
                GenericParam::Expr(expr) => write!(f, "{}", expr),
            }
        })?;
        write!(f, ">")
    }
}

impl Debug for Generics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}