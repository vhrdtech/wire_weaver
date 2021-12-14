use crate::warning::ParseWarning;
use crate::error::ParseError;

pub struct ParseInput<'i, 'm> {
    pub pairs: pest::iterators::Pairs<'i, crate::lexer::Rule>,
    pub warnings: &'m mut Vec<ParseWarning>,
    pub errors: &'m mut Vec<ParseError>,
}

impl<'i, 'm> ParseInput<'i, 'm> {
    pub fn new(
        pairs: pest::iterators::Pairs<'i, crate::lexer::Rule>,
        warnings: &'m mut Vec<ParseWarning>,
        errors: &'m mut Vec<ParseError>
    ) -> Self {
        ParseInput {
            pairs, warnings, errors
        }
    }

    pub fn fork(
        pair: pest::iterators::Pair<'i, crate::lexer::Rule>,
        prev_input: &'m mut ParseInput,
    ) -> Self {
        ParseInput {
            pairs: pair.into_inner(), warnings: prev_input.warnings, errors: prev_input.errors
        }
    }
}

/// Parsing interface implemented by all AST nodes
pub trait Parse<'i>: Sized {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()>;
}

impl<'i, 'm> ParseInput<'i, 'm> {
    pub fn parse<T: Parse<'i>>(&mut self) -> Result<T, ()> {
        T::parse(self)
    }
}