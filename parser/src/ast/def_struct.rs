use pest::Span;
use crate::ast::naming::{StructFieldName, StructTyName};
use crate::ast::prelude::*;
use crate::ast::ty::Ty;

#[derive(Debug, Clone)]
pub struct DefStruct<'i> {
    pub doc: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Identifier<'i, StructTyName>,
    pub fields: StructFields<'i>,
    pub span: Span<'i>
}

#[derive(Debug, Clone)]
pub struct StructFields<'i> {
    pub fields: Vec<StructField<'i>>,
}

#[derive(Debug, Clone)]
pub struct StructField<'i> {
    pub doc: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub name: Identifier<'i, StructFieldName>,
    pub ty: Ty<'i>,
    pub span: Span<'i>
}

impl<'i> Parse<'i> for DefStruct<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::struct_def)?, input);

        Ok(DefStruct {
            doc: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            fields: input.parse()?,
            span: input.span
        })
    }
}

impl<'i> Parse<'i> for StructFields<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut fields = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let mut input = ParseInput::fork(input.expect1(Rule::struct_field)?, input);
            fields.push(input.parse()?);
        }

        Ok(StructFields {
            fields
        })
    }
}

impl<'i> Parse<'i> for StructField<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let span = input.span.clone();
        Ok(StructField {
            doc: input.parse()?,
            attrs: input.parse()?,
            name: input.parse()?,
            ty: input.parse()?,
            span
        })
    }
}