use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use crate::lexer::Rule;
use crate::span::ast_span_from_pest;
use crate::warning::ParseWarning;
use ast::Span;
use pest::iterators::{Pair, Pairs};

pub struct ParseInput<'i, 'm> {
    pub pairs: Pairs<'i, Rule>,
    pub span: Span,
    pub warnings: &'m mut Vec<ParseWarning>,
    pub errors: &'m mut Vec<ParseError>,
}

impl<'i, 'm> ParseInput<'i, 'm> {
    pub fn new(
        pairs: Pairs<'i, Rule>,
        span: Span,
        warnings: &'m mut Vec<ParseWarning>,
        errors: &'m mut Vec<ParseError>,
    ) -> Self {
        ParseInput {
            pairs,
            span,
            warnings,
            errors,
        }
    }

    /// Given the following pairs tree:
    /// rule_a, rule_b, rule_c, ...
    ///   |_ rule_a1, rule_a2, ...
    /// Where rula_ai is children rules inside rula_a.
    ///
    /// If called with expected_rule == rule_a, will return with input on rula_a1 ready to be parsed.
    ///
    /// Use fork(input.expect1(rule_a)?, input) if rula_a is expected to be the next rule to parse.
    /// Otherwise UnexpectedInput error will be return, that can be ignored by `parse_or_skip()`
    /// or propagated and recorded.
    pub fn fork(pair: Pair<'i, Rule>, prev_input: &'m mut ParseInput) -> Self {
        let span = pair.as_span();
        ParseInput {
            pairs: pair.into_inner(),
            span: ast_span_from_pest(span),
            warnings: prev_input.warnings,
            errors: prev_input.errors,
        }
    }

    /// Consume and return next pair if it exists with specified rule. Otherwise return an error,
    /// leaving input as before.
    pub fn expect1(&mut self, rule1: Rule, context: &'static str) -> Result<Pair<'i, Rule>, ParseErrorSource> {
        match self.pairs.peek() {
            Some(p1) => {
                if p1.as_rule() == rule1 {
                    let _ = self.pairs.next();
                    Ok(p1)
                } else {
                    Err(ParseErrorSource::UnexpectedInput {
                        expect1: Some(rule1),
                        expect2: None,
                        got: Some(p1.as_rule()),
                        context,
                        span: self.span.clone()
                    })
                }
            }
            None => Err(ParseErrorSource::UnexpectedInput {
                expect1: Some(rule1),
                expect2: None,
                got: None,
                context,
                span: self.span.clone()
            }),
        }
    }

    /// Consume and return next pair if it exists with one of the specified rules.
    /// Otherwise return an error, leaving input as before.
    pub fn expect1_either(
        &mut self,
        rule1: Rule,
        rule2: Rule,
        context: &'static str
    ) -> Result<Pair<'i, Rule>, ParseErrorSource> {
        match self.pairs.peek() {
            Some(p1) => {
                if p1.as_rule() == rule1 || p1.as_rule() == rule2 {
                    let _ = self.pairs.next();
                    Ok(p1)
                } else {
                    Err(ParseErrorSource::UnexpectedInput {
                        expect1: Some(rule1),
                        expect2: Some(rule2),
                        got: Some(p1.as_rule()),
                        context,
                        span: self.span.clone()
                    })
                }
            }
            None => Err(ParseErrorSource::UnexpectedInput {
                expect1: Some(rule1),
                expect2: Some(rule2),
                got: None,
                context,
                span: self.span.clone()
            }),
        }
    }

    pub fn expect2(
        &mut self,
        rule1: Rule,
        rule2: Rule,
        context: &'static str
    ) -> Result<(Pair<'i, Rule>, Pair<'i, Rule>), ParseErrorSource> {
        Ok((self.expect1(rule1, context)?, self.expect1(rule2, context)?))
    }

    /// Consume and return next pair if it exists.
    pub fn expect1_any(&mut self, context: &'static str) -> Result<Pair<'i, Rule>, ParseErrorSource> {
        self.pairs
            .next()
            .ok_or_else(|| ParseErrorSource::UnexpectedInput {
                expect1: None,
                expect2: None,
                got: None,
                context,
                span: self.span.clone()
            })
    }

    pub fn push_error(&mut self, on_pair: &Pair<'i, Rule>, kind: ParseErrorKind) {
        self.errors.push(ParseError {
            kind,
            rule: on_pair.as_rule(),
            span: on_pair.as_span().start()..on_pair.as_span().end(),
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
            Err(e) => match e {
                e @ ParseErrorSource::InternalError { .. } => Err(e),
                e @ ParseErrorSource::Unimplemented(_) => Err(e),
                ParseErrorSource::UnexpectedInput { .. } => Ok(None),
                e @ ParseErrorSource::UserError => Err(e),
            },
        }
    }
}
