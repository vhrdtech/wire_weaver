use crate::Span;
use std::fmt::{Debug, Display, Formatter};
use std::rc::Rc;
use util::color;

#[derive(Clone, Eq, PartialEq)]
pub struct Doc {
    pub lines: Vec<(Rc<String>, Span)>,
}

impl Display for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        itertools::intersperse(
            self.lines
                .iter()
                .map(|line| format!("{}{}{}", color::GREEN, line.0, color::DEFAULT)),
            "↩".to_owned(),
        )
            .try_for_each(|s| write!(f, "{}", s))?;
        if !self.lines.is_empty() {
            write!(f, " ")?;
        }
        Ok(())
    }
}

impl Debug for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
