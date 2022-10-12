use std::fmt::{Debug, Display, Formatter};
use std::rc::Rc;
use util::color;
use crate::Span;

#[derive(Clone, Eq, PartialEq)]
pub struct Doc {
    pub lines: Vec<(Rc<String>, Span)>,
}

// impl<'i> From<ParserDoc<'i>> for Doc {
//     fn from(pd: ParserDoc<'i>) -> Self {
//         Doc {
//             lines: pd
//                 .lines
//                 .iter()
//                 .map(|l| (Rc::new(String::from(l.0)), l.1.clone().into()))
//                 .collect(),
//         }
//     }
// }

impl Display for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for l in &self.lines {
            writeln!(f, "{}{}{}", color::GREEN, l.0, color::DEFAULT)?;
        }
        Ok(())
    }
}

impl Debug for Doc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}