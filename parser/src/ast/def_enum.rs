use super::prelude::*;
use crate::ast::naming::{EnumFieldName, EnumTyName};
use crate::ast::tuple::TupleFieldsTy;
use crate::error::ParseErrorSource;

#[derive(Debug, Clone)]
pub struct DefEnum<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Identifier<'i, EnumTyName>,
    pub entries: EnumItems<'i>,
}

#[derive(Debug, Clone)]
pub struct EnumItems<'i> {
    pub entries: Vec<EnumItem<'i>>,
}

#[derive(Debug, Clone)]
pub struct EnumItem<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub name: Identifier<'i, EnumFieldName>,
    pub kind: Option<EnumItemKind<'i>>,
}

#[derive(Debug, Clone)]
pub enum EnumItemKind<'i> {
    Tuple(TupleFieldsTy<'i>),
    Struct,
    Discriminant(&'i str),
}

impl<'i> Parse<'i> for DefEnum<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_def)?, input);

        Ok(DefEnum {
            docs: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            entries: input.parse()?,
        })
    }
}

impl<'i> Parse<'i> for EnumItems<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut entries = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let mut input = ParseInput::fork(input.expect1(Rule::enum_item)?, input);
            entries.push(input.parse()?);
        }

        Ok(EnumItems { entries })
    }
}

impl<'i> Parse<'i> for EnumItem<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(EnumItem {
            docs: input.parse()?,
            attrs: input.parse()?,
            name: input.parse()?,
            kind: input.parse_or_skip()?,
        })
    }
}

impl<'i> Parse<'i> for EnumItemKind<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_item_kind)?, input);
        let entry_kind = match input.pairs.peek() {
            Some(entry_kind) => entry_kind,
            None => return Err(ParseErrorSource::internal("enum_item_kind: expected kind")),
        };

        match entry_kind.as_rule() {
            Rule::enum_item_tuple => {
                let mut input = ParseInput::fork(input.expect1(Rule::enum_item_tuple)?, &mut input);
                Ok(EnumItemKind::Tuple(input.parse()?))
            }
            Rule::enum_item_struct => Err(ParseErrorSource::Unimplemented("enum item struct")),
            Rule::enum_item_discriminant => {
                Err(ParseErrorSource::Unimplemented("enum item discriminant"))
            }
            _ => return Err(ParseErrorSource::internal("unexpected enum kind")),
        }
    }
}
