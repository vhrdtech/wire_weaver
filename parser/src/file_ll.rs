use std::fmt::{Display, Formatter};
use pest::{Position, Span};
use crate::lexer::{Lexer, Rule};
use crate::span::SpanOrigin;

#[derive(Debug, Clone)]
pub enum LLItem<'i> {
    Punct(char, Position<'i>),
    Str(Span<'i>)
}

#[derive(Debug, Clone)]
pub struct LLFile<'i> {
    pub items: Vec<LLItem<'i>>,
    pub origin: SpanOrigin,
}

#[derive(Debug, Clone)]
pub struct DelimError {
    origin: SpanOrigin,
    kind: DelimErrorKind
}

#[derive(Debug, Clone)]
pub enum DelimErrorKind {
    UnexpectedClosing {
        delim: char,
        line_col: (usize, usize),
        line: String,
    },
    Unmatched {
        open_delim: char,
        open_line_col: (usize, usize),
        open_line: String,
        close_delim: char,
        close_line_col: (usize, usize),
        close_line: String,
    },
    ClosingNotFound {
        open_delim: char,
        open_line_col: (usize, usize),
        open_line: String,
    }
}

impl<'i> LLFile<'i> {
    pub fn parse(input: &'i str, origin: SpanOrigin) -> Self {
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
            items,
            origin
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
                                return Err(DelimError { kind: DelimErrorKind::Unmatched {
                                    open_delim: **c_open,
                                    open_line_col: open_pos.line_col(),
                                    open_line: open_pos.line_of().to_owned(),
                                    close_delim: *c,
                                    close_line_col: pos.line_col(),
                                    close_line: pos.line_of().to_owned()
                                }, origin: self.origin.clone()})
                            }
                        },
                        None => {
                            return Err(DelimError {kind:DelimErrorKind::UnexpectedClosing {
                                delim: *c,
                                line_col: pos.line_col(),
                                line: pos.line_of().to_owned()
                            }, origin: self.origin.clone()})
                        }
                    }
                }
            }
        }

        match delims.last() {
            Some((delim, pos)) => {
               Err(DelimError{kind:DelimErrorKind::ClosingNotFound {
                   open_delim: **delim,
                   open_line_col: pos.line_col(),
                   open_line: pos.line_of().to_owned(),
               }, origin: self.origin.clone()})
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

impl DelimError {
    pub fn format(&self) -> String {
        let spacing = self.spacing();
        let origin = format!("{:#}", self.origin);
        match &self.kind {
            DelimErrorKind::UnexpectedClosing { delim, line_col, line } => {
                format!(
                    "\x1b[31m\x1b[1merror\x1b[39m: unmatched delimiters\x1b[0m\n\
                     {s    }{b}-->{r} {p}:{l}:{c}\n\
                     {s    }{b} |{r}\n\
                     {b}{l:w$ } |{r} {line}\
                     {s    }{b} |{r} {underline}\n\
                     {s    }{b} |{r}\n\
                     {s    }{b} = \x1b[39mnote:{r} Unexpected closing delimiter '{cd}'",
                    s = spacing,
                    p = origin,
                    l = line_col.0,
                    c = line_col.1,
                    w = spacing.len(),
                    line = line,
                    underline = Self::underline(line.as_str(), line_col.1),
                    cd = delim,
                    b = "\x1b[34m\x1b[1m",
                    r = "\x1b[0m"
                )
            }
            DelimErrorKind::Unmatched { open_delim, open_line_col, open_line, close_delim, close_line_col, close_line } => {
                format!(
                    "\x1b[31m\x1b[1merror\x1b[39m: unmatched delimiters\x1b[0m\n\
                     {s    }{b}-->{r} {p}:{ol}:{oc}\n\
                     {s    }{b} |{r}\n\
                     {b}{ol:w$} |{r} {oline}\
                     {s    }{b} |{r} {o_underline}\n\
                     {s    }{b} |{r} ...\n\
                     {b}{cl:w$} |{r} {cline}\
                     {s    }{b} |{r} {c_underline}\n\
                     {s    }{b} |{r}\n\
                     {s    }{b} = \x1b[39mnote:{r} Open delimiter '{od}' is closed with incorrect '{cd}'",
                    s = spacing,
                    p = origin,
                    ol = open_line_col.0,
                    oc = open_line_col.1,
                    cl = close_line_col.0,
                    w = spacing.len(),
                    oline = open_line,
                    cline = close_line,
                    o_underline = Self::underline(open_line.as_str(), open_line_col.1),
                    c_underline = Self::underline(close_line.as_str(), close_line_col.1),
                    od = open_delim,
                    cd = close_delim,
                    b = "\x1b[34m\x1b[1m",
                    r = "\x1b[0m"
                )
            }
            DelimErrorKind::ClosingNotFound { open_delim, open_line_col, open_line } => {
                format!(
                    "\x1b[31m\x1b[1merror\x1b[39m: unmatched delimiters\x1b[0m\n\
                     {s    }{b}-->{r} {p}:{l}:{c}\n\
                     {s    }{b} |{r}\n\
                     {b}{l:w$ } |{r} {line}\
                     {s    }{b} |{r} {underline}\n\
                     {s    }{b} |{r}\n\
                     {s    }{b} = \x1b[39mnote:{r} Found open delimiter: '{od}', but no closing one for it",
                    s = spacing,
                    p = origin,
                    l = open_line_col.0,
                    c = open_line_col.1,
                    w = spacing.len(),
                    line = open_line,
                    underline = Self::underline(open_line.as_str(), open_line_col.1),
                    od = open_delim,
                    b = "\x1b[34m\x1b[1m",
                    r = "\x1b[0m"
                )
            }
        }
    }

    fn spacing(&self) -> String {
        let line = match self.kind {
            DelimErrorKind::UnexpectedClosing { line_col, .. } => line_col.0,
            DelimErrorKind::Unmatched { close_line_col, .. } => close_line_col.0,
            DelimErrorKind::ClosingNotFound { open_line_col, .. } => open_line_col.0,
        };

        let line_str_len = format!("{}", line).len();

        let mut spacing = String::new();
        for _ in 0..line_str_len {
            spacing.push(' ');
        }

        spacing
    }

    fn underline(line: &str, pos: usize) -> String {
        let mut underline = String::new();

        for c in line.chars().take(pos - 1) {
            match c {
                '\t' => underline.push('\t'),
                _ => underline.push(' '),
            }
        }

        underline.push_str("^---");

        underline
    }
}

impl Display for DelimError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format())
    }
}