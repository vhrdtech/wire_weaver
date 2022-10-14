use crate::expr::Expr;
use crate::Ty;
use parser::ast::generics::{GenericParam as GenericParamParser, Generics as GenericsParser};
use std::fmt::{Debug, Display, Formatter};

#[derive(Clone, Eq, PartialEq)]
pub struct Generics {
    pub params: Vec<GenericParam>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenericParam {
    Ty(Ty),
    Expr(Expr),
}

impl<'i> From<GenericsParser<'i>> for Generics {
    fn from(g: GenericsParser<'i>) -> Self {
        Generics {
            params: g.0.iter().map(|p| p.clone().into()).collect(),
        }
    }
}

impl Display for Generics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "<")?;
        itertools::intersperse(
            self.params.iter().map(|param| match param {
                GenericParam::Ty(ty) => format!(f, "{}", ty),
                GenericParam::Expr(expr) => format!(f, "{}", expr),
            }),
            ", ".to_owned(),
        )
            .try_for_each(|s| write!(f, "{}", s))?;
        write!(f, ">")
    }
}

impl Debug for Generics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
