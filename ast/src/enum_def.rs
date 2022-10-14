use crate::{Doc, Attrs, Identifier, Span, Ty, Lit};
use crate::struct_def::StructField;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub typename: Identifier,
    pub items: Vec<EnumItem>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumItem {
    pub doc: Doc,
    pub attrs: Attrs,
    pub name: Identifier,
    pub kind: Option<EnumItemKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnumItemKind {
    Tuple(Vec<Ty>),
    Struct(Vec<StructField>),
    Discriminant(Lit),
}