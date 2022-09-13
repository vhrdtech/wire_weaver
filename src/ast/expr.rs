use std::fmt::{Display, Formatter};
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

/// Expression that is eventually expected to be a literal
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TryEvaluateInto<F, T> {
    NotResolved(F),
    Resolved(T),
    Error
}

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

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Call { method, args } => {
                write!(f, "{}({})", method, args)
            }
            Expr::Index { object, by } => {
                write!(f, "{}({})", object, by)
            }
            Expr::Lit(lit) => {
                write!(f, "{}", lit)
            }
            Expr::Tuple(exprs) => {
                write!(f, "{}", exprs)
            }
            Expr::Id(ident) => {
                write!(f, "{}", ident)
            }

            Expr::ConsU(op, expr) => write!(f, "{}({})", op.to_str(), expr),
            Expr::ConsB(op, a) => {
                write!(f, "({} {} {})", op.to_str(), a.as_ref().0, a.as_ref().1)
            }
        }
    }
}

impl Display for VecExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|expr| write!(f, "{}, ", expr))
    }
}

impl<F: Display, T: Display> Display for TryEvaluateInto<F, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TryEvaluateInto::NotResolved(from) => write!(f, "NR({})", from),
            TryEvaluateInto::Resolved(to) => write!(f, "R({})", to),
            TryEvaluateInto::Error => write!(f, "ER()"),
        }
    }
}