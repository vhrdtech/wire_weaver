use crate::ast::def_const::DefConst;
use crate::ast::def_fn::DefFn;
use super::prelude::*;
use crate::ast::def_type_alias::DefTypeAlias;
use crate::ast::def_xpi_block::DefXpiBlock;
use crate::error::ParseErrorSource;
use super::def_enum::DefEnum;

#[derive(Debug)]
pub enum Definition<'i> {
    Const(DefConst),
    Enum(DefEnum<'i>),
    Struct(),
    Function(DefFn<'i>),
    TypeAlias(DefTypeAlias<'i>),
    XpiBlock(DefXpiBlock<'i>),
}

impl<'i> Parse<'i> for Definition<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // crate::util::pest_print_tree(input.pairs.clone());
        let rule = match input.pairs.peek() {
            Some(r) => r,
            None => {
                println!("Item::parse: None");
                return Err(ParseErrorSource::internal(""));
            }
        };
        match rule.as_rule() {
            Rule::enum_def => {
                input.parse().map(|enum_def| Definition::Enum(enum_def))
            }
            Rule::type_alias_def => {
                input.parse()
                    .map(|item_type_alias| Definition::TypeAlias(item_type_alias))
            }
            Rule::xpi_block => {
                input.parse()
                    .map(|item_xpi_block| Definition::XpiBlock(item_xpi_block))
            }
            Rule::def_fn => {
                input.parse().map(|def_fn| Definition::Function(def_fn))
            }
            _ => {
                // input.errors.push(ParseError::E0001);
                Err(ParseErrorSource::internal(""))
            }
        }
    }
}






