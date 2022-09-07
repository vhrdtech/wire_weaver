use crate::ast::expr::Expr;
use crate::ast::file::FileError;
use crate::ast::naming::VariableDefName;
use crate::ast::ty::Ty;
use crate::error::{ParseError, ParseErrorKind};
use super::prelude::*;
use crate::lexer::{Lexer, Rule};

#[derive(Debug, Clone)]
pub enum Stmt<'i> {
    Let(LetStmt<'i>),
    Expr(Expr<'i>, bool)
}


impl<'i> Stmt<'i> {
    pub fn parse(input: &'i str) -> Result<Self, FileError> {
        let pairs = <Lexer as pest::Parser<Rule>>::parse(Rule::statement, input)?;
        let mut errors = Vec::new();

        let input_parsed_str = pairs.as_str();
        if input_parsed_str != input {
            errors.push(ParseError {
                kind: ParseErrorKind::UnhandledUnexpectedInput,
                rule: Rule::statement,
                span: (input_parsed_str.len(), input.len())
            });
            return Err(FileError::ParserError(errors));
        }
        // println!("{:?}", pairs);

        // TODO: Improve this
        let pair = pairs.peek().unwrap();
        let span = (pair.as_span().start(), pair.as_span().end());
        let rule = pair.as_rule();
        let pair_span = pair.as_span();
        let mut warnings = Vec::new();
        let mut input = ParseInput::new(pairs, pair_span, &mut warnings, &mut errors);
        match input.parse() {
            Ok(stmt) => {
                Ok(stmt)
            },
            Err(e) => {
                let kind = match e {
                    #[cfg(feature = "backtrace")]
                    ParseErrorSource::InternalError{ rule, backtrace } => ParseErrorKind::InternalError{rule, backtrace: backtrace.to_string()},
                    #[cfg(not(feature = "backtrace"))]
                    ParseErrorSource::InternalError{ rule, message } => ParseErrorKind::InternalError{rule, message},
                    ParseErrorSource::Unimplemented(f) => ParseErrorKind::Unimplemented(f),
                    ParseErrorSource::UnexpectedInput => ParseErrorKind::UnhandledUnexpectedInput,
                    ParseErrorSource::UserError => ParseErrorKind::UserError
                };
                errors.push(ParseError {
                    kind,
                    rule,
                    span
                });
                Err(FileError::ParserError(errors))
            }
        }
    }
}

impl<'i> Parse<'i> for Stmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::statement)?, input);
        let s = input.pairs.peek().ok_or_else(|| ParseErrorSource::UnexpectedInput)?;
        match s.as_rule() {
            Rule::let_stmt => {
                Ok(Stmt::Let(input.parse()?))
            },
            Rule::expr_stmt => {
                let _ = input.pairs.next();
                let mut input = ParseInput::fork(s, &mut input);
                let expr: Expr = input.parse()?;
                let semicolon_present = input.pairs.next().is_some();
                Ok(Stmt::Expr(expr, semicolon_present))
            },
            _ => {
                Err(ParseErrorSource::internal_with_rule(s.as_rule(), "Stmt::parse: unexpected rule"))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LetStmt<'i> {
    pub ident: Identifier<'i, VariableDefName>,
    pub type_ascription: Option<Ty<'i>>,
    pub expr: Expr<'i>
}

impl<'i> Parse<'i> for LetStmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::let_stmt)?, input);
        Ok(LetStmt {
            ident: input.parse()?,
            type_ascription: input.parse_or_skip()?,
            expr: input.parse()?
        })
    }
}