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

    /// Given the following pairs tree:
    /// rule_a, rule_b, rule_c, ...
    ///   |_ rule_a1, rule_a2, ...
    /// Where rula_ai is children rules inside rula_a.
    ///
    /// If called with expected_rule == rule_a, will return Ok with input on rula_a1 ready to be parsed.
    /// Otherwise UnexpectedInput error will be return, that can be ignored by `parse_or_skip()`
    /// or propagated and recorded.
    pub fn fork_at(mut self, expected_rule: Rule) -> Result<ParseInput<'i, 'm>, ParseErrorSource> {
        let pair = self.pairs.next().ok_or(ParseErrorSource::UnexpectedInput)?;
        if pair.as_rule() == expected_rule {
            Ok(ParseInput {
                pairs: pair.into_inner(), warnings: self.warnings, errors: self.errors
            })
        } else {
            Err(ParseErrorSource::UnexpectedInput)
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
    /// Try to parse input, return Ok(Some(Self)) if succeeded, Ok(None) if input is absent and it
    /// is expected, Err if user or internal error was encountered.
    /// While parsing, push warnings and errors into respective lists.
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource>;
}

impl<'i, 'm> ParseInput<'i, 'm> {
    /// Try to parse input stream into an Ok(AST node). Return error otherwise.
    pub fn parse<T: Parse<'i>>(&mut self) -> Result<T, ParseErrorSource> {
        T::parse(self)
    }

    /// Try to parse input stream into an Ok(Option<AST node>). If failed with UnexpectedInput,
    /// ignore and return Ok(None).
    /// Parsing can continue, leaving None in a field, where that was intended.
    ///
    /// Otherwise return original error that will be propagated further and parsing of the current
    /// node will fail.
    pub fn parse_or_skip<T: Parse<'i>>(&mut self) -> Result<Option<T>, ParseErrorSource> {
        match T::parse(self) {
            Ok(t) => Ok(Some(t)),
            Err(e) => {
                match e {
                    e @ ParseErrorSource::InternalError => Err(e),
                    ParseErrorSource::UnexpectedInput => Ok(None),
                    e @ ParseErrorSource::UserError => Err(e)
                }
            }
        }
    }
}