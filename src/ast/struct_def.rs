use crate::ast::doc::Doc;
use crate::ast::identifier::Identifier;
use crate::ast::ty::Ty;
use parser::ast::def_struct::{
    DefStruct as StructDefParser, StructField as StructFieldParser,
    StructFields as StructFieldsParser,
};
use parser::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDef {
    pub doc: Doc,
    pub typename: Identifier,
    pub fields: StructFields,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructFields {
    pub fields: Vec<StructField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructField {
    pub doc: Doc,
    pub name: Identifier,
    pub ty: Ty,
    pub span: Span,
}

impl<'i> From<StructDefParser<'i>> for StructDef {
    fn from(sd: StructDefParser<'i>) -> Self {
        StructDef {
            doc: sd.doc.into(),
            typename: sd.typename.into(),
            fields: sd.fields.into(),
            span: sd.span.into(),
        }
    }
}

impl<'i> From<StructFieldsParser<'i>> for StructFields {
    fn from(sfs: StructFieldsParser<'i>) -> Self {
        StructFields {
            fields: sfs.fields.iter().map(|sf| sf.clone().into()).collect(),
        }
    }
}

impl<'i> From<StructFieldParser<'i>> for StructField {
    fn from(sf: StructFieldParser<'i>) -> Self {
        StructField {
            doc: sf.doc.into(),
            name: sf.name.into(),
            ty: sf.ty.into(),
            span: sf.span.into(),
        }
    }
}

impl StructDef {
    pub fn is_sized(&self) -> bool {
        for f in &self.fields.fields {
            if !f.ty.is_sized() {
                return false;
            }
        }
        true
    }
}
