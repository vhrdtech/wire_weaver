use std::fmt::{Display, Formatter};
use crate::Identifier;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path {
    pub segments: Vec<Identifier>,
}

impl Path {
    pub fn new() -> Self {
        Path {
            segments: vec![]
        }
    }
}

#[macro_export]
macro_rules! make_path {
    ($($path:ident)::+) => {
        Path {
            segments: vec![ $( ast::Identifier {
                symbols: std::rc::Rc::new(stringify!($path).to_owned()),
                context: ast::IdentifierContext::PathSegment,
                span: ast::Span::call_site()
            }),+]
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
