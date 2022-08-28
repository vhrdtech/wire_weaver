use crate::parse::{ParseInput};
use crate::ast::definition::Definition;
use crate::lexer::{Lexer, Rule};
use pest::error::Error;
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use crate::warning::ParseWarning;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum FileError {
    Lexer(pest::error::Error<Rule>),
    ParserError(Vec<ParseError>)
}

impl From<pest::error::Error<Rule>> for FileError {
    fn from(e: Error<Rule>) -> Self {
        FileError::Lexer(e)
    }
}

impl Display for FileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for FileError {}

#[derive(Debug, Clone)]
pub struct File<'i> {
    pub defs: Vec<Definition<'i>>,
    pub warnings: Vec<ParseWarning>,
}

impl<'i> File<'i> {
    pub fn parse(input: &'i str) -> Result<Self, FileError> {
        let mut pi = <Lexer as pest::Parser<Rule>>::parse(Rule::file, input)?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        // println!("File::parse: {:?}", pi);
        match pi.next() { // Rule::file
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    // println!("next file item: {:?}", p.as_rule());
                    match p.as_rule() {
                        Rule::inner_attribute => {
                            let attr = pi.next();
                            println!("inner attribute found: {:?}", attr);
                        },
                        Rule::EOI => {
                            break;
                        },
                        // silent in rules
                        // Rule::COMMENT => {
                        //     let _ = pi.next();
                        // }
                        _ => {
                            let pair = pi.next().unwrap();
                            let pair_span = pair.as_span();
                            let rule = pair.as_rule();
                            let span = (pair.as_span().start(), pair.as_span().end());
                            // println!("deferring to others {:?}", pair);
                            match ParseInput::new(pair.into_inner(), pair_span, &mut warnings, &mut errors).parse() {
                                Ok(item) => {
                                    items.push(item);
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
                                }
                            }
                        }
                    }
                }
            },
            None => {}
        }
        if errors.is_empty() {
            Ok(File {
                defs: items,
                warnings
            })
        } else {
            Err(FileError::ParserError(errors))
        }
    }
}