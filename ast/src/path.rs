use crate::{Identifier, Span};
use std::collections::VecDeque;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Path {
    pub segments: VecDeque<PathSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PathSegment {
    pub ident: Identifier,
    pub index: Option<u32>,
}

impl Path {
    pub fn new() -> Self {
        Path {
            segments: VecDeque::new(),
        }
    }

    pub fn append(&mut self, segment: Identifier) {
        self.segments.push_back(PathSegment {
            ident: segment,
            index: None,
        });
    }

    pub fn pop_front(&mut self) -> Option<Identifier> {
        self.segments.pop_front().map(|s| s.ident)
    }

    pub fn is_from_crate(&self) -> bool {
        if self.segments.is_empty() {
            false
        } else {
            let s0 = &self.segments[0];
            s0.ident.symbols.as_str() == "crate" && s0.index.is_none()
        }
    }

    pub fn is_from_super(&self) -> bool {
        if self.segments.is_empty() {
            false
        } else {
            let s0 = &self.segments[0];
            s0.ident.symbols.as_str() == "super" && s0.index.is_none()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn span(&self) -> Span {
        if self.is_empty() {
            return Span::call_site();
        }
        let mut sum_span = self.segments[0].ident.span.clone();
        for segment in self.segments.iter().skip(1) {
            sum_span = sum_span + segment.ident.span.clone();
        }
        sum_span
    }

    pub fn as_string(&self) -> String {
        format!("{}", self)
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

#[macro_export]
macro_rules! make_path {
    ($($path:ident)::+) => {
        {
            let mut segments = std::collections::VecDeque::new();
            $(
                let ident = ast::Identifier {
                    symbols: std::rc::Rc::new(stringify!($path).to_owned()),
                    context: ast::IdentifierContext::MakePath,
                    span: ast::Span::call_site()
                };
                segments.push_back(ast::PathSegment { ident, index: None });
            )+
            ast::Path { segments }
        }
    }
}
pub use make_path;

impl Display for PathSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.index {
            None => write!(f, "{:-}", self.ident),
            Some(idx) => write!(f, "{:-}[{}]", self.ident, idx),
        }
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: replace with standard function when it is stabilized
        itertools::intersperse(
            self.segments.iter().map(|s| format!("{}", s)),
            "::".to_owned(),
        )
        .try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}
