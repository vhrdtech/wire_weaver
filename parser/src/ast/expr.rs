use super::prelude::*;

#[derive(Debug)]
pub struct Expr<'i> {
    x: &'i str,
}

impl<'i> Parse<'i> for Expr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(Expr {
            x: "fake_expr"
        })
    }
}