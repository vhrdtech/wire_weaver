use crate::{Definition, Expr, Identifier, Ty};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Stmt {
    Let(LetStmt),
    Expr(Expr, bool),
    Def(Definition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LetStmt {
    pub ident: Identifier,
    pub type_ascription: Option<Ty>,
    pub expr: Expr,
}