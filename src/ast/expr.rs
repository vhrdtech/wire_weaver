use std::fmt::{Display, Formatter};
use crate::ast::identifier::Identifier;
use crate::ast::lit::Lit;
use parser::ast::expr::{CallArguments, Expr as ExprParser, IndexArguments};
use parser::ast::ops::{BinaryOp, UnaryOp};
use std::ops::Deref;
use parser::span::Span;
use crate::ast::path::Path;
use crate::error::{Error, ErrorKind};

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

impl Expr {
    pub fn expect_ident(&self) -> Result<Identifier, Error> {
        match self {
            Expr::Id(ident) => Ok(ident.clone()),
            _ => Err(Error::new(
                ErrorKind::ExprExpectedToBe("Id".to_owned(), self.format_kind()),
                self.span(),
            ))
        }
    }

    pub fn expect_call(&self) -> Result<(Identifier, VecExpr), Error> {
        match self {
            Expr::Call { method, args } => Ok((method.clone(), args.clone())),
            _ => Err(Error::new(
                ErrorKind::ExprExpectedToBe("Call".to_owned(), self.format_kind()),
                self.span(),
            ))
        }
    }

    pub fn expect_path(&self) -> Result<Path, Error> {
        let mut path = Path::new();
        Self::expect_path_inner(self, &mut path)?;
        Ok(path)
    }

    fn expect_path_inner(expr: &Expr, path: &mut Path) -> Result<(), Error> {
        match &expr {
            Expr::ConsB(op, cons) => {
                if *op == BinaryOp::Path {
                    Self::expect_path_inner(&cons.deref().0, path)?;
                    Self::expect_path_inner(&cons.deref().1, path)?;
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::ExprExpectedToBe("Path".to_owned(), expr.format_kind()),
                        expr.span())
                    )
                }
            }
            Expr::Id(ident) => {
                path.items.push(ident.clone());
                Ok(())
            }
            _ => Err(Error::new(
                ErrorKind::ExprExpectedToBe("Path".to_owned(), expr.format_kind()),
                expr.span())
            )
        }
    }

    pub fn format_kind(&self) -> String {
        match self {
            Expr::Call { .. } => "Call",
            Expr::Index { .. } => "Index",
            Expr::Lit(_) => "Lit",
            Expr::Tuple(_) => "Tuple",
            Expr::Id(_) => "Ident",
            Expr::ConsU(_, _) => "Unary",
            Expr::ConsB(_, _) => "Binary",
        }.to_owned()
    }

    pub fn span(&self) -> Span {
        match self {
            Expr::Call { method, args } => method.span.clone() + args.span(),
            Expr::Index { object, by } => object.span.clone() + by.span(),
            Expr::Lit(lit) => lit.span.clone(),
            Expr::Tuple(t) => t.span(),
            Expr::Id(id) => id.span.clone(),
            Expr::ConsU(_, cons) => cons.span(),
            Expr::ConsB(_, cons) => cons.deref().0.span() + cons.deref().1.span(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecExpr(pub Vec<Expr>);

impl VecExpr {
    pub fn span(&self) -> Span {
        if self.0.is_empty() {
            panic!("VecExpr::span() called on empty");
        }
        self.0
            .iter()
            .skip(1)
            .fold(self.0[0].span().clone(), |prev, expr| prev + expr.span())
    }
}

/// Expression that is eventually expected to be a literal
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TryEvaluateInto<F, T> {
    NotResolved(F),
    Resolved(T),
    Error,
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