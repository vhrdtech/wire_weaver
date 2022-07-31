use crate::ast::lit::Lit;
use crate::ast::ops::BinaryOp;
use super::prelude::*;

/// Expression in S-notation: 1 + 2 * 3 = (+ 1 (* 2 3))
/// Atoms is everything except Cons variant, pre-processed by pest.
#[derive(Debug)]
pub enum Expr<'i> {
    Call,
    IndexInto,
    Unary,
    Lit(Lit<'i>),
    TupleOfExprs,
    Ident(&'i str),
    ResourcePathStart,
    ExprInParen,

    Cons(BinaryOp, Vec<Expr<'i>>)
}

impl<'i> Parse<'i> for Expr<'i> {
    fn parse<'m>(mut input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        crate::util::pest_print_tree(input.pairs.clone());
        let mut input = ParseInput::fork(input.expect1(Rule::expression)?, input);
        pratt_parser(&mut input, 0)
    }
}

// Inspired by: https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
fn pratt_parser<'i, 'm>(input: &mut ParseInput<'i, 'm>, min_bp: u8) -> Result<Expr<'i>, ParseErrorSource> {
    let pair = input.pairs.peek().ok_or_else(|| ParseErrorSource::internal())?;
    // println!("lhs pair = {:?}", pair);
    println!("pratt(_, {})", min_bp);
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
            return Err(ParseErrorSource::Unimplemented("resource_path_start"))
        }
        Rule::expression_parenthesized => {
            return Err(ParseErrorSource::Unimplemented("expression_parenthesized"))
        }

        // Op
        Rule::op_binary => {
            return Err(ParseErrorSource::internal_with_rule(pair.as_rule()));
        }

        _ => {
            return Err(ParseErrorSource::internal_with_rule(pair.as_rule()));
        }
    };
    println!("lhs = {:?}", lhs);

    loop {
        println!("loop start {:?}", input.pairs);
        let op = match input.pairs.peek() {
            Some(p) => {
                BinaryOp::from_rule(p
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParseErrorSource::internal())?
                    .as_rule()
                )?
            }
            None => {
                println!("break on eof");
                break;
            }
        };
        println!("op = {:?}", op);

        let (l_bp, r_bp) = op.binding_power();
        if l_bp < min_bp {
            println!("l_bp < min_bp => breaking");
            // do not consume op and break
            break;
        }
        let _ = input.pairs.next(); // consume op
        let rhs = pratt_parser(input, r_bp)?;
        lhs = Expr::Cons(op, vec![lhs, rhs]);
        println!("reassign lhs = {:?}", lhs);
    }

    println!("ret lhs = {:?}", lhs);
    Ok(lhs)
}