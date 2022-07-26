use super::prelude::*;

#[derive(Debug)]
pub struct ItemTypeAlias<'i> {
    pub doc: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub typename: Typename<'i>,
    pub r#type: u32,
}

impl<'i> Parse<'i> for ItemTypeAlias<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        Ok(ItemTypeAlias {
            doc: input.parse()?,
            attrs: input.parse()?,
            typename: input.parse()?,
            r#type: 0
        })
    }
}