use pest::{Position, Span};
use crate::lexer::{Lexer, Rule};

#[derive(Debug, Clone)]
pub enum LLItem<'i> {
    Punct(char, Position<'i>),
    Str(Span<'i>)
}

#[derive(Debug, Clone)]
pub struct LLFile<'i> {
    pub items: Vec<LLItem<'i>>,
}

#[derive(Debug, Clone)]
pub enum DelimError {
    UnexpectedClosing {
        delim: char,
        line_col: (usize, usize)
    },
    Unmatched {
        open_delim: char,
        open_line_col: (usize, usize),
        close_delim: char,
        close_line_col: (usize, usize),
    },
    ClosingNotFound {
        open_delim: char,
        open_line_col: (usize, usize),
    }
}

impl<'i> LLFile<'i> {
    pub fn parse(input: &'i str) -> Self {
        let mut pairs = <Lexer as pest::Parser<Rule>>::parse(Rule::file_ll, input)
            .expect("Rule::file_ll shouldn't fail");
        let mut items = Vec::new();
        for p in pairs.next().expect("Wrong file_ll grammar rule").into_inner() {
            match p.as_rule() {
                Rule::file_ll_item => {
                    match p.clone().into_inner().peek() {
                        Some(p) => {
                            match p.as_rule() {
                                Rule::doc_comment => {}, // ignore doc comments
                                Rule::punct_ll => {
                                    items.push(LLItem::Punct(
                                        p.as_str().chars().next().expect("Wrong punct_ll rule"),
                                        p.as_span().start_pos()
                                    ));
                                },
                                _ => unreachable!()
                            }
                        }
                        None => {
                            items.push(LLItem::Str(p.as_span()));
                        }
                    }
                },
                _ => unreachable!()
            }
        }
        LLFile {
            items
        }
    }

    pub fn check_delimiters(&self) -> Result<(), DelimError> {
        let mut delims = vec![];
        for i in &self.items {
            if let LLItem::Punct(c, pos) = i {
                // TODO: handle < > better to not false positive on comparison operators
                if *c == '{' || *c == '(' || *c == '[' || *c == '<' {
                    delims.push((c, pos));
                } else {
                    match delims.last() {
                        Some((c_open, open_pos)) => {
                            if is_matching_delim(**c_open, *c) {
                                delims.remove(delims.len() - 1);
                            } else {
                                return Err(DelimError::Unmatched {
                                    open_delim: **c_open,
                                    open_line_col: open_pos.line_col(),
                                    close_delim: *c,
                                    close_line_col: pos.line_col()
                                })
                            }
                        },
                        None => {
                            return Err(DelimError::UnexpectedClosing { delim: *c, line_col: pos.line_col() })
                        }
                    }
                }
            }
        }

        match delims.last() {
            Some((delim, pos)) => {
               Err(DelimError::ClosingNotFound {
                   open_delim: **delim,
                   open_line_col: pos.line_col()
               })
            },
            None => Ok(())
        }
    }
}

fn is_matching_delim(first: char, second: char) -> bool {
    match first {
        '{' => second == '}',
        '(' => second == ')',
        '[' => second == ']',
        '<' => second == '>',
        _ => unreachable!()
    }
}