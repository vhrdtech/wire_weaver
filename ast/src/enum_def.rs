use std::convert::{TryFrom, TryInto};
use parser::span::Span;
use crate::ast::{Doc, Attrs, Identifier, Ty, Lit};
use crate::ast::struct_def::StructField;
use parser::ast::def_enum::{
    DefEnum as EnumDefParser,
    EnumItem as EnumItemParser,
    EnumItemKind as EnumItemKindParser,
};
use crate::error::Error;

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

impl<'i> From<EnumItemKindParser<'i>> for EnumItemKind {
    fn from(kind: EnumItemKindParser<'i>) -> Self {
        match kind {
            EnumItemKindParser::Tuple(tys) => EnumItemKind::Tuple(tys.fields.iter().map(|ty| ty.clone().into()).collect()),
            EnumItemKindParser::Struct => todo!(),
            EnumItemKindParser::Discriminant(_) => todo!(),
        }
    }
}

impl<'i> TryFrom<EnumItemParser<'i>> for EnumItem {
    type Error = Error;

    fn try_from(item: EnumItemParser<'i>) -> Result<Self, Error> {
        Ok(EnumItem {
            doc: item.docs.into(),
            attrs: item.attrs.try_into()?,
            name: item.name.into(),
            kind: item.kind.map(|item| item.into()),
        })
    }
}

impl<'i> TryFrom<EnumDefParser<'i>> for EnumDef {
    type Error = Error;

    fn try_from(def: EnumDefParser<'i>) -> Result<Self, Self::Error> {
        Ok(EnumDef {
            doc: def.docs.into(),
            attrs: def.attrs.try_into()?,
            typename: def.typename.into(),
            items: def.entries.entries.iter().try_fold(vec![], |mut items, item| {
                items.push(item.clone().try_into()?);
                Ok(items)
            })?,
            span: Span::call_site(),
        })
    }
}