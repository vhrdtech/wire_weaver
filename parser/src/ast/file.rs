use crate::ast::definition::DefinitionParse;
use crate::error::{Error, ErrorKind, ParseError, ParseErrorKind, ParseErrorSource};
use crate::lexer::{Lexer, Rule};
use crate::parse::ParseInput;
use crate::span::{ast_span_from_pest, ChangeOrigin};
use crate::warning::{ParseWarning, ParseWarningKind};
use ast::span::SpanOrigin;
use ast::VisitMut;
use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FileParse {
    pub ast_file: ast::File,
    pub warnings: Vec<ParseWarning>,
}

impl FileParse {
    pub fn parse<S: AsRef<str>>(input: S, origin: SpanOrigin) -> Result<Self, Error> {
        let mut pi =
            <Lexer as pest::Parser<Rule>>::parse(Rule::file, input.as_ref()).map_err(|e| {
                Error {
                    kind: ErrorKind::Grammar(e),
                    origin: origin.clone(),
                    input: input.as_ref().to_owned(),
                }
            })?;
        let mut defs = HashMap::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        match pi.next() {
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    match p.as_rule() {
                        // Rule::inner_attribute => {
                        //     let attr = pi.next();
                        // }
                        Rule::EOI => {
                            break;
                        }
                        // silent in rules
                        // Rule::COMMENT => {
                        //     let _ = pi.next();
                        // }
                        _ => {
                            let pair = pi.next().unwrap();
                            let pair_span = pair.as_span();
                            let rule = pair.as_rule();
                            let span = (pair.as_span().start(), pair.as_span().end());
                            let mut input = ParseInput::new(
                                pair.into_inner(),
                                ast_span_from_pest(pair_span),
                                &mut warnings,
                                &mut errors,
                            );
                            let def: Result<DefinitionParse, _> = input.parse();
                            match def {
                                Ok(def) => {
                                    let def = def.0;
                                    defs.insert(def.name(), def);
                                }
                                Err(e) => {
                                    let kind = match e {
                                        #[cfg(feature = "backtrace")]
                                        ParseErrorSource::InternalError { rule, backtrace } => {
                                            ParseErrorKind::InternalError {
                                                rule,
                                                backtrace: backtrace.to_string(),
                                            }
                                        }
                                        #[cfg(not(feature = "backtrace"))]
                                        ParseErrorSource::InternalError { rule, message } => {
                                            ParseErrorKind::InternalError { rule, message }
                                        }
                                        ParseErrorSource::Unimplemented(f) => {
                                            ParseErrorKind::Unimplemented(f)
                                        }
                                        ParseErrorSource::UnexpectedInput { expect1, expect2, got, context } => {
                                            ParseErrorKind::UnhandledUnexpectedInput { expect1, expect2, got, context }
                                        }
                                        ParseErrorSource::UserError => ParseErrorKind::UserError,
                                    };
                                    errors.push(ParseError { kind, rule, span });
                                }
                            }
                        }
                    }
                }
            }
            None => {}
        }
        if errors.is_empty() {
            let line_starts = std::iter::once(0)
                .chain(input.as_ref().match_indices('\n').map(|(i, _)| i + 1))
                .collect();
            let mut ast_file = ast::File {
                origin: origin.clone(),
                defs,
                input: input.as_ref().to_owned(),
                line_starts,
            };
            let mut change_origin = ChangeOrigin { to: origin };
            change_origin.visit_file(&mut ast_file);
            Ok(FileParse { ast_file, warnings })
        } else {
            Err(Error {
                kind: ErrorKind::Parser(errors),
                origin: origin.clone(),
                input: input.as_ref().to_owned(),
            })
        }
    }

    pub fn parse_tree<S: AsRef<str>>(
        input: S,
        def_name: S,
        origin: SpanOrigin,
    ) -> Result<Option<String>, Error> {
        let input = input.as_ref();
        let def_name = def_name.as_ref();
        let mut pi =
            <Lexer as pest::Parser<Rule>>::parse(Rule::file, input).map_err(|e| Error {
                kind: ErrorKind::Grammar(e),
                origin: origin.clone(),
                input: input.to_owned(),
            })?;
        let mut tree = None;
        match pi.next() {
            // Rule::file
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.next() {
                    match p.as_rule() {
                        Rule::definition => {
                            let mut name = None;
                            for p in p.clone().into_inner().flatten() {
                                match p.as_rule() {
                                    Rule::identifier => {
                                        name = Some(p.as_str());
                                        break;
                                    }
                                    _ => continue,
                                };
                            }
                            match name {
                                Some(name) => {
                                    if name == def_name {
                                        tree = Some(crate::util::pest_tree(p.into_inner()));
                                        break;
                                    }
                                }
                                None => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
            None => {}
        }
        Ok(tree)
    }

    pub fn report(&self) -> Vec<Diagnostic<()>> {
        self.warnings
            .iter()
            .map(|warning| {
                let range = warning.span.0..warning.span.1;
                match &warning.kind {
                    ParseWarningKind::NonCamelCaseTypename => Diagnostic::warning()
                        .with_message("non camel case typename")
                        .with_labels(vec![
                            Label::primary((), range).with_message("consider renaming to: '{}'")
                        ]),
                    ParseWarningKind::CellWithConstRo => Diagnostic::warning()
                        .with_message("resource containing cell with a constant or read only data")
                        .with_labels(vec![
                            Label::primary((), range).with_message("remove this Cell<_>")
                        ])
                        .with_notes(vec![
                            "const and read only resources are safe to use without a Cell"
                                .to_owned(),
                        ]),
                    ParseWarningKind::CellWithRoStream => Diagnostic::warning()
                        .with_message("resource containing cell with a read only stream")
                        .with_labels(vec![
                            Label::primary((), range).with_message("remove this Cell<_>")
                        ])
                        .with_notes(vec![
                            "read only streams are safe to use without a Cell".to_owned()
                        ]),
                }
            })
            .collect()
    }

    pub fn print_report(&self) {
        let diagnostics = self.report();
        let writer = StandardStream::stderr(ColorChoice::Always);
        let config = codespan_reporting::term::Config::default();
        let file = SimpleFile::new(self.ast_file.origin.clone(), &self.ast_file.input);
        for diagnostic in &diagnostics {
            codespan_reporting::term::emit(&mut writer.lock(), &config, &file, diagnostic).unwrap();
        }
    }
}
