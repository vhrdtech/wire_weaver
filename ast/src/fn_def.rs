use std::fmt::{Debug, Display, Formatter};
use crate::{Attrs, Doc, Generics, Identifier, Stmt, Ty};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FnDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub name: Identifier,
    pub generics: Option<Generics>,
    pub arguments: FnArguments,
    pub ret_ty: Option<Ty>,
    pub statements: Vec<Stmt>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct FnArguments {
    pub args: Vec<FnArg>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FnArg {
    pub name: Identifier,
    pub ty: Ty,
}

impl Display for FnArguments {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        itertools::intersperse(
            self.args.iter().map(|arg| format!("{}: {}", arg.name, arg.ty)),
            ", ".to_owned(),
        ).try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}

impl Debug for FnArguments {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}