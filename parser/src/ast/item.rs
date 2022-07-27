use crate::ast::item_type_alias::ItemTypeAlias;
use crate::ast::item_xpi_block::ItemXpiBlock;
use crate::error::ParseErrorSource;
use super::prelude::*;
use super::item_enum::ItemEnum;

#[derive(Debug)]
pub enum Item<'i> {
    Const(ItemConst),
    Enum(ItemEnum<'i>),
    TypeAlias(ItemTypeAlias<'i>),
    XpiBlock(ItemXpiBlock<'i>),
}

impl<'i> Parse<'i> for Item<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        crate::util::pest_print_tree(input.pairs.clone());
        let rule = match input.pairs.peek() {
            Some(r) => r,
            None => {
                println!("Item::parse: None");
                return Err(ParseErrorSource::InternalError);
            }
        };
        let rule_copy = rule.clone();
        match rule.as_rule() {
            Rule::enum_def => {
                todo!("update parser");
                let _ = input.pairs.next();
                ParseInput::fork(rule, input).parse()
                    .map(|item_enum| Item::Enum(item_enum))
            }
            Rule::type_alias_def => {
                todo!("update parser");
                let _ = input.pairs.next();
                ParseInput::fork(rule, input).parse()
                    .map(|item_type_alias| Item::TypeAlias(item_type_alias))
            }
            Rule::xpi_block => {
                input.parse()
                    .map(|item_xpi_block| Item::XpiBlock(item_xpi_block))
            }
            _ => {
                // input.errors.push(ParseError::E0001);
                Err(ParseErrorSource::InternalError)
            }
        }
    }
}

#[derive(Debug)]
pub struct ItemConst {

}




