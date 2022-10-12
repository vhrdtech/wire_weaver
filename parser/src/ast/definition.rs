use crate::ast::def_type_alias::TypeAliasDef;
use super::prelude::*;
use crate::error::ParseErrorSource;

#[derive(Debug, Clone)]
pub struct Definition(pub ast::Definition);

impl<'i> Parse<'i> for Definition {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // crate::util::pest_print_tree(input.pairs.clone());
        let rule = match input.pairs.peek() {
            Some(r) => r,
            None => {
                return Err(ParseErrorSource::internal("Item::parse: expected input"));
            }
        };
        let ast_def = match rule.as_rule() {
            // Rule::enum_def => input.parse().map(|enum_def| Definition::Enum(enum_def)),
            // Rule::struct_def => input
            //     .parse()
            //     .map(|struct_def| Definition::Struct(struct_def)),
            Rule::type_alias_def => {
                let ty_alias: TypeAliasDef = input.parse()?;
                ast::Definition::TypeAlias(ty_alias.0)
            },
            // Rule::xpi_block => input
            //     .parse()
            //     .map(|item_xpi_block| Definition::XpiBlock(item_xpi_block)),
            // Rule::def_fn => input.parse().map(|def_fn| Definition::Function(def_fn)),
            _ => {
                return Err(ParseErrorSource::internal("unexpected definition"));
            },
        };
        Ok(Definition(ast_def))
    }
}
