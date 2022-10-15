use ast::struct_def::StructField;
use ast::StructDef;
use crate::ast::prelude::*;
use crate::ast::ty::TyParse;

pub struct StructDefParse(pub StructDef);

pub struct StructFieldsParse(pub Vec<StructField>);

impl<'i> Parse<'i> for StructDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::struct_def)?, input);
        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let typename: IdentifierParse<identifier::EnumTyName> = input.parse()?;
        let fields: StructFieldsParse = input.parse()?;
        Ok(StructDefParse(StructDef {
            doc: doc.0,
            attrs: attrs.0,
            typename: typename.0,
            fields: fields.0,
            span: ast_span_from_pest(input.span.clone()),
        }))
    }
}

impl<'i> Parse<'i> for StructFieldsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut fields = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let mut input = ParseInput::fork(input.expect1(Rule::struct_field)?, input);
            let doc: DocParse = input.parse()?;
            let attrs: AttrsParse = input.parse()?;
            let name: IdentifierParse<identifier::StructFieldName> = input.parse()?;
            let ty: TyParse = input.parse()?;
            fields.push(StructField {
                doc: doc.0,
                attrs: attrs.0,
                name: name.0,
                ty: ty.0,
                span: ast_span_from_pest(input.span.clone()),
            });
        }

        Ok(StructFieldsParse(fields))
    }
}