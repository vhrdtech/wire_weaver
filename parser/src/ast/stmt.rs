use super::prelude::*;

#[derive(Debug)]
pub struct Stmt<'i> {
    x: &'i str,
}

impl<'i> Parse<'i> for Stmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut _input = ParseInput::fork(input.expect1(Rule::statement)?, input);
        Err(ParseErrorSource::UnexpectedInput)
    }
}