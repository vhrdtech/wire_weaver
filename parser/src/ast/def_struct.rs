use crate::ast::prelude::*;
use crate::ast::ty::TyParse;
use ast::struct_def::StructField;
use ast::StructDef;

pub struct StructDefParse(pub StructDef);

pub struct StructFieldsParse(pub Vec<StructField>);

impl<'i> Parse<'i> for StructDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::struct_def, "StructDefParse")?, input);
        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let typename: IdentifierParse<identifier::EnumTyName> = input.parse()?;
        let fields: StructFieldsParse = input.parse()?;
        Ok(StructDefParse(StructDef {
            doc: doc.0,
            attrs: attrs.0,
            typename: typename.0,
            fields: fields.0,
            span: input.span.clone(),
        }))
    }
}

impl<'i> Parse<'i> for StructFieldsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut fields = Vec::new();
        while input.pairs.peek().is_some() {
            let mut input = ParseInput::fork(
                input.expect1(Rule::struct_field, "StructFieldsParse")?,
                input,
            );
            let doc: DocParse = input.parse()?;
            let attrs: AttrsParse = input.parse()?;
            let name: IdentifierParse<identifier::StructFieldName> = input.parse()?;
            let mut ty: TyParse = input.parse()?;
            ty.0.attrs = Some(attrs.0.clone());
            fields.push(StructField {
                doc: doc.0,
                attrs: attrs.0,
                name: name.0,
                ty: ty.0,
                span: input.span.clone(),
            });
        }

        Ok(StructFieldsParse(fields))
    }
}
