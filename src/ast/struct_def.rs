use crate::ast::doc::Doc;
use crate::ast::identifier::Identifier;
use crate::ast::ty::Ty;
use crate::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDef {
    pub doc: Doc,
    pub typename: Identifier,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructField {
    pub doc: Doc,
    pub name: Identifier,
    pub ty: Ty,
    pub span: Span,
}