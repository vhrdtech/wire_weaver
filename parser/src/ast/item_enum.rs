use super::prelude::*;
use super::item::{Typename, Docs};

#[derive(Debug)]
pub struct ItemEnum<'i> {
    docs: Docs<'i>,
    // attrs: Vec<Attribute>,
    typename: Typename<'i>,
    items: EnumItems<'i>
}

#[derive(Debug)]
pub struct EnumItems<'i> {
    items: Vec<EnumItem<'i>>,
}

#[derive(Debug)]
pub struct EnumItem<'i> {
    docs: Docs<'i>,
    name: EnumItemName<'i>,
}

#[derive(Debug)]
pub struct EnumItemName<'i> {
    name: &'i str,
}

impl<'i> Parse<'i> for ItemEnum<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        Ok(ItemEnum {
            docs: input.parse()?,
            typename: input.parse()?,
            items: input.parse()?
        })
    }
}

impl<'i> Parse<'i> for EnumItems<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        let mut items = Vec::new();
        if let Some(p) = input.pairs.peek() {
            if p.as_rule() == Rule::enum_items {
                let p = input.pairs.next().unwrap();
                for item in p.into_inner() {
                    match ParseInput::fork(item, input).parse() {
                        Ok(item) => {
                            items.push(item);
                        },
                        Err(()) => {
                            println!("enum item parse error");
                            return Err(());
                        }
                    }
                }
            } else {
                println!("enum unexpected rule: {:?}", p);
                return Err(());
            }
        } else {
            println!("enum items absent");
            return Err(());
        }

        Ok(EnumItems {
            items
        })
    }
}

impl<'i> Parse<'i> for EnumItem<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        Ok(EnumItem {
            docs: input.parse()?,
            name: input.parse()?
        })
    }
}

impl<'i> Parse<'i> for EnumItemName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::enum_item_name {
                let p = input.pairs.next().unwrap();
                Ok(EnumItemName {
                    name: p.as_str()
                })
            } else {
                Err(())
            };
        }
        Err(())
    }
}
