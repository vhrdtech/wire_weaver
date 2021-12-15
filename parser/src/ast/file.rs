use crate::parse::{ParseInput};
use crate::ast::item::Item;
use crate::lexer::{Lexer, Rule};
use pest::error::Error;
use std::fmt::{Display, Formatter};
use crate::error::ParseError;
use crate::warning::ParseWarning;


#[derive(Debug)]
pub enum FileError {
    Lexer(pest::error::Error<Rule>),
    Parser(Vec<ParseError>)
}

impl From<pest::error::Error<Rule>> for FileError {
    fn from(e: Error<Rule>) -> Self {
        FileError::Lexer(e)
    }
}

impl Display for FileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Lexer(le) => {
                write!(f, "{}", le)
            }
            FileError::Parser(pe) => {
                write!(f, "{:?}", pe)
            }
        }
    }
}

#[derive(Debug)]
pub struct File<'i> {
    pub items: Vec<Item<'i>>
}

impl<'i> File<'i> {
    fn parse(input: &'i str) -> Result<(Self, Vec<ParseWarning>), FileError> {
        let mut pi = <Lexer as pest::Parser<Rule>>::parse(Rule::file, input)?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        // println!("File::parse: {:?}", pi);
        match pi.next() { // Rule::file
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    println!("next file item: {:?}", p);
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
                            // println!("deferring to others {:?}", pair);
                            match ParseInput::new(pair.into_inner(), &mut warnings, &mut errors).parse().map(|item| items.push(item)) {
                                Ok(_) => {},
                                Err(()) => {
                                    if errors.is_empty() {
                                        errors.push(ParseError::E0002);
                                    }
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
            Err(FileError::Parser(errors))
        }
    }
}

#[cfg(test)]
mod test {
    use super::File;

    #[test]
    fn test_simple() {
        let input = r#"
        /// Doc line 1
        /// Doc line 2
        enum FrameId {
            /// Doc for std
            Standard(u11, bool),
            /// Doc for ext
            Extended(u29)
        }"#;
        let file= match File::parse(input) {
            Ok(file) => file,
            Err(e) => {
                println!("{}", e);
                return;
            }
        };
        println!("Warnings: {:?}", file.1);
        println!("File: {}", file.0);
    }
}