use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;

#[derive(Clone, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub origin: SpanOrigin,
}

impl Span {
    pub fn call_site() -> Self {
        Span {
            start: 0,
            end: 0,
            origin: SpanOrigin::Coder
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum SpanOrigin {
    Parser(SourceOrigin),
    Coder
}

#[derive(Clone, Eq, PartialEq)]
pub enum SourceOrigin {
    File(PathBuf),
    Registry(/*RegistryUri*/),
    DescriptorBlock(/*NodeUid*/),

    /// Replaced with actual origin after whole tree is created
    ImplFrom,

    Str,
}

impl Display for SpanOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            match self {
                SpanOrigin::Parser(origin) => write!(f, "Parser:{:#}", origin),
                SpanOrigin::Coder => write!(f, "Coder:"),
            }
        } else {
            match self {
                SpanOrigin::Parser(origin) => write!(f, "Parser:{}", origin),
                SpanOrigin::Coder => write!(f, "Coder:"),
            }
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
        if f.sign_minus() {
            write!(f, "{}:{}", self.start, self.end)
        } else if f.alternate() {
            write!(f, "{:#}:{}:{}", self.origin, self.start, self.end)
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
            SourceOrigin::File(path) => {
                if f.alternate() {
                    write!(f, "file:{}", path.to_str().unwrap_or("?"))
                } else {
                    write!(
                        f,
                        "file:{}",
                        path.file_name().map(|p| p.to_str().unwrap_or("?")).unwrap_or("?")
                    )
                }
            },
            SourceOrigin::Registry() => write!(f, "registry"),
            SourceOrigin::DescriptorBlock() => write!(f, "descriptor"),
            SourceOrigin::ImplFrom => write!(f, "impl From"),
            SourceOrigin::Str => write!(f, "str")
        }
    }
}
impl Debug for SourceOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl<'i> From<pest::Span<'i>> for Span {
    fn from(s: pest::Span<'i>) -> Self {
        Span {
            start: s.start(),
            end: s.end(),
            origin: SpanOrigin::Parser(SourceOrigin::ImplFrom)
        }
    }
}