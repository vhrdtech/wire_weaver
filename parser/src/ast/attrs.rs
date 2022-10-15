use ast::attribute::{Attr, AttrKind};
use ast::Attrs;
use crate::ast::expr::ExprParse;
use crate::ast::paths::PathParse;
use super::prelude::*;
use crate::lexer::Rule;

#[derive(Debug, Clone)]
pub struct AttrsParse(pub Attrs);

impl<'i> Parse<'i> for AttrsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut attrs = Vec::new();
        while let Some(a) = input.pairs.peek() {
            if a.as_rule() == Rule::outer_attribute || a.as_rule() == Rule::inner_attribute {
                let a = input.pairs.next().unwrap();
                let mut input = ParseInput::fork(a, input);
                let attr: AttrParse = input.parse()?;
                attrs.push(attr.0);
            } else {
                break;
            }
        }
        Ok(AttrsParse(Attrs { attrs, span: ast_span_from_pest(input.span.clone()) }))
    }
}

#[derive(Debug, Clone)]
pub struct AttrParse(pub Attr);

#[derive(Debug, Clone)]
pub struct AttrKindParse(pub AttrKind);

impl<'i> Parse<'i> for AttrParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let path: PathParse = input.parse()?;

        let attr_input = input.expect1(Rule::attribute_input)?;
        let mut attr_input = ParseInput::fork(attr_input, input);
        let kind = match attr_input.pairs.peek() {
            Some(p) => {
                if p.as_rule() == Rule::expression {
                    let expr: ExprParse = attr_input.parse()?;
                    AttrKind::Expr(expr.0)
                } else {
                    AttrKind::TT(())
                }
            }
            None => {
                return Err(ParseErrorSource::internal("wrong attribute grammar"));
            }
        };

        Ok(AttrParse(Attr {
            path: path.0,
            kind,
            span: ast_span_from_pest(input.span.clone()),
        }))
    }
}
