use std::fmt::{Display, Formatter};
use crate::ast::lit::Lit;
use crate::ast::ops::BinaryOp;
use crate::ast::paths::{ResourcePathKind, ResourcePathPart, ResourcePathTail};
use crate::error::{ParseError, ParseErrorKind};
use super::prelude::*;

/// Expression in S-notation: 1 + 2 * 3 = (+ 1 (* 2 3))
/// Atoms is everything except Cons variant, pre-processed by pest.
#[derive(Debug, Clone)]
pub enum Expr<'i> {
    Call,
    IndexInto,
    Unary,
    Lit(Lit<'i>),
    TupleOfExprs,
    Ident(&'i str),
    ResourcePath {
        kind: ResourcePathKind,
        parts: Vec<ResourcePathPart<'i>>,
        tail: ResourcePathTail<'i>,
    },
    ExprInParen,

    Cons(BinaryOp, Vec<Expr<'i>>)
}

impl<'i> Parse<'i> for Expr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::expression)?, input);
        pratt_parser(&mut input, 0)
    }
}

impl<'i> Parse<'i> for Vec<Expr<'i>> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut exprs = Vec::new();
        while let Some(_) = input.pairs.peek() {
            exprs.push(input.parse()?);
        }
        Ok(exprs)
    }
}

impl<'i> Display for Expr<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Call => { write!(f, "call") }
            Expr::IndexInto => { write!(f, "index_into") }
            Expr::Unary => { write!(f, "unary") }
            Expr::Lit(lit) => { write!(f, "{:?}", lit) }
            Expr::TupleOfExprs => { write!(f, "tuple_of_exprs") }
            Expr::Ident(ident) => { write!(f, "{}", *ident) }
            Expr::ResourcePath {
                kind, parts, tail
            } => {
                write!(f, "{}", kind.to_str())?;
                for part in parts {
                    write!(f, "{}", part)?;
                }
                write!(f, "{}", tail)
            }
            Expr::ExprInParen => { write!(f, "expr_in_paren") }

            Expr::Cons(op, cons) => {
                write!(f, "({}", op.to_str())?;
                for e in cons {
                    write!(f, " {}", e)?;
                }
                write!(f, ")")
            }
        }
    }
}

// Inspired by: https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
fn pratt_parser<'i, 'm>(input: &mut ParseInput<'i, 'm>, min_bp: u8) -> Result<Expr<'i>, ParseErrorSource> {
    let pair = input.pairs.peek().ok_or_else(|| ParseErrorSource::internal(""))?;
    let mut lhs = match pair.as_rule() {
        // Atoms
        Rule::call_expr => {
            return Err(ParseErrorSource::Unimplemented("call_expr"))
        }
        Rule::index_into_expr => {
            return Err(ParseErrorSource::Unimplemented("index_into_expr"))
        }
        Rule::unary_expr => {
            return Err(ParseErrorSource::Unimplemented("unary_expr"))
        }
        Rule::any_lit => {
            Expr::Lit(input.parse()?)
        }
        Rule::tuple_of_expressions => {
            return Err(ParseErrorSource::Unimplemented("tuple_of_expressions"))
        }
        Rule::identifier => {
            let _ = input.pairs.next();
            Expr::Ident(pair.as_str())
        }
        Rule::resource_path_start => {
            consume_resource_path(input)?
        }
        Rule::expression_parenthesized => {
            return Err(ParseErrorSource::Unimplemented("expression_parenthesized"))
        }

        // Op
        Rule::op_binary => {
            return Err(ParseErrorSource::internal_with_rule(pair.as_rule(), ""));
        }

        _ => {
            return Err(ParseErrorSource::internal_with_rule(pair.as_rule(), ""));
        }
    };

    loop {
        let op = match input.pairs.peek() {
            Some(p) => {
                BinaryOp::from_rule(p
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParseErrorSource::internal(""))?
                    .as_rule()
                )?
            }
            None => {
                break;
            }
        };

        let (l_bp, r_bp) = op.binding_power();
        if l_bp < min_bp {
            // do not consume op and break
            break;
        }
        let _ = input.pairs.next(); // consume op
        let rhs = pratt_parser(input, r_bp)?;
        lhs = Expr::Cons(op, vec![lhs, rhs]);
    }

    Ok(lhs)
}

fn consume_resource_path<'i, 'm>(input: &mut ParseInput<'i, 'm>) -> Result<Expr<'i>, ParseErrorSource> {
    let kind: ResourcePathKind = input.parse()?;
    let mut tails = Vec::new();
    loop {
        match input.pairs.peek() {
            Some(p) => {
                if p.as_rule() != Rule::op_binary || p.as_str() != "/" {
                    return finish_resource_path(kind, tails);
                } else {
                    let _ = input.pairs.next();
                }
            }
            None => {
                return finish_resource_path(kind, tails);
            }
        }

        match input.pairs.peek() {
            Some(p) => {
                match p.as_rule() {
                    Rule::identifier => {
                        tails.push(ResourcePathTail::Reference(input.parse()?));
                    }
                    Rule::index_into_expr => {
                        tails.push(ResourcePathTail::IndexInto(input.parse()?));
                    }
                    Rule::call_expr => {
                        tails.push(ResourcePathTail::Call(input.parse()?));
                    }
                    _ => {
                        input.errors.push(ParseError {
                            kind: ParseErrorKind::MalformedResourcePath,
                            rule: p.as_rule(),
                            span: (p.as_span().start(), p.as_span().end())
                        });
                        return Err(ParseErrorSource::UserError);
                    }
                }
            }
            None => {
                return Err(ParseErrorSource::internal("consume_resource_path"));
            }
        }
    }
}

fn finish_resource_path(kind: ResourcePathKind, tails: Vec<ResourcePathTail>) -> Result<Expr, ParseErrorSource> {
    if tails.is_empty() {
        Err(ParseErrorSource::internal("finish_resource_path: empty_tails"))
    } else {
        if tails.len() == 1 {
            Ok(Expr::ResourcePath {
                kind,
                parts: Vec::new(),
                tail: tails[0].clone()
            })
        } else {
            let mut parts = Vec::new();
            let tails_len = tails.len();
            for (i, t) in tails.into_iter().enumerate() {
                if i != tails_len - 1 {
                    parts.push(t.try_into()?);
                } else {
                    return Ok(Expr::ResourcePath {
                        kind, parts, tail: t.clone()
                    })
                }
            };
            unreachable!()
        }
    }
}