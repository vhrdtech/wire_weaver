use crate::parse::{ParseInput, Parse};
use crate::error::ParseError;
use crate::lexer::Rule;

#[derive(Copy, Clone, Debug)]
pub enum Item {
    EnumItem(EnumItem)
}

impl Parse for Item {
    fn parse(mut input: ParseInput) -> Option<Self> {
        match input.pair.as_rule() {
            Rule::definition => {
                let def_pair = input.pair.into_inner().next().unwrap();
                match def_pair.as_rule() {
                    Rule::enum_def => {
                        ParseInput::new(def_pair, &mut input.warnings, &mut input.errors).parse().map(|enum_item| Item::EnumItem(enum_item))
                    },
                    _ => None
                }
            },
            _ => {
                input.errors.push(ParseError::E0001);
                None
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EnumItem {

}

impl Parse for EnumItem {
    fn parse(input: ParseInput) -> Option<Self> {
        for p in input.pair.into_inner() {
            println!("{:#?}", p);
        }
        Some(EnumItem {

        })
    }
}

