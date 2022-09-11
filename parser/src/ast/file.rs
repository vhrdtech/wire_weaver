use crate::ast::definition::Definition;
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use crate::lexer::{Lexer, Rule};
use crate::parse::ParseInput;
use crate::span::SpanOrigin;
use crate::warning::ParseWarning;
use std::fmt::{Display, Formatter};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct File<'i> {
    pub defs: Vec<Definition<'i>>,
    pub warnings: Vec<ParseWarning>,
    pub origin: SpanOrigin,
}

#[derive(Error, Debug)]
pub struct FileError {
    pub kind: FileErrorKind,
    pub origin: SpanOrigin,
    pub input: String,
}

#[derive(Error, Debug)]
pub enum FileErrorKind {
    #[error("Source contains syntax errors")]
    Lexer(pest::error::Error<Rule>),
    #[error("Source contains structural errors")]
    Parser(Vec<ParseError>),
}

impl<'i> File<'i> {
    pub fn parse(input: &'i str, origin: SpanOrigin) -> Result<Self, FileError> {
        let mut pi =
            <Lexer as pest::Parser<Rule>>::parse(Rule::file, input).map_err(|e| FileError {
                kind: FileErrorKind::Lexer(e),
                origin: origin.clone(),
                input: input.to_owned(),
            })?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        // println!("File::parse: {:?}", pi);
        match pi.next() {
            // Rule::file
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    match p.as_rule() {
                        Rule::inner_attribute => {
                            let attr = pi.next();
                            println!("inner attribute found: {:?}", attr);
                        }
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
                            match ParseInput::new(
                                pair.into_inner(),
                                pair_span,
                                &mut warnings,
                                &mut errors,
                            )
                            .parse()
                            {
                                Ok(item) => {
                                    items.push(item);
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
                                        ParseErrorSource::UnexpectedInput => {
                                            ParseErrorKind::UnhandledUnexpectedInput
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
            Ok(File {
                defs: items,
                warnings,
                origin: origin.clone(),
            })
        } else {
            Err(FileError {
                kind: FileErrorKind::Parser(errors),
                origin: origin.clone(),
                input: input.to_owned(),
            })
        }
    }

    pub fn parse_tree(
        input: &'i str,
        def_name: &str,
        origin: SpanOrigin,
    ) -> Result<Option<String>, FileError> {
        let mut pi =
            <Lexer as pest::Parser<Rule>>::parse(Rule::file, input).map_err(|e| FileError {
                kind: FileErrorKind::Lexer(e),
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
}

impl Display for FileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            FileErrorKind::Lexer(pest_err) => {
                let ll_file =
                    crate::file_ll::LLFile::parse(self.input.as_str(), self.origin.clone());
                match ll_file.check_delimiters() {
                    Ok(()) => {
                        // TODO: colorize pest error in the same way
                        writeln!(
                            f,
                            " --> {}\n\x1b[31m{}\x1b[0m",
                            self.origin,
                            pest_err
                                .clone()
                                .renamed_rules(|r| crate::user_readable::rule_names(r))
                        )
                    }
                    Err(e) => {
                        // Input contains unmatched delimiters, display extensive information about them
                        writeln!(f, "{}", e)
                    }
                }
            }
            FileErrorKind::Parser(parser_errors) => {
                writeln!(f, " --> {}\n{:?}", self.origin, parser_errors)
            }
        }
    }
}
