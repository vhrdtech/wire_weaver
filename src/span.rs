use std::fmt::{Debug, Formatter};
use std::rc::Rc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub origin: SpanOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpanOrigin {
    Parser(SourceOrigin),
    Coder
}

#[derive(Clone, Eq, PartialEq)]
pub enum SourceOrigin {
    File(Rc<String>),
    Registry(/*RegistryUri*/),
    DescriptorBlock(/*NodeUid*/),

    /// Replaced with actual origin after whole tree is created
    ImplFrom,
}

impl Debug for SourceOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceOrigin::File(_) => write!(f, "file"),
            SourceOrigin::Registry() => write!(f, "registry"),
            SourceOrigin::DescriptorBlock() => write!(f, "descriptor"),
            SourceOrigin::ImplFrom => write!(f, "impl From")
        }
    }
}

impl<'i> From<parser::pest::Span<'i>> for Span {
    fn from(s: parser::pest::Span<'i>) -> Self {
        Span {
            start: s.start(),
            end: s.end(),
            origin: SpanOrigin::Parser(SourceOrigin::ImplFrom)
        }
    }
}