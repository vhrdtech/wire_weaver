use std::collections::VecDeque;
use std::fmt::{Display, Formatter};
use crate::Identifier;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Path {
    pub segments: VecDeque<Identifier>,
}

impl Path {
    pub fn new() -> Self {
        Path {
            segments: VecDeque::new()
        }
    }

    pub fn append(&mut self, segment: Identifier) {
        self.segments.push_back(segment);
    }

    pub fn pop_front(&mut self) -> Option<Identifier> {
        self.segments.pop_front()
    }

    pub fn is_from_crate(&self) -> bool {
        if self.segments.is_empty() {
            false
        } else {
            self.segments[0].symbols.as_str() == "crate"
        }
    }

    pub fn is_from_super(&self) -> bool {
        if self.segments.is_empty() {
            false
        } else {
            self.segments[0].symbols.as_str() == "super"
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[macro_export]
macro_rules! make_path {
    ($($path:ident)::+) => {
        {
            let mut segments = std::collections::VecDeque::new();
            $( segments.push_back(ast::Identifier {
                symbols: std::rc::Rc::new(stringify!($path).to_owned()),
                context: ast::IdentifierContext::MakePath,
                span: ast::Span::call_site()
            });)+
            ast::Path { segments }
        }
    }
}
pub use make_path;

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: replace with standard function when it is stabilized
        itertools::intersperse(
            self.segments.iter().map(|elem| format!("{:-}", elem)),
            "::".to_owned(),
        ).try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResourcePathMarker {
    FromRoot,
    FromParent,
    FromSelf,
}

impl ResourcePathMarker {
    pub fn to_str(&self) -> &'static str {
        match self {
            ResourcePathMarker::FromRoot => "#",
            ResourcePathMarker::FromParent => "#..",
            ResourcePathMarker::FromSelf => "#.",
        }
    }
}
