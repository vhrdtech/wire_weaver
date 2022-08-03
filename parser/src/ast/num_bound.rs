use super::prelude::*;

#[derive(Debug)]
pub struct NumBound<'i> {
    pub x: &'i str,
}

impl<'i> Parse<'i> for NumBound<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(NumBound {
            x: "dummy"
        })
    }
}

