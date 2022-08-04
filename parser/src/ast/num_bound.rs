use super::prelude::*;

#[derive(Debug)]
pub struct NumBound<'i> {
    pub x: &'i str,
}

impl<'i> Parse<'i> for NumBound<'i> {
    fn parse<'m>(_input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        Ok(NumBound {
            x: "dummy"
        })
    }
}

