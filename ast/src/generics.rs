use std::fmt::{Display, Formatter};
use parser::ast::generics::{Generics as GenericsParser, GenericParam as GenericParamParser};
use crate::ast::expr::Expr;
use crate::ast::ty::Ty;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Generics {
    pub params: Vec<GenericParam>
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

impl<'i> From<GenericParamParser<'i>> for GenericParam {
    fn from(p: GenericParamParser<'i>) -> Self {
        match p {
            GenericParamParser::Ty(ty) => GenericParam::Ty(ty.into()),
            GenericParamParser::Expr(expr) => GenericParam::Expr(expr.into())
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