use ast::Definition;
use crate::ast::def_enum::EnumDefParse;
use crate::ast::def_fn::FnDefParse;
use crate::ast::def_struct::StructDefParse;
use crate::ast::def_type_alias::TypeAliasDefParse;
use crate::ast::def_xpi_block::XpiDefParse;
use super::prelude::*;
use crate::error::ParseErrorSource;

pub struct DefinitionParse(pub Definition);

impl<'i> Parse<'i> for DefinitionParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // crate::util::pest_print_tree(input.pairs.clone());
        let rule = match input.pairs.peek() {
            Some(r) => r,
            None => {
                return Err(ParseErrorSource::internal("Item::parse: expected input"));
            }
        };
        let ast_def = match rule.as_rule() {
            Rule::enum_def => {
                let enum_def: EnumDefParse = input.parse()?;
                Definition::Enum(enum_def.0)
            },
            Rule::struct_def => {
                let struct_def: StructDefParse = input.parse()?;
                Definition::Struct(struct_def.0)
            }
            Rule::type_alias_def => {
                let ty_alias: TypeAliasDefParse = input.parse()?;
                Definition::TypeAlias(ty_alias.0)
            },
            Rule::xpi_block => {
                let xpi_def: XpiDefParse = input.parse()?;
                Definition::Xpi(xpi_def.0)
            }
            Rule::def_fn => {
                let fn_def: FnDefParse = input.parse()?;
                Definition::Function(fn_def.0)
            }
            _ => {
                return Err(ParseErrorSource::internal("unexpected definition"));
            },
        };
        Ok(DefinitionParse(ast_def))
    }
}
