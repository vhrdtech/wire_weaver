use std::fmt::{Debug, Display, Formatter};
use std::rc::Rc;
use util::color;
use crate::Span;

#[derive(Clone, Eq, PartialEq)]
pub struct Doc {
    pub lines: Vec<(Rc<String>, Span)>,
}

impl Display for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (i, l) in self.lines.iter().enumerate() {
            write!(f, "{}{}{}", color::GREEN, l.0, color::DEFAULT)?;
            if i < self.lines.len() - 1 {
                write!(f, "↩")?;
            }
        }
        Ok(())
    }
}

impl Debug for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}