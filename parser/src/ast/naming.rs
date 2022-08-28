use std::fmt::{Debug, Formatter};
use pest::iterators::Pair;
use pest::Span;
use super::prelude::*;

#[derive(Clone)]
pub struct Identifier<'i, K> {
    pub name: &'i str,
    pub span: Span<'i>,
    pub kind: K
}

macro_rules! identifier_kind {
    ($kind: ident) => {
        #[derive(Clone, Debug)]
        pub struct $kind {}
        impl sealed::IdentifierKind for $kind {
            fn new() -> Self {
                Self {}
            }
        }
    }
}

identifier_kind!(UserTyName);
identifier_kind!(BuiltinTyName);
identifier_kind!(PathSegment);
identifier_kind!(XpiUriSegmentName);
identifier_kind!(XpiKeyName);
identifier_kind!(FnName);
identifier_kind!(FnArgName);
identifier_kind!(VariableDefName);
identifier_kind!(VariableRefName);
identifier_kind!(StructTyName);
identifier_kind!(StructFieldName);
identifier_kind!(EnumTyName);
identifier_kind!(EnumFieldName);
identifier_kind!(GenericName);

impl<'i, K: sealed::IdentifierKind + Debug> Debug for Identifier<'i, K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Id<{:?}>(\x1b[35m{}\x1b[0m @{}:{})", self.kind, self.name, self.span.start(), self.span.end())
    }
}

mod sealed {
    pub trait IdentifierKind {
        fn new() -> Self;
    }
}

impl<'i, K: sealed::IdentifierKind> Parse<'i> for Identifier<'i, K> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Identifier<'i, K>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(Identifier {
            name: ident.as_str(),
            span: ident.as_span(),
            kind: K::new()
        })
    }
}

use sealed::IdentifierKind;
impl<'i> From<Pair<'i, crate::lexer::Rule>> for Identifier<'i, UserTyName> {
    fn from(p: Pair<'i, crate::lexer::Rule>) -> Self {
        Identifier {
            name: p.as_str(),
            span: p.as_span(),
            kind: UserTyName::new()
        }
    }
}