use crate::ast::generics::Generics;
use crate::ast::naming::{FnArgName, FnName};
use crate::ast::stmt::Stmt;
use crate::ast::ty::Ty;
use super::prelude::*;

#[derive(Debug, Clone)]
pub struct DefFn<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub name: Identifier<'i, FnName>,
    pub generics: Option<Generics<'i>>,
    pub arguments: FnArguments<'i>,
    pub ret_ty: Option<FnRetTy<'i>>,
    pub statements: FnStmts<'i>,
}

impl<'i> Parse<'i> for DefFn<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::def_fn)?, input);

        Ok(DefFn {
            docs: input.parse()?,
            attrs: input.parse()?,
            name: input.parse()?,
            generics: input.parse_or_skip()?,
            arguments: input.parse()?,
            ret_ty: input.parse_or_skip()?,
            statements: input.parse()?
        })
    }
}

#[derive(Debug, Clone)]
pub struct FnArguments<'i> {
    pub args: Vec<FnArg<'i>>
}

impl<'i> Parse<'i> for FnArguments<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::fn_args)?, input);

        let mut args = Vec::new();
        while let Some(_) = input.pairs.peek() {
            args.push(input.parse()?);
        }
        Ok(FnArguments {
            args
        })
    }
}

#[derive(Debug, Clone)]
pub struct FnArg<'i> {
    pub name: Identifier<'i, FnArgName>,
    pub ty: Ty<'i>,
}

impl<'i> Parse<'i> for FnArg<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::named_ty)?, input);
        Ok(FnArg {
            name: input.parse()?,
            ty: input.parse()?
        })
    }
}

#[derive(Debug, Clone)]
pub struct FnStmts<'i> {
    pub stmts: Vec<Stmt<'i>>
}

impl<'i> Parse<'i> for FnStmts<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut stmts = Vec::new();
        while let Some(_) = input.pairs.peek() {
            stmts.push(input.parse()?);
        }
        Ok(FnStmts {
            stmts
        })
    }
}

#[derive(Debug, Clone)]
pub struct FnRetTy<'i>(pub Ty<'i>);

impl<'i> Parse<'i> for FnRetTy<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::fn_ret_ty)?, input);
        Ok(FnRetTy(input.parse()?))
    }
}
