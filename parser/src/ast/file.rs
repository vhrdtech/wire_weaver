use crate::parse::{ParseInput, Parse};
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
pub struct File {
    pub items: Vec<Item>
}

impl File {
    fn parse(input: &str) -> Result<(Self, Vec<ParseWarning>), FileError> {
        let mut pi = <Lexer as pest::Parser<Rule>>::parse(Rule::file, input)?;
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        match pi.next() {
            Some(pair) => {
                let mut pi = pair.into_inner();
                while let Some(p) = pi.peek() {
                    match p.as_rule() {
                        Rule::inner_attribute => {
                            let attr = pi.next();
                            println!("inner attribute found: {:?}", attr);
                        },
                        Rule::EOI => {
                            break;
                        },
                        _ => {
                            let pair = pi.next().unwrap();
                            println!("deferring to others {:?}", pair);
                            ParseInput::new(pair, &mut warnings, &mut errors).parse().map(|item| items.push(item));
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
    use crate::lexer::{Lexer, Rule};
    // use crate::pest::Parser;
    use crate::parse::{Parse, ParseInput};

    #[test]
    fn test_simple() {
        let input = r#"
        #![inner]

        ///Docs
        ///Docs more
        #[outer1]
        #[outer2]
        enum FrameId {
            /// Std doc
            /// Std doc 2
            #[std_outer]
            #[std_outer2]
            Standard(u11),

            /// Ext doc
            /// Ext doc 2
            #[ext_outer]
            #[ext_outer2]
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
        println!("File: {:?}", file.0);
    }
}