use crate::ast::expr::Expr;
use crate::ast::ty::Ty;
use super::prelude::*;

#[derive(Debug)]
pub struct Generics<'i>(pub Vec<GenericParam<'i>>);

#[derive(Debug)]
pub enum GenericParam<'i> {
    Ty(Ty<'i>),
    Expr(Expr<'i>)
}

impl<'i> Parse<'i> for Generics<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::generics)?, input);
        let mut params = Vec::new();
        while let Some(p) = input.pairs.peek() {
            match p.as_rule() {
                Rule::expression => params.push(GenericParam::Expr(input.parse()?)),
                _ => params.push(GenericParam::Ty(input.parse()?)),
            }
        }
        Ok(Generics(params))
    }
}