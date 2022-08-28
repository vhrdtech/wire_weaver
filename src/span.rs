use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub origin: SpanOrigin,
}

#[derive(Clone, Eq, PartialEq)]
pub enum SpanOrigin {
    Parser(SourceOrigin),
    Coder
}

#[derive(Clone, Eq, PartialEq)]
pub enum SourceOrigin {
    File(Rc<PathBuf>),
    Registry(/*RegistryUri*/),
    DescriptorBlock(/*NodeUid*/),

    /// Replaced with actual origin after whole tree is created
    ImplFrom,
}

impl Display for SpanOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SpanOrigin::Parser(origin) => write!(f, "Parser:{}", origin),
            SpanOrigin::Coder => write!(f, "Coder:"),
        }
    }
}
impl Debug for SpanOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}:{}", self.start, self.end)
        } else {
            write!(f, "{}:{}:{}", self.origin, self.start, self.end)
        }
    }
}
impl Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for SourceOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceOrigin::File(path) => write!(
                f,
                "file:{}",
                path.file_name().map(|p| p.to_str().unwrap_or("?")).unwrap_or("?")),
            SourceOrigin::Registry() => write!(f, "registry"),
            SourceOrigin::DescriptorBlock() => write!(f, "descriptor"),
            SourceOrigin::ImplFrom => write!(f, "impl From")
        }
    }
}
impl Debug for SourceOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
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