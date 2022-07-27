use super::prelude::*;

#[derive(Debug)]
pub struct ItemStmt<'i> {
    x: &'i str,
}

impl<'i> Parse<'i> for ItemStmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut _input = ParseInput::fork(input.expect1(Rule::statement)?, input);
        Err(ParseErrorSource::UnexpectedInput)
    }
}