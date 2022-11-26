use super::prelude::*;
use crate::ast::ty::TyParse;
use ast::TypeAliasDef;

pub struct TypeAliasDefParse(pub TypeAliasDef);

impl<'i> Parse<'i> for TypeAliasDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::type_alias_def, "TypeAliasDefParse")?, input);

        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let typename: IdentifierParse<identifier::TyAlias> = input.parse()?;
        let ty: TyParse = input.parse()?;
        Ok(TypeAliasDefParse(TypeAliasDef {
            doc: doc.0,
            attrs: attrs.0,
            typename: typename.0,
            ty: ty.0,
        }))
    }
}
