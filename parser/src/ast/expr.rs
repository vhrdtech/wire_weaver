use super::prelude::*;
use crate::ast::lit::Lit;
use crate::ast::naming::{FnName, Identifier, VariableRefName};
use crate::ast::ops::{BinaryOp, UnaryOp};
use crate::ast::paths::{ResourcePathKind, ResourcePathPart, ResourcePathTail};
use crate::error::{ParseError, ParseErrorKind};
use std::fmt::{Display, Formatter};
use crate::ast::ty::Ty;

/// Expression in S-notation: 1 + 2 * 3 = (+ 1 (* 2 3))
/// Atoms is everything except Cons variant, pre-processed by pest.
#[derive(Debug, Clone)]
pub enum Expr<'i> {
    Call(Identifier<'i, FnName>, CallArguments<'i>),
    IndexInto(Identifier<'i, VariableRefName>, IndexArguments<'i>),
    // CallThenIndexInto(CallArguments<'i>, IndexArguments<'i>),
    // IndexIntoThenCall(IndexArguments<'i>, CallArguments<'i>),
    Lit(Lit<'i>),
    TupleOfExprs,
    Ty(Box<Ty<'i>>),
    Id(Identifier<'i, VariableRefName>),
    ResourcePath {
        kind: ResourcePathKind,
        parts: Vec<ResourcePathPart<'i>>,
        tail: ResourcePathTail<'i>,
    },

    ConsU(UnaryOp, Box<Expr<'i>>),
    ConsB(BinaryOp, Box<(Expr<'i>, Expr<'i>)>),
}

#[derive(Debug, Clone)]
pub struct IndexArguments<'i>(pub Vec<Expr<'i>>);

impl<'i> Parse<'i> for IndexArguments<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::index_arguments)?, input);
        Ok(IndexArguments(input.parse()?))
    }
}

#[derive(Debug, Clone)]
pub struct CallArguments<'i>(pub Vec<Expr<'i>>);

impl<'i> Parse<'i> for CallArguments<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::call_arguments)?, input);
        Ok(CallArguments(input.parse()?))
    }
}

impl<'i> Parse<'i> for Expr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.peek() {
            Some(p) => {
                if p.as_rule() == Rule::expression_ticked {
                    let mut input =
                        ParseInput::fork(input.expect1(Rule::expression_ticked)?, input);
                    let mut input = ParseInput::fork(input.expect1(Rule::expression)?, &mut input);
                    pratt_parser(&mut input, 0)
                } else {
                    let mut input = ParseInput::fork(input.expect1(Rule::expression)?, input);
                    pratt_parser(&mut input, 0)
                }
            }
            None => {
                return Err(ParseErrorSource::UnexpectedInput);
            }
        }
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
            Expr::Call(id, args) => {
                write!(f, "{}({:?})", id.name, args)
            }
            Expr::IndexInto(id, args) => {
                write!(f, "{}[{:?}]", id.name, args)
            }
            // Expr::CallThenIndexInto(call, index) => { write!(f, "call_index") }
            // Expr::IndexIntoThenCall(index, call) => { write!(f, "index_call") }
            Expr::Lit(lit) => {
                write!(f, "{:?}", lit)
            }
            Expr::TupleOfExprs => {
                write!(f, "tuple_of_exprs")
            }
            Expr::Ty(ty) => {
                write!(f, "{:?}", ty)
            }
            Expr::Id(ident) => {
                write!(f, "{}", ident.name)
            }
            Expr::ResourcePath { kind, parts, tail } => {
                write!(f, "{}", kind.to_str())?;
                for part in parts {
                    write!(f, "{}/", part)?;
                }
                write!(f, "{}", tail)
            }

            Expr::ConsU(op, expr) => write!(f, "{}({})", op.to_str(), expr),
            Expr::ConsB(op, a) => {
                write!(f, "({} {} {})", op.to_str(), a.as_ref().0, a.as_ref().1)
            }
        }
    }
}

// Inspired by: https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
fn pratt_parser<'i, 'm>(
    input: &mut ParseInput<'i, 'm>,
    min_bp: u8,
) -> Result<Expr<'i>, ParseErrorSource> {
    let pair = input
        .pairs
        .peek()
        .ok_or_else(|| ParseErrorSource::internal("pratt_parser: expected input"))?;
    let mut lhs = match pair.as_rule() {
        // Atoms
        Rule::call_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            Expr::Call(input.parse()?, input.parse()?)
        }
        Rule::index_into_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            Expr::IndexInto(input.parse()?, input.parse()?)
        }
        Rule::unary_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let op: UnaryOp = input.parse()?;
            let mut input = ParseInput::fork(input.expect1(Rule::expression)?, &mut input);
            Expr::ConsU(op, Box::new(pratt_parser(&mut input, 0)?))
        }
        Rule::any_lit => Expr::Lit(input.parse()?),
        Rule::tuple_of_expressions => {
            return Err(ParseErrorSource::Unimplemented("tuple_of_expressions"))
        }
        Rule::any_ty => Expr::Ty(Box::new(input.parse()?)),
        Rule::identifier => Expr::Id(input.parse()?),
        Rule::resource_path_start => consume_resource_path(input)?,
        Rule::expression_parenthesized => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let mut input = ParseInput::fork(input.expect1(Rule::expression)?, &mut input);
            pratt_parser(&mut input, 0)?
        }

        // Op
        Rule::op_binary => {
            return Err(ParseErrorSource::internal_with_rule(
                pair.as_rule(),
                "pratt_parser: expected atom, got op_binary",
            ));
        }

        _ => {
            return Err(ParseErrorSource::internal_with_rule(
                pair.as_rule(),
                "pratt_parser: expected atom",
            ));
        }
    };

    loop {
        let op = match input.pairs.peek() {
            Some(p) => BinaryOp::from_rule(
                p.into_inner()
                    .next()
                    .ok_or_else(|| ParseErrorSource::internal("pratt_parser: expected binary op"))?
                    .as_rule(),
            )?,
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
        lhs = Expr::ConsB(op, Box::new((lhs, rhs)));
    }

    Ok(lhs)
}

fn consume_resource_path<'i, 'm>(
    input: &mut ParseInput<'i, 'm>,
) -> Result<Expr<'i>, ParseErrorSource> {
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
            Some(p) => match p.as_rule() {
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
                        span: (p.as_span().start(), p.as_span().end()),
                    });
                    return Err(ParseErrorSource::UserError);
                }
            },
            None => {
                return Err(ParseErrorSource::internal("consume_resource_path"));
            }
        }
    }
}

fn finish_resource_path(
    kind: ResourcePathKind,
    tails: Vec<ResourcePathTail>,
) -> Result<Expr, ParseErrorSource> {
    if tails.is_empty() {
        Err(ParseErrorSource::internal(
            "finish_resource_path: empty_tails",
        ))
    } else {
        if tails.len() == 1 {
            Ok(Expr::ResourcePath {
                kind,
                parts: Vec::new(),
                tail: tails[0].clone(),
            })
        } else {
            let mut parts = Vec::new();
            let tails_len = tails.len();
            for (i, t) in tails.into_iter().enumerate() {
                if i != tails_len - 1 {
                    parts.push(t.try_into()?);
                } else {
                    return Ok(Expr::ResourcePath {
                        kind,
                        parts,
                        tail: t.clone(),
                    });
                }
            }
            unreachable!()
        }
    }
}

#[cfg(test)]
mod test {
    use super::Expr;
    use crate::ast::ops::{BinaryOp, UnaryOp};
    use crate::ast::test::parse_str;
    use crate::lexer::Rule;

    #[test]
    fn single_lit() {
        let expr: Expr = parse_str("7", Rule::expression);
        assert!(matches!(expr, Expr::Lit(_)));
    }

    #[test]
    fn not_false() {
        let expr: Expr = parse_str("!false", Rule::expression);
        assert!(matches!(expr, Expr::ConsU(UnaryOp::Not, _)));
        if let Expr::ConsU(_, cons) = expr {
            assert!(matches!(cons.as_ref(), Expr::Lit(_)));
        }
    }

    #[test]
    fn one_plus_two() {
        let expr: Expr = parse_str("1+2", Rule::expression);
        assert!(matches!(expr, Expr::ConsB(BinaryOp::Plus, _)));
        if let Expr::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, Expr::Lit(_)));
            assert!(matches!(cons.as_ref().1, Expr::Lit(_)));
        }
    }

    #[test]
    fn expr_in_paren() {
        let expr: Expr = parse_str("1 * (2 + 3)", Rule::expression);
        assert!(matches!(expr, Expr::ConsB(BinaryOp::Mul, _)));
        if let Expr::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, Expr::Lit(_)));
            assert!(matches!(cons.as_ref().1, Expr::ConsB(BinaryOp::Plus, _)));
        }
    }

    #[test]
    fn call_fn() {
        let expr: Expr = parse_str("fun(1, 2)", Rule::expression);
        assert!(matches!(expr, Expr::Call(_, _)));
        if let Expr::Call(id, args) = expr {
            assert_eq!(id.name, "fun");
            assert_eq!(args.0.len(), 2);
            assert!(matches!(args.0[0], Expr::Lit(_)));
            assert!(matches!(args.0[1], Expr::Lit(_)));
        }
    }

    #[test]
    fn index_array() {
        let expr: Expr = parse_str("arr[0, 5]", Rule::expression);
        assert!(matches!(expr, Expr::IndexInto(_, _)));
        if let Expr::Call(id, args) = expr {
            assert_eq!(id.name, "arr");
            assert_eq!(args.0.len(), 2);
            assert!(matches!(args.0[0], Expr::Lit(_)));
            assert!(matches!(args.0[1], Expr::Lit(_)));
        }
    }

    #[test]
    fn path_to_xpi_block() {
        let expr: Expr = parse_str("crate::log::#/full", Rule::expression);
        assert!(matches!(expr, Expr::ConsB(BinaryOp::Path, _)));
        if let Expr::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, Expr::ConsB(BinaryOp::Path, _)));
            if let Expr::ConsB(_, cons) = &cons.as_ref().0 {
                assert!(matches!(cons.as_ref().0, Expr::Id(_)));
                assert!(matches!(cons.as_ref().1, Expr::Id(_)));
            }
        }
    }

    #[test]
    fn associated_const_of_ty() {
        let expr: Expr = parse_str("u32::MAX", Rule::expression);
        println!("{:?}", expr);
        assert!(matches!(expr, Expr::ConsB(BinaryOp::Path, _)));
        if let Expr::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, Expr::Ty(_)));
            assert!(matches!(cons.as_ref().1, Expr::Id(_)));
        }
    }

    #[test]
    fn associated_const_of_generic_ty() {
        let expr: Expr = parse_str("Ty<1,2>::MAX", Rule::expression);
        assert!(matches!(expr, Expr::ConsB(BinaryOp::Path, _)));
        if let Expr::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, Expr::Ty(_)));
            assert!(matches!(cons.as_ref().1, Expr::Id(_)));
        }
    }

    // #[test]
    // fn call_fn_then_index_result() {
    //     let expr: Expr = parse_str("fun(1)[2]", Rule::expression);
    //     assert!(matches!(expr, Expr::CallThenIndexInto));
    // }
    //
    // #[test]
    // fn index_array_and_call() {
    //     let expr: Expr = parse_str("arr[0](1)", Rule::expression);
    //     assert!(matches!(expr, Expr::IndexIntoThenCall));
    // }
}
