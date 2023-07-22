use super::prelude::*;
use ast::identifier::IdentifierContext;
use ast::Identifier;
use pest::iterators::Pair;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::rc::Rc;

pub struct IdentifierParse<K>(pub Identifier, PhantomData<K>);

macro_rules! identifier_context {
    ($context: ident) => {
        #[derive(Clone, Debug)]
        pub struct $context {}
        impl sealed::IdentifierContextParse for $context {
            fn context() -> IdentifierContext {
                IdentifierContext::$context
            }
        }
    };
}

identifier_context!(TyAlias);
identifier_context!(BuiltinTyName);
identifier_context!(PathSegment);
identifier_context!(XpiUriSegmentName);
identifier_context!(XpiKeyName);
identifier_context!(FnName);
identifier_context!(FnArgName);
identifier_context!(VariableDefName);
identifier_context!(VariableRefName);
identifier_context!(StructTyName);
identifier_context!(StructFieldName);
identifier_context!(EnumTyName);
identifier_context!(EnumFieldName);
identifier_context!(GenericName);
//
// #[derive(Clone, Debug)]
// pub struct XpiUriSegmentName;
//
// impl<'i, K: sealed::IdentifierKind + Debug> Debug for Identifier<'i, K> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         write!(
//             f,
//             "Id<{:?}>(\x1b[35m{}\x1b[0m @{}:{})",
//             self.kind,
//             self.name,
//             self.span.start(),
//             self.span.end()
//         )
//     }
// }
// impl<'i> Debug for Identifier<'i, XpiUriSegmentName> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         write!(
//             f,
//             "Id<XpiUriSegmentName>(\x1b[35m{}\x1b[0m @{}:{})",
//             self.name,
//             self.span.start(),
//             self.span.end()
//         )
//     }
// }

mod sealed {
    pub trait IdentifierContextParse {
        fn context() -> ast::IdentifierContext;
    }
}

//
// impl<'i, K: sealed::IdentifierKind> Parse<'i> for Identifier<'i, K> {
//     fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Identifier<'i, K>, ParseErrorSource> {
//         let ident = input.expect1(Rule::identifier)?;
//         Ok(Identifier {
//             name: ident.as_str(),
//             span: ident.as_span(),
//             kind: K::new(),
//         })
//     }
// }
impl<'i, K: sealed::IdentifierContextParse> Parse<'i> for IdentifierParse<K> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<IdentifierParse<K>, ParseErrorSource> {
        let p = input.expect1_either(
            Rule::identifier,
            Rule::identifier_continue,
            "IdentifierParse",
        )?;
        Ok(IdentifierParse(
            Identifier {
                symbols: Rc::new(p.as_str().to_owned()),
                context: K::context(),
                span: ast_span_from_pest(p.as_span()),
            },
            PhantomData,
        ))
    }
}

// use sealed::IdentifierKind;
impl<'i, K: sealed::IdentifierContextParse> From<Pair<'i, crate::lexer::Rule>>
    for IdentifierParse<K>
{
    fn from(p: Pair<'i, crate::lexer::Rule>) -> Self {
        IdentifierParse(
            Identifier {
                symbols: Rc::new(p.as_str().to_owned()),
                context: K::context(),
                span: ast_span_from_pest(p.as_span()),
            },
            PhantomData,
        )
    }
}
