use crate::ast::item_type::Type;
use super::prelude::*;

#[derive(Debug)]
pub struct ItemTypeAlias<'i> {
    pub doc: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Typename<'i>,
    pub r#type: Type<'i>,
}

impl<'i> Parse<'i> for ItemTypeAlias<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        Ok(ItemTypeAlias {
            doc: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            r#type: input.parse()?
        })
    }
}