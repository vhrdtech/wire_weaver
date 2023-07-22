use crate::{Expr, Ty};
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

impl Display for Generics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "<")?;
        itertools::intersperse(
            self.params.iter().map(|param| match param {
                GenericParam::Ty(ty) => format!("{}", ty),
                GenericParam::Expr(expr) => format!("{}", expr),
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
