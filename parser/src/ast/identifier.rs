use super::prelude::*;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::rc::Rc;
use ast::identifier::IdentifierContext;

#[derive(Clone)]
pub struct Identifier<K>(pub ast::Identifier, PhantomData<K>);

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
impl<'i, K: sealed::IdentifierContextParse> Parse<'i> for Identifier<K> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Identifier<K>, ParseErrorSource> {
        let ident = match input.pairs.peek() {
            Some(p) => match p.as_rule() {
                Rule::identifier | Rule::identifier_continue => { // TODO: handle ident_continue better?
                    let _ = input.pairs.next();
                    p
                }
                _ => {
                    return Err(ParseErrorSource::UnexpectedInput);
                }
            },
            None => {
                return Err(ParseErrorSource::UnexpectedInput);
            }
        };
        Ok(Identifier(ast::Identifier {
            symbols: Rc::new(ident.as_str().to_owned()),
            context: K::context(),
            span: ast_span_from_pest(ident.as_span()),
        }, PhantomData))
    }
}

// use sealed::IdentifierKind;
// impl<'i> From<Pair<'i, crate::lexer::Rule>> for Identifier<'i, UserTyName> {
//     fn from(p: Pair<'i, crate::lexer::Rule>) -> Self {
//         Identifier {
//             name: p.as_str(),
//             span: p.as_span(),
//             kind: UserTyName::new(),
//         }
//     }
// }
