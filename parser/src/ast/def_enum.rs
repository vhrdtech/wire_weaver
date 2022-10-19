use ast::{EnumDef, EnumItem, EnumItemKind};
use crate::ast::def_struct::StructFieldsParse;
use crate::ast::lit::LitParse;
use super::prelude::*;
use crate::ast::ty::TupleTyParse;
use crate::error::ParseErrorSource;

pub struct EnumDefParse(pub EnumDef);

pub struct EnumItemsParse(pub Vec<EnumItem>);

pub struct EnumItemKindParse(pub EnumItemKind);

impl<'i> Parse<'i> for EnumDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_def)?, input);
        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let typename: IdentifierParse<identifier::EnumTyName> = input.parse()?;
        let items: EnumItemsParse = input.parse()?;
        Ok(EnumDefParse(EnumDef {
            doc: doc.0,
            attrs: attrs.0,
            typename: typename.0,
            items: items.0,
            span: input.span.clone(),
        }))
    }
}

impl<'i> Parse<'i> for EnumItemsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut entries = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let mut input = ParseInput::fork(input.expect1(Rule::enum_item)?, input);
            let doc: DocParse = input.parse()?;
            let attrs: AttrsParse = input.parse()?;
            let name: IdentifierParse<identifier::EnumTyName> = input.parse()?;
            let kind: Option<EnumItemKindParse> = input.parse_or_skip()?;
            entries.push(EnumItem {
                doc: doc.0,
                attrs: attrs.0,
                name: name.0,
                kind: kind.map(|kind| kind.0),
            });
        }

        Ok(EnumItemsParse(entries))
    }
}

impl<'i> Parse<'i> for EnumItemKindParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::enum_item_kind)?, input);
        let entry_kind = match input.pairs.peek() {
            Some(entry_kind) => entry_kind,
            None => return Err(ParseErrorSource::internal("enum_item_kind: expected kind")),
        };

        match entry_kind.as_rule() {
            Rule::enum_item_tuple => {
                let mut input = ParseInput::fork(input.expect1(Rule::enum_item_tuple)?, &mut input);
                let tuple_ty: TupleTyParse = input.parse()?;
                Ok(EnumItemKindParse(EnumItemKind::Tuple(tuple_ty.0)))
            }
            Rule::enum_item_struct => {
                let mut input = ParseInput::fork(input.expect1(Rule::enum_item_struct)?, &mut input);
                let fields: StructFieldsParse = input.parse()?;
                Ok(EnumItemKindParse(EnumItemKind::Struct(fields.0)))
            },
            Rule::enum_item_discriminant => {
                let mut input = ParseInput::fork(input.expect1(Rule::enum_item_discriminant)?, &mut input);
                let lit: LitParse = input.parse()?;
                Ok(EnumItemKindParse(EnumItemKind::Discriminant(lit.0)))
            }
            _ => return Err(ParseErrorSource::internal("unexpected enum kind")),
        }
    }
}
