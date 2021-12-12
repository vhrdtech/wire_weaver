use crate::warning::ParseWarning;
use crate::error::ParseError;

pub struct ParseInput<'i> {
    pub pair: pest::iterators::Pair<'i, crate::lexer::Rule>,
    pub warnings: &'i mut Vec<ParseWarning>,
    pub errors: &'i mut Vec<ParseError>,
}

impl<'i> ParseInput<'i> {
    pub fn new(
        pair: pest::iterators::Pair<'i, crate::lexer::Rule>,
        warnings: &'i mut Vec<ParseWarning>,
        errors: &'i mut Vec<ParseError>
    ) -> Self {
        ParseInput {
            pair, warnings, errors
        }
    }
}

/// Parsing interface implemented by all AST nodes
pub trait Parse: Sized {
    fn parse(input: ParseInput) -> Option<Self>;
}

impl<'i> ParseInput<'i> {
    pub fn parse<T: Parse>(self) -> Option<T> {
        T::parse(self)
    }
}