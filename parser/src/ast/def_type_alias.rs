use super::prelude::*;

#[derive(Debug, Clone)]
pub struct TypeAliasDef(pub ast::TypeAliasDef);

impl<'i> Parse<'i> for TypeAliasDef {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::type_alias_def)?, input);
        // Ok(TypeAliasDef {
        //     doc: input.parse()?,
        //     // attrs: input.parse()?,
        //     typename: input.parse()?,
        //     // r#type: input.parse()?,
        // })
        let doc: Doc = input.parse()?;
        let typename: Identifier<identifier::TyAlias> = input.parse()?;
        Ok(TypeAliasDef(ast::TypeAliasDef {
            doc: doc.0,
            typename: typename.0,
        }))
    }
}
