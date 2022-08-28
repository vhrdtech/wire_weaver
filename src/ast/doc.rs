use std::rc::Rc;
use parser::ast::doc::Doc as ParserDoc;
use crate::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Doc {
    pub lines: Vec<(Rc<String>, Span)>,
}

impl<'i> From<ParserDoc<'i>> for Doc {
    fn from(pd: ParserDoc<'i>) -> Self {
        Doc {
            lines: pd.lines
                .iter()
                .map(|l| (Rc::new(String::from(l.0)), l.1.clone().into()))
                .collect()
        }
    }
}