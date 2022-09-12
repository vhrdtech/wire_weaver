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

impl<'i> From<StructDefParser<'i>> for StructDef {
    fn from(sd: StructDefParser<'i>) -> Self {
        StructDef {
            doc: sd.doc.into(),
            typename: sd.typename.into(),
            fields: sd.fields.fields.iter().map(|sf| sf.clone().into()).collect(),
            span: sd.span.into(),
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
        for f in &self.fields {
            if !f.ty.is_sized() {
                return false;
            }
        }
        true
    }
}
