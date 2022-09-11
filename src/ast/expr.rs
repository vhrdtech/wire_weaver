use crate::ast::identifier::Identifier;
use crate::ast::lit::Lit;
use parser::ast::expr::{CallArguments, Expr as ExprParser, IndexArguments};
use parser::ast::ops::{BinaryOp, UnaryOp};
use std::ops::Deref;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expr {
    Call { method: Identifier, args: VecExpr },
    Index { object: Identifier, by: VecExpr },
    Lit(Lit),
    Tuple(VecExpr),
    Id(Identifier),

    ConsU(UnaryOp, Box<Expr>),
    ConsB(BinaryOp, Box<(Expr, Expr)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecExpr(pub Vec<Expr>);

impl<'i> From<ExprParser<'i>> for Expr {
    fn from(expr: ExprParser<'i>) -> Self {
        match expr {
            ExprParser::Call(method, args) => Expr::Call {
                method: method.into(),
                args: args.into(),
            },
            ExprParser::IndexInto(object, by) => Expr::Index {
                object: object.into(),
                by: by.into(),
            },
            ExprParser::Lit(lit) => Expr::Lit(lit.into()),
            ExprParser::TupleOfExprs => unimplemented!(),
            ExprParser::Id(id) => Expr::Id(id.into()),

            ExprParser::ConsU(op, expr) => Expr::ConsU(op, Box::new(expr.deref().clone().into())),
            ExprParser::ConsB(op, exprs) => {
                let exprs = exprs.deref();
                Expr::ConsB(
                    op,
                    Box::new((exprs.0.clone().into(), exprs.1.clone().into())),
                )
            }

            _ => unimplemented!(),
        }
    }
}

impl<'i> From<CallArguments<'i>> for VecExpr {
    fn from(args: CallArguments<'i>) -> Self {
        VecExpr(args.0.iter().map(|a| a.clone().into()).collect())
    }
}

impl<'i> From<IndexArguments<'i>> for VecExpr {
    fn from(args: IndexArguments<'i>) -> Self {
        VecExpr(args.0.iter().map(|a| a.clone().into()).collect())
    }
}
