use std::cmp::{max, min};
use std::fmt::{Debug, Display, Formatter};
use std::ops::Add;
use std::path::PathBuf;
use std::rc::Rc;

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
            origin: SpanOrigin::Coder,
        }
    }
}

impl Add<Span> for Span {
    type Output = Span;

    fn add(self, rhs: Span) -> Self::Output {
        assert_eq!(self.origin, rhs.origin);
        Span {
            start: min(self.start, rhs.start),
            end: max(self.end, rhs.end),
            origin: self.origin.clone(),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum SpanOrigin {
    Parser(SourceOrigin),
    Coder, // TODO: { origin: Box(SpanOrigin) } // preserve origin chain
}

#[derive(Clone, Eq, PartialEq)]
pub enum SourceOrigin {
    File(Rc<PathBuf>),
    // TODO: use Rc?
    Registry(/*RegistryUri*/),
    DescriptorBlock(/*NodeUid*/),

    /// Replaced with actual origin after whole tree is created
    Pest,

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
                        path.file_name()
                            .map(|p| p.to_str().unwrap_or("?"))
                            .unwrap_or("?")
                    )
                }
            }
            SourceOrigin::Registry() => write!(f, "registry"),
            SourceOrigin::DescriptorBlock() => write!(f, "descriptor"),
            SourceOrigin::Pest => write!(f, "impl From"),
            SourceOrigin::Str => write!(f, "str"),
        }
    }
}

impl Debug for SourceOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
