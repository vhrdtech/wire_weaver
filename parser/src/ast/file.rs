use crate::parse::{ParseInput};
use crate::ast::item::Item;
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

#[derive(Debug)]
pub struct File<'i> {
    pub items: Vec<Item<'i>>
}

impl<'i> File<'i> {
    pub fn parse(input: &'i str) -> Result<(Self, Vec<ParseWarning>), FileError> {
        let mut pi = <Lexer as pest::Parser<Rule>>::parse(Rule::file, input)?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        // println!("File::parse: {:?}", pi);
        match pi.next() { // Rule::file
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    println!("next file item: {:?}", p.as_rule());
                    match p.as_rule() {
                        Rule::inner_attribute => {
                            let attr = pi.next();
                            println!("inner attribute found: {:?}", attr);
                        },
                        Rule::EOI => {
                            break;
                        },
                        // Rule::COMMENT => {}
                        _ => {
                            let pair = pi.next().unwrap();
                            let rule = pair.as_rule();
                            let span = (pair.as_span().start(), pair.as_span().end());
                            // println!("deferring to others {:?}", pair);
                            match ParseInput::new(pair.into_inner(), &mut warnings, &mut errors).parse() {
                                Ok(item) => {
                                    items.push(item);
                                },
                                Err(e) => {
                                    let kind = match e {
                                        ParseErrorSource::InternalError => ParseErrorKind::InternalError,
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
            Ok((File {
                items
            }, warnings))
        } else {
            Err(FileError::ParserError(errors))
        }
    }
}