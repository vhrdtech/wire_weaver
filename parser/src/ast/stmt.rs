use crate::ast::expr::Expr;
use crate::ast::naming::LetStmtName;
use crate::ast::ty::Ty;
use super::prelude::*;

#[derive(Debug)]
pub enum Stmt<'i> {
    Let(LetStmt<'i>),
    Expr(Expr<'i>)
}

impl<'i> Parse<'i> for Stmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::statement)?, input);
        let s = input.pairs.next().ok_or_else(|| ParseErrorSource::UnexpectedInput)?;
        match s.as_rule() {
            Rule::let_stmt => Ok(Stmt::Let(input.parse()?)),
            Rule::expr_stmt => {
                let mut input = ParseInput::fork(s, &mut input);
                Ok(Stmt::Expr(input.parse()?))
            },
            _ => {
                Err(ParseErrorSource::internal_with_rule(s.as_rule()))
            }
        }
    }
}

#[derive(Debug)]
pub struct LetStmt<'i> {
    pub ident: LetStmtName<'i>,
    pub type_ascription: Option<Ty<'i>>,
    pub expr: Expr<'i>
}

impl<'i> Parse<'i> for LetStmt<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::let_stmt)?, input);
        Ok(LetStmt {
            ident: input.parse()?,
            type_ascription: input.parse_or_skip()?,
            expr: input.parse()?
        })
    }
}