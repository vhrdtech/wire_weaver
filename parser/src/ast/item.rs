use crate::ast::item_type_alias::ItemTypeAlias;
use super::prelude::*;
use super::item_enum::ItemEnum;

#[derive(Debug)]
pub enum Item<'i> {
    Const(ItemConst),
    Enum(ItemEnum<'i>),
    TypeAlias(ItemTypeAlias<'i>),

}

impl<'i> Parse<'i> for Item<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        crate::util::pest_print_tree(input.pairs.clone());
        let rule = match input.pairs.next() {
            Some(r) => r,
            None => {
                println!("Item::parse: None");
                return Err(());
            }
        };
        let rule_copy = rule.clone();
        match rule.as_rule() {
            Rule::enum_def => {
                ParseInput::fork(rule, input).parse()
                    .map(|item| Item::Enum(item))
            },
            Rule::type_alias_def => {
                ParseInput::fork(rule, input).parse()
                    .map(|item| Item::TypeAlias(item))
                    .map_err(|()| input.push_internal_error(&rule_copy))
            }
            _ => {
                // input.errors.push(ParseError::E0001);
                Err(())
            }
        }
    }
}

#[derive(Debug)]
pub struct ItemConst {

}




