use super::prelude::*;
use crate::ast::item_tuple::TupleFields;
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
pub struct EnumEntryName<'i> {
    pub name: &'i str,
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

impl<'i> Parse<'i> for EnumEntryName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let ident = input.next1(Rule::identifier).ok_or(ParseErrorSource::Internal)?;
        //check_lower_snake_case(&ident, &mut input.warnings);
        Ok(EnumEntryName {
            name: ident.as_str()
        })
    }
}

impl<'i> Parse<'i> for EnumEntryKind<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.peek() {
            Some(_) => {
                let enum_item = input.pairs.next().unwrap();
                println!("enum_item: {:?}", enum_item);
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