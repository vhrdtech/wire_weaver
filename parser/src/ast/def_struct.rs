use crate::ast::naming::StructFieldName;
use crate::ast::prelude::*;
use crate::ast::ty::Ty;

#[derive(Debug)]
pub struct DefStruct<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Typename<'i>,
    pub fields: StructFields<'i>
}

#[derive(Debug)]
pub struct StructFields<'i> {
    pub fields: Vec<StructField<'i>>,
}

#[derive(Debug)]
pub struct StructField<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub name: StructFieldName<'i>,
    pub ty: Ty<'i>
}

impl<'i> Parse<'i> for DefStruct<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::struct_def)?, input);

        Ok(DefStruct {
            docs: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            fields: input.parse()?
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
        Ok(StructField {
            docs: input.parse()?,
            attrs: input.parse()?,
            name: input.parse()?,
            ty: input.parse()?
        })
    }
}