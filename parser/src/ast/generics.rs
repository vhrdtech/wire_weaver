use super::prelude::*;
use crate::ast::expr::ExprParse;
use crate::ast::ty::TyParse;
use ast::Generics;

pub struct GenericsParse(pub Generics);

impl<'i> Parse<'i> for GenericsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::generics, "GenericsParse")?, input);
        let mut params = Vec::new();
        while let Some(p) = input.pairs.peek() {
            match p.as_rule() {
                Rule::expression => {
                    let expr: ExprParse = input.parse()?;
                    params.push(ast::generics::GenericParam::Expr(expr.0))
                }
                _ => {
                    let ty: TyParse = input.parse()?;
                    params.push(ast::generics::GenericParam::Ty(ty.0))
                }
            }
        }
        Ok(GenericsParse(Generics { params }))
    }
}
