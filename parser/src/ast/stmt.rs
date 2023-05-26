use super::prelude::*;
use crate::ast::definition::DefinitionParse;
use crate::ast::expr::ExprParse;
use crate::ast::ty::TyParse;
use crate::error::{Error, ErrorKind, ParseError, ParseErrorKind};
use crate::lexer::{Lexer, Rule};
use crate::warning::ParseWarning;
use ast::span::SpanOrigin;
use ast::stmt::LetStmt;
use ast::Stmt;
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};

// Used when parsing whole file
pub struct StmtParse(pub Stmt);

// Used in REPL
pub struct StmtParseDetached {
    pub stmt: Stmt,
    pub input: String,
    pub warnings: Vec<ParseWarning>,
}

pub struct LetStmtParse(pub LetStmt);

pub struct VecStmtParse(pub Vec<Stmt>);

impl StmtParseDetached {
    pub fn parse_detached<S: AsRef<str>>(input: S, origin: SpanOrigin) -> Result<Self, Box<Error>> {
        let input = input.as_ref();
        let pairs = <Lexer as pest::Parser<Rule>>::parse(Rule::repl, input).map_err(|e| Error {
            kind: ErrorKind::Grammar(e),
            origin: origin.clone(),
            input: input.to_owned(),
        })?;
        let mut errors = Vec::new();

        let input_parsed_str = pairs.as_str();
        if !input.contains(input_parsed_str) {
            errors.push(ParseError {
                kind: ParseErrorKind::UnexpectedUnconsumedInput(input_parsed_str.to_owned()),
                rule: Rule::statement,
                span: input_parsed_str.len()..input.len(),
            });
            return Err(Box::new(Error {
                kind: ErrorKind::Parser(errors),
                origin,
                input: input.to_owned(),
            }));
        }

        let Some(pair) = pairs.peek() else {
            errors.push(ParseError {
                kind: ParseErrorKind::EmptyInput,
                rule: Rule::statement,
                span: input_parsed_str.len()..input.len(),
            });
            return Err(Box::new(Error {
                kind: ErrorKind::Parser(errors),
                origin,
                input: input.to_owned(),
            }));
        };
        let span = pair.as_span().start()..pair.as_span().end();
        let rule = pair.as_rule();
        let pair_span = ast_span_from_pest(pair.as_span());
        let mut warnings = Vec::new();
        let mut input_parse = ParseInput::new(pairs, pair_span, &mut warnings, &mut errors);
        let stmt_parse: Result<StmtParse, ParseErrorSource> = input_parse.parse();
        match stmt_parse {
            Ok(stmt) => Ok(StmtParseDetached {
                stmt: stmt.0,
                input: input.to_owned(),
                warnings,
            }),
            Err(e) => {
                let kind = match e {
                    ParseErrorSource::InternalError { rule, message } => {
                        ParseErrorKind::InternalError { rule, message }
                    }
                    ParseErrorSource::Unimplemented(f) => ParseErrorKind::Unimplemented(f),
                    ParseErrorSource::UnexpectedInput {
                        expect1,
                        expect2,
                        got,
                        context,
                        span,
                    } => ParseErrorKind::UnhandledUnexpectedInput {
                        expect1,
                        expect2,
                        got,
                        context,
                        span,
                    },
                    ParseErrorSource::UserError => ParseErrorKind::UserError,
                };
                errors.push(ParseError { kind, rule, span });
                Err(Box::new(Error {
                    kind: ErrorKind::Parser(errors),
                    origin,
                    input: input.to_owned(),
                }))
            }
        }
    }

    pub fn print_warnings_report(&self) {
        let writer = StandardStream::stderr(ColorChoice::Always);
        let config = codespan_reporting::term::Config::default();
        let file = SimpleFile::new("str", &self.input);
        for diagnostic in self.warnings.iter().map(ParseWarning::to_diagnostic) {
            codespan_reporting::term::emit(&mut writer.lock(), &config, &file, &diagnostic)
                .unwrap();
        }
    }
}

impl<'i> Parse<'i> for StmtParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::statement, "StmtParse")?, input);
        match input.pairs.peek().map(|r| (r.clone(), r.as_rule())) {
            Some((_, Rule::let_stmt)) => {
                let let_stmt: LetStmtParse = input.parse()?;
                Ok(StmtParse(Stmt::Let(let_stmt.0)))
            }
            Some((s, Rule::expr_stmt)) => {
                let _ = input.pairs.next();
                let mut input = ParseInput::fork(s, &mut input);
                let expr: ExprParse = input.parse()?;
                let semicolon_present = input.pairs.next().is_some();
                Ok(StmtParse(Stmt::Expr(expr.0, semicolon_present)))
            }
            // Rule::braced_statement => {
            //     let mut input = ParseInput::fork(s, &mut input);
            //     let stmt: StmtParse = input.parse()?;
            //     Ok(stmt)
            // }
            Some((s, Rule::definition)) => {
                let mut input = ParseInput::fork(s, &mut input);
                let def: DefinitionParse = input.parse()?;
                Ok(StmtParse(Stmt::Def(def.0)))
            }
            Some((s, _)) => Err(ParseErrorSource::internal_with_rule(
                s.as_rule(),
                "StmtParse: unexpected rule",
            )),
            None => Err(ParseErrorSource::internal("StmtParse: wrong grammar")),
        }
    }
}

impl<'i> Parse<'i> for LetStmtParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::let_stmt, "LetStmtParse")?, input);
        let ident: IdentifierParse<identifier::VariableDefName> = input.parse()?;
        let ty: Option<TyParse> = input.parse_or_skip()?;
        let expr: ExprParse = input.parse()?;
        Ok(LetStmtParse(LetStmt {
            ident: ident.0,
            type_ascription: ty.map(|ty| ty.0),
            expr: expr.0,
        }))
    }
}

impl<'i> Parse<'i> for VecStmtParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut stmts = Vec::new();
        while input.pairs.peek().is_some() {
            let stmt: StmtParse = input.parse()?;
            stmts.push(stmt.0);
        }
        Ok(VecStmtParse(stmts))
    }
}
