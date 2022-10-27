use ast::{Expr, VecExpr};
use super::prelude::*;
use crate::ast::lit::LitParse;
use crate::ast::ops::{binary_from_rule, UnaryOpParse};
use crate::ast::paths::PathParse;
use crate::ast::ty::TyParse;

pub struct ExprParse(pub Expr);

pub struct VecExprParse(pub VecExpr);

impl<'i> Parse<'i> for ExprParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.peek() {
            Some(p) => {
                if p.as_rule() == Rule::expression_ticked {
                    let mut input =
                        ParseInput::fork(input.expect1(Rule::expression_ticked)?, input);
                    let mut input = ParseInput::fork(input.expect1(Rule::expression)?, &mut input);
                    pratt_parser(&mut input, 0).map(|expr| ExprParse(expr))
                } else {
                    let mut input = ParseInput::fork(input.expect1(Rule::expression)?, input);
                    pratt_parser(&mut input, 0).map(|expr| ExprParse(expr))
                }
            }
            None => {
                return Err(ParseErrorSource::UnexpectedInput);
            }
        }
    }
}


impl<'i> Parse<'i> for VecExprParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut exprs = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let expr: ExprParse = input.parse()?;
            exprs.push(expr.0);
        }
        Ok(VecExprParse(VecExpr(exprs)))
    }
}

// Inspired by: https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
fn pratt_parser(input: &mut ParseInput, min_bp: u8) -> Result<Expr, ParseErrorSource> {
    let pair = input
        .pairs
        .peek()
        .ok_or_else(|| ParseErrorSource::internal("pratt_parser: expected input"))?;
    let mut lhs = match pair.as_rule() {
        // Atoms
        Rule::call_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let method: PathParse = input.parse()?;
            let mut input = ParseInput::fork(input.expect1(Rule::call_arguments)?, &mut input);
            let args: VecExprParse = input.parse()?;
            Expr::Call { method: method.0, args: args.0 }
        }
        Rule::index_into_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let object: PathParse = input.parse()?;
            let by: VecExprParse = input.parse()?;
            Expr::Index { object: object.0, by: by.0 }
        }
        Rule::unary_expr => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let op: UnaryOpParse = input.parse()?;
            let mut input = ParseInput::fork(input.expect1(Rule::expression)?, &mut input);
            Expr::ConsU(op.0, Box::new(pratt_parser(&mut input, 0)?))
        }
        Rule::lit => {
            let lit: LitParse = input.parse()?;
            Expr::Lit(lit.0)
        },
        Rule::tuple_of_expressions => {
            let _ = input.pairs.next();
            let mut input = ParseInput::fork(pair, input);
            let mut exprs = vec![];
            while let Some(_) = input.pairs.peek() {
                let expr: ExprParse = input.parse()?;
                exprs.push(expr.0);
            }
            Expr::Tuple(VecExpr(exprs))
        }
        Rule::ty => {
            let ty: TyParse = input.parse()?;
            Expr::Ty(Box::new(ty.0))
        },
        Rule::path => {
            let path: PathParse = input.parse()?;
            Expr::Ref(path.0)
        },
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
            Some(p) => binary_from_rule(
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

// fn consume_resource_path<'i, 'm>(
//     input: &mut ParseInput<'i, 'm>,
// ) -> Result<Expr<'i>, ParseErrorSource> {
//     let kind: ResourcePathKind = input.parse()?;
//     let mut tails = Vec::new();
//     loop {
//         match input.pairs.peek() {
//             Some(p) => {
//                 if p.as_rule() != Rule::op_binary || p.as_str() != "/" {
//                     return finish_resource_path(kind, tails);
//                 } else {
//                     let _ = input.pairs.next();
//                 }
//             }
//             None => {
//                 return finish_resource_path(kind, tails);
//             }
//         }
//
//         match input.pairs.peek() {
//             Some(p) => match p.as_rule() {
//                 Rule::identifier => {
//                     tails.push(ResourcePathTail::Reference(input.parse()?));
//                 }
//                 Rule::index_into_expr => {
//                     tails.push(ResourcePathTail::IndexInto(input.parse()?));
//                 }
//                 Rule::call_expr => {
//                     tails.push(ResourcePathTail::Call(input.parse()?));
//                 }
//                 _ => {
//                     input.errors.push(ParseError {
//                         kind: ParseErrorKind::MalformedResourcePath,
//                         rule: p.as_rule(),
//                         span: (p.as_span().start(), p.as_span().end()),
//                     });
//                     return Err(ParseErrorSource::UserError);
//                 }
//             },
//             None => {
//                 return Err(ParseErrorSource::internal("consume_resource_path"));
//             }
//         }
//     }
// }
//
// fn finish_resource_path(
//     kind: ResourcePathKind,
//     tails: Vec<ResourcePathTail>,
// ) -> Result<Expr, ParseErrorSource> {
//     if tails.is_empty() {
//         Err(ParseErrorSource::internal(
//             "finish_resource_path: empty_tails",
//         ))
//     } else {
//         if tails.len() == 1 {
//             Ok(Expr::ResourcePath {
//                 kind,
//                 parts: Vec::new(),
//                 tail: tails[0].clone(),
//             })
//         } else {
//             let mut parts = Vec::new();
//             let tails_len = tails.len();
//             for (i, t) in tails.into_iter().enumerate() {
//                 if i != tails_len - 1 {
//                     parts.push(t.try_into()?);
//                 } else {
//                     return Ok(Expr::ResourcePath {
//                         kind,
//                         parts,
//                         tail: t.clone(),
//                     });
//                 }
//             }
//             unreachable!()
//         }
//     }
// }

#[cfg(test)]
mod test {
    use super::ExprParse;
    use crate::ast::ops::{BinaryOpParse, UnaryOpParse};
    use crate::ast::test::parse_str;
    use crate::lexer::Rule;

    #[test]
    fn single_lit() {
        let expr: ExprParse = parse_str("7", Rule::expression);
        assert!(matches!(expr, ExprParse::Lit(_)));
    }

    #[test]
    fn not_false() {
        let expr: ExprParse = parse_str("!false", Rule::expression);
        assert!(matches!(expr, ExprParse::ConsU(UnaryOpParse::Not, _)));
        if let ExprParse::ConsU(_, cons) = expr {
            assert!(matches!(cons.as_ref(), ExprParse::Lit(_)));
        }
    }

    #[test]
    fn one_plus_two() {
        let expr: ExprParse = parse_str("1+2", Rule::expression);
        assert!(matches!(expr, ExprParse::ConsB(BinaryOpParse::Plus, _)));
        if let ExprParse::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, ExprParse::Lit(_)));
            assert!(matches!(cons.as_ref().1, ExprParse::Lit(_)));
        }
    }

    #[test]
    fn expr_in_paren() {
        let expr: ExprParse = parse_str("1 * (2 + 3)", Rule::expression);
        assert!(matches!(expr, ExprParse::ConsB(BinaryOpParse::Mul, _)));
        if let ExprParse::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, ExprParse::Lit(_)));
            assert!(matches!(cons.as_ref().1, ExprParse::ConsB(BinaryOpParse::Plus, _)));
        }
    }

    #[test]
    fn call_fn() {
        let expr: ExprParse = parse_str("fun(1, 2)", Rule::expression);
        assert!(matches!(expr, ExprParse::Call(_, _)));
        if let ExprParse::Call(id, args) = expr {
            assert_eq!(id.name, "fun");
            assert_eq!(args.0.len(), 2);
            assert!(matches!(args.0[0], ExprParse::Lit(_)));
            assert!(matches!(args.0[1], ExprParse::Lit(_)));
        }
    }

    #[test]
    fn index_array() {
        let expr: ExprParse = parse_str("arr[0, 5]", Rule::expression);
        assert!(matches!(expr, ExprParse::IndexInto(_, _)));
        if let ExprParse::Call(id, args) = expr {
            assert_eq!(id.name, "arr");
            assert_eq!(args.0.len(), 2);
            assert!(matches!(args.0[0], ExprParse::Lit(_)));
            assert!(matches!(args.0[1], ExprParse::Lit(_)));
        }
    }

    #[test]
    fn path_to_xpi_block() {
        let expr: ExprParse = parse_str("crate::log::#/full", Rule::expression);
        assert!(matches!(expr, ExprParse::ConsB(BinaryOpParse::Path, _)));
        if let ExprParse::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, ExprParse::ConsB(BinaryOpParse::Path, _)));
            if let ExprParse::ConsB(_, cons) = &cons.as_ref().0 {
                assert!(matches!(cons.as_ref().0, ExprParse::Id(_)));
                assert!(matches!(cons.as_ref().1, ExprParse::Id(_)));
            }
        }
    }

    #[test]
    fn associated_const_of_ty() {
        let expr: ExprParse = parse_str("u32::MAX", Rule::expression);
        let expr = expr.0;
        println!("{:?}", expr);
        assert!(matches!(expr, ExprParse::ConsB(BinaryOpParse::Path, _)));
        if let ExprParse::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, ExprParse::Ty(_)));
            assert!(matches!(cons.as_ref().1, ExprParse::Id(_)));
        }
    }

    #[test]
    fn associated_const_of_generic_ty() {
        let expr: ExprParse = parse_str("Ty<1,2>::MAX", Rule::expression);
        assert!(matches!(expr, ExprParse::ConsB(BinaryOpParse::Path, _)));
        if let ExprParse::ConsB(_, cons) = expr {
            assert!(matches!(cons.as_ref().0, ExprParse::Ty(_)));
            assert!(matches!(cons.as_ref().1, ExprParse::Id(_)));
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
