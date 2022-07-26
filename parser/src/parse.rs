use crate::warning::ParseWarning;
use crate::lexer::Rule;
use pest::iterators::{Pairs, Pair};
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};

pub struct ParseInput<'i, 'm> {
    pub pairs: Pairs<'i, Rule>,
    pub warnings: &'m mut Vec<ParseWarning>,
    pub errors: &'m mut Vec<ParseError>,
}

impl<'i, 'm> ParseInput<'i, 'm> {
    pub fn new(
        pairs: Pairs<'i, Rule>,
        warnings: &'m mut Vec<ParseWarning>,
        errors: &'m mut Vec<ParseError>
    ) -> Self {
        ParseInput {
            pairs, warnings, errors
        }
    }

    pub fn fork(
        pair: Pair<'i, Rule>,
        prev_input: &'m mut ParseInput,
    ) -> Self {
        ParseInput {
            pairs: pair.into_inner(), warnings: prev_input.warnings, errors: prev_input.errors
        }
    }

    pub fn next1(&mut self, rule1: Rule) -> Option<Pair<'i, Rule>> {
        match self.pairs.next() {
            Some(p1) => {
                if p1.as_rule() == rule1 {
                    Some(p1)
                } else {
                    None
                }
            },
            None => None
        }
    }

    pub fn next2(&mut self, rule1: Rule, rule2: Rule) -> (Option<Pair<'i, Rule>>, Option<Pair<'i, Rule>>) {
        let (p1, p2) = (self.pairs.next(), self.pairs.next());
        let p1 = match p1 {
            Some(p1) => {
                if p1.as_rule() == rule1 {
                    Some(p1)
                } else {
                    None
                }
            },
            None => None
        };

        let p2 = match p2 {
            Some(p2) => {
                if p2.as_rule() == rule2 {
                    Some(p2)
                } else {
                    None
                }
            },
            None => None
        };

        (p1, p2)
    }

    pub fn push_internal_error(&mut self, on_pair: &Pair<'i, Rule>) {
        self.errors.push(ParseError {
            kind: ParseErrorKind::InternalError,
            rule: on_pair.as_rule(),
            span: (on_pair.as_span().start(), on_pair.as_span().end())
        })
    }
}

/// Parsing interface implemented by all AST nodes
pub trait Parse<'i>: Sized {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource>;
}

impl<'i, 'm> ParseInput<'i, 'm> {
    pub fn parse<T: Parse<'i>>(&mut self) -> Result<T, ParseErrorSource> {
        T::parse(self)
    }
}