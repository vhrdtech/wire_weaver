use super::prelude::*;

#[derive(Debug)]
pub struct Generics<'i> {
    pub x: &'i str,
}

impl<'i> Parse<'i> for Generics<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let _input = ParseInput::fork(input.expect1(Rule::generics)?, input);
        Ok(Generics {
            x: "fake"
        })
    }
}