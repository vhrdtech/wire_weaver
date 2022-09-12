use std::fmt::{Display, Formatter};
use parser::ast::doc::Doc as ParserDoc;
use parser::span::Span;
use std::rc::Rc;
use termion::{color, style};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Doc {
    pub lines: Vec<(Rc<String>, Span)>,
}

impl<'i> From<ParserDoc<'i>> for Doc {
    fn from(pd: ParserDoc<'i>) -> Self {
        Doc {
            lines: pd
                .lines
                .iter()
                .map(|l| (Rc::new(String::from(l.0)), l.1.clone().into()))
                .collect(),
        }
    }
}

impl Display for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for l in &self.lines {
            writeln!(f, "{}{}{}", color::Fg(color::Green), l.0, style::Reset)?;
        }
        Ok(())
    }
}