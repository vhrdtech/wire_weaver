use pest::iterators::Pair;
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
                return Err(ParseErrorSource::InternalError);
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
        let entry_kind = match input.pairs.next() {
            Some(kind) => kind,
            None => {
                return Err(ParseErrorSource::InternalError)
            }
        };
        let entry_kind_clone = entry_kind.clone();

        // enum entry kind is an Option and parsed as: parse().ok()
        // Empty input will be returned as error and ignored. Actual parse error will be remembered.
        parse_enum_entry_kind(entry_kind, input).map_err(|e| {
            if e == ParseErrorSource::InternalError {
                input.push_internal_error(&entry_kind_clone);
            }
            e
        })
    }
}

fn parse_enum_entry_kind<'i, 'm>(entry_kind: Pair<'i, Rule>, input: &mut ParseInput<'i, 'm>) -> Result<EnumEntryKind<'i>, ParseErrorSource> {
    match entry_kind.as_rule() {
        Rule::enum_item_tuple => {
            Ok(EnumEntryKind::Tuple(ParseInput::fork(entry_kind, input).parse()?))
        }
        Rule::enum_item_struct => {
            Err(ParseErrorSource::InternalError)
        }
        Rule::enum_item_discriminant => {
            Err(ParseErrorSource::InternalError)
        }
        _ => { return Err(ParseErrorSource::InternalError) }
    }
}