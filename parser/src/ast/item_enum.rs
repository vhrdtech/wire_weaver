use super::prelude::*;
use crate::ast::item_tuple::TupleFields;
use crate::ast::naming::EnumEntryName;
use crate::error::ParseErrorSource;

#[derive(Debug)]
pub struct ItemEnum<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Typename<'i>,
    pub entries: EnumEntries<'i>
}

#[derive(Debug)]
pub struct EnumEntries<'i> {
    pub entries: Vec<EnumEntry<'i>>,
}

#[derive(Debug)]
pub struct EnumEntry<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub name: EnumEntryName<'i>,
    pub kind: Option<EnumEntryKind<'i>>
}

#[derive(Debug)]
pub enum EnumEntryKind<'i> {
    Tuple(TupleFields<'i>),
    Struct,
    Discriminant(&'i str)
}

impl<'i> Parse<'i> for ItemEnum<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(ItemEnum {
            docs: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            entries: input.parse()?
        })
    }
}

impl<'i> Parse<'i> for EnumEntries<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut entries = Vec::new();
        while let Some(p) = input.pairs.peek() {
            if p.as_rule() == Rule::enum_item {
                let p = input.pairs.next().unwrap();
                let entry = ParseInput::fork(p, input).parse()?;
                entries.push(entry);
            } else {
                println!("enum unexpected rule: {:?}", p);
                return Err(ParseErrorSource::Internal);
            }
        }

        Ok(EnumEntries {
            entries
        })
    }
}

impl<'i> Parse<'i> for EnumEntry<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(EnumEntry {
            docs: input.parse()?,
            attrs: input.parse()?,
            name: input.parse()?,
            kind: input.parse().ok()
        })
    }
}

impl<'i> Parse<'i> for EnumEntryKind<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.next() {
            Some(enum_item) => {
                match enum_item.as_rule() {
                    Rule::enum_item_tuple => {
                        Ok(EnumEntryKind::Tuple(ParseInput::fork(enum_item, input).parse()?))
                    }
                    Rule::enum_item_struct => {
                        Err(ParseErrorSource::Internal)
                    }
                    Rule::enum_item_discriminant => {
                        Err(ParseErrorSource::Internal)
                    }
                    _ => { return Err(ParseErrorSource::Internal) }
                }
            },
            None => {
                Err(ParseErrorSource::Internal)
            }
        }
    }
}