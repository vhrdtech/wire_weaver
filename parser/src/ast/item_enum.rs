use super::prelude::*;
use crate::ast::item_tuple::TupleFields;
use crate::error::ParseErrorSource;

#[derive(Debug)]
pub struct ItemEnum<'i> {
    pub docs: Doc<'i>,
    // attrs: Vec<Attribute>,
    pub typename: Typename<'i>,
    pub items: EnumItems<'i>
}

#[derive(Debug)]
pub struct EnumItems<'i> {
    pub items: Vec<EnumItem<'i>>,
}

#[derive(Debug)]
pub struct EnumItem<'i> {
    pub docs: Doc<'i>,
    pub name: EnumItemName<'i>,
    pub kind: Option<EnumItemKind<'i>>
}

#[derive(Debug)]
pub struct EnumItemName<'i> {
    pub name: &'i str,
}

#[derive(Debug)]
pub enum EnumItemKind<'i> {
    Tuple(TupleFields<'i>),
    Struct,
    Discriminant(&'i str)
}

impl<'i> Parse<'i> for ItemEnum<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(ItemEnum {
            docs: input.parse()?,
            typename: input.parse()?,
            items: input.parse()?
        })
    }
}

impl<'i> Parse<'i> for EnumItems<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut items = Vec::new();
        if let Some(p) = input.pairs.peek() {
            if p.as_rule() == Rule::enum_items {
                let p = input.pairs.next().unwrap();
                for item in p.into_inner() {
                    match ParseInput::fork(item, input).parse() {
                        Ok(item) => {
                            items.push(item);
                        },
                        Err(e) => {
                            println!("enum item parse error");
                            return Err(ParseErrorSource::Internal);
                        }
                    }
                }
            } else {
                println!("enum unexpected rule: {:?}", p);
                return Err(ParseErrorSource::Internal);
            }
        } else {
            println!("enum items absent");
            return Err(ParseErrorSource::Internal);
        }

        Ok(EnumItems {
            items
        })
    }
}

impl<'i> Parse<'i> for EnumItem<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(EnumItem {
            docs: input.parse()?,
            name: input.parse()?,
            kind: input.parse().ok()
        })
    }
}

impl<'i> Parse<'i> for EnumItemName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::identifier {
                let p = input.pairs.next().unwrap();
                Ok(EnumItemName {
                    name: p.as_str()
                })
            } else {
                Err(ParseErrorSource::Internal)
            };
        }
        Err(ParseErrorSource::Internal)
    }
}

impl<'i> Parse<'i> for EnumItemKind<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.peek() {
            Some(_) => {
                let enum_item = input.pairs.next().unwrap();
                println!("enum_item: {:?}", enum_item);
                match enum_item.as_rule() {
                    Rule::enum_item_tuple => {
                        Ok(EnumItemKind::Tuple(ParseInput::fork(enum_item, input).parse()?))
                    }
                    Rule::enum_item_struct => {
                        Err(ParseErrorSource::Internal)
                    }
                    Rule::enum_item_discriminant => {
                        Err(ParseErrorSource::Internal)
                    }
                    _ => unreachable!()
                }
            },
            None => {
                println!("enum_item none");
                Err(ParseErrorSource::Internal)
            }
        }
    }
}