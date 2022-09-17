use pest::iterators::Pair;
use crate::ast::expr::Expr;
use super::prelude::*;
use crate::ast::naming::PathSegment;
use crate::lexer::Rule;

#[derive(Debug, Clone)]
pub struct Attrs<'i> {
    pub attributes: Vec<Attr<'i>>,
}

impl<'i> Parse<'i> for Attrs<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut attributes = Vec::new();
        while let Some(a) = input.pairs.peek() {
            if a.as_rule() == Rule::outer_attribute || a.as_rule() == Rule::inner_attribute {
                let a = input.pairs.next().unwrap();
                ParseInput::fork(a, input)
                    .parse()
                    .map(|attr| attributes.push(attr))?;
            } else {
                break;
            }
        }
        Ok(Attrs { attributes })
    }
}

#[derive(Debug, Clone)]
pub struct Attr<'i> {
    pub path: Vec<Identifier<'i, PathSegment>>,
    pub kind: AttrKind<'i>,
}

#[derive(Debug, Clone)]
pub enum AttrKind<'i> {
    TokenTree(Pair<'i, Rule>),
    Expression(Expr<'i>),
}

impl<'i> Parse<'i> for Attr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let (simple_path, attr_input) = input.expect2(Rule::simple_path, Rule::attribute_input)?;

        let mut path_segments = Vec::new();
        for segment in simple_path.into_inner() {
            ParseInput::fork(segment, input).parse().map(|s| path_segments.push(s))?;
        }

        let mut attr_input = ParseInput::fork(attr_input, input);
        let kind = match attr_input.pairs.peek() {
            Some(p) => {
                if p.as_rule() == Rule::expression {
                    AttrKind::Expression(attr_input.parse()?)
                } else {
                    AttrKind::TokenTree(attr_input.pairs.next().unwrap())
                }
            }
            None => {
                return Err(ParseErrorSource::internal("wrong attribute grammar"));
            }
        };

        Ok(Attr {
            path: path_segments,
            kind,
        })
    }
}
