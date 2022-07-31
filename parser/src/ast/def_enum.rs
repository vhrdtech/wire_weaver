use super::prelude::*;
use crate::ast::tuple::TupleFieldsTy;
use crate::ast::naming::EnumEntryName;
use crate::error::ParseErrorSource;

#[derive(Debug)]
pub struct DefEnum<'i> {
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
    Tuple(TupleFieldsTy<'i>),
    Struct,
    Discriminant(&'i str)
}

impl<'i> Parse<'i> for DefEnum<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_def)?, input);

        Ok(DefEnum {
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
        while let Some(_) = input.pairs.peek() {
            let mut input = ParseInput::fork(input.expect1(Rule::enum_item)?, input);
            entries.push(input.parse()?);
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
            kind: input.parse_or_skip()?
        })
    }
}

impl<'i> Parse<'i> for EnumEntryKind<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_item_kind)?, input);
        let entry_kind = match input.pairs.peek() {
            Some(entry_kind) => entry_kind,
            None => {
                return Err(ParseErrorSource::internal())
            }
        };

        match entry_kind.as_rule() {
            Rule::enum_item_tuple => {
                let mut input = ParseInput::fork(input.expect1(Rule::enum_item_tuple)?, &mut input);
                Ok(EnumEntryKind::Tuple(input.parse()?))
            }
            Rule::enum_item_struct => {
                Err(ParseErrorSource::Unimplemented("enum item struct"))
            }
            Rule::enum_item_discriminant => {
                Err(ParseErrorSource::Unimplemented("enum item discriminant"))
            }
            _ => {
                return Err(ParseErrorSource::internal())
            }
        }
    }
}