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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceOrigin {
    File(Rc<String>),
    Registry(/*RegistryUri*/),
    DescriptorBlock(/*NodeUid*/),

    /// Replaced with actual origin after whole tree is created
    ImplFrom,
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