use super::prelude::*;
use ast::{Expr, Path};
use std::collections::VecDeque;
use ast::lit::LitKind;
use ast::path::PathSegment;
use crate::ast::expr::VecExprParse;
use crate::error::{ParseError, ParseErrorKind};
use crate::warning::{ParseWarning, ParseWarningKind};

pub struct PathParse(pub Path);

impl<'i> Parse<'i> for PathParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let simple_path = input.expect1(Rule::path, "PathParse")?;
        let mut input = ParseInput::fork(simple_path, input);
        let mut segments = VecDeque::new();
        while let Some(ident_or_index) = input.pairs.peek() {
            let segment = if ident_or_index.as_rule() == Rule::identifier {
                let ident: IdentifierParse<identifier::PathSegment> = input.parse()?;
                PathSegment {
                    ident: ident.0,
                    index: None,
                }
            } else {
                let mut input = ParseInput::fork(input.expect1(Rule::path_index, "PathParse:2")?, &mut input);
                let ident: IdentifierParse<identifier::PathSegment> = input.parse()?;
                let mut input = ParseInput::fork(input.expect1(Rule::index_arguments, "PathParse:3")?, &mut input);
                let index: VecExprParse = input.parse()?;
                let exprs = index.0;
                let span = exprs.span();
                if exprs.0.len() != 1 {
                    input.errors.push(ParseError {
                        kind: ParseErrorKind::WrongIndexInPath("only 1D indexes are supported"),
                        rule: Rule::path,
                        span: span.to_range(),
                    });
                    return Err(ParseErrorSource::UserError);
                }
                let index = if let Expr::Lit(lit) = &exprs.0[0] {
                    if let LitKind::Discrete(discrete) = &lit.kind {
                        println!("{:?}", discrete);
                        if discrete.is_ty_forced {
                            input.warnings.push(ParseWarning {
                                kind: ParseWarningKind::ForcedTyOnPathIndex,
                                rule: Rule::path,
                                span: lit.span.to_range(),
                            });
                        }
                        if discrete.val >= u32::MAX as u128 - 1 {
                            input.errors.push(ParseError {
                                kind: ParseErrorKind::WrongIndexInPath("too big (>= u32::MAX - 1)"),
                                rule: Rule::path,
                                span: span.to_range(),
                            });
                            return Err(ParseErrorSource::UserError);
                        }
                        discrete.val as u32
                    } else {
                        input.errors.push(ParseError {
                            kind: ParseErrorKind::WrongIndexInPath("index in path must be a discrete number"),
                            rule: Rule::path,
                            span: span.to_range(),
                        });
                        return Err(ParseErrorSource::UserError);
                    }
                } else {
                    input.errors.push(ParseError {
                        kind: ParseErrorKind::WrongIndexInPath("index in path must be a discrete number"),
                        rule: Rule::path,
                        span: span.to_range(),
                    });
                    return Err(ParseErrorSource::UserError);
                };
                PathSegment {
                    ident: ident.0,
                    index: Some(index),
                }
            };
            segments.push_back(segment);
        }
        Ok(PathParse(Path { segments }))
    }
}
