use super::prelude::*;

#[derive(Debug)]
pub struct ItemExpr<'i> {
    x: &'i str,
}

impl<'i> Parse<'i> for ItemExpr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(ItemExpr {
            x: "fake_expr"
        })
    }
}