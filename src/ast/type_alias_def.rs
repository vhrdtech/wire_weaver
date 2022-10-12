use std::convert::{TryFrom, TryInto};
use crate::ast::{Doc, Attrs, Identifier, Ty};
use parser::ast::def_type_alias::DefTypeAlias as TypeAliasDefParser;
use crate::error::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct TypeAliasDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub typename: Identifier,
    pub ty: Ty,
}

impl<'i> TryFrom<TypeAliasDefParser<'i>> for TypeAliasDef {
    type Error = Error;

    fn try_from(a: TypeAliasDefParser<'i>) -> Result<Self, Self::Error> {
        Ok(TypeAliasDef {
            doc: a.doc.into(),
            attrs: a.attrs.try_into()?,
            typename: a.typename.into(),
            ty: a.r#type.into(),
        })
    }
}