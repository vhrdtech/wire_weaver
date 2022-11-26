use super::prelude::*;
use crate::ast::generics::GenericsParse;
use crate::ast::stmt::VecStmtParse;
use crate::ast::ty::TyParse;
use ast::{FnArg, FnArguments, FnDef};

pub struct FnDefParse(pub FnDef);

pub struct FnArgumentsParse(pub FnArguments);

pub struct FnArgParse(pub FnArg);

impl<'i> Parse<'i> for FnDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::def_fn, "FnDefParse")?, input);
        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let name: IdentifierParse<identifier::FnName> = input.parse()?;
        let generics: Option<GenericsParse> = input.parse_or_skip()?;
        let arguments: FnArgumentsParse = input.parse()?;
        let ret_ty: Option<TyParse> = input.parse_or_skip()?;
        let statements: VecStmtParse = input.parse()?;
        Ok(FnDefParse(FnDef {
            doc: doc.0,
            attrs: attrs.0,
            name: name.0,
            generics: generics.map(|g| g.0),
            arguments: arguments.0,
            ret_ty: ret_ty.map(|ty| ty.0),
            statements: statements.0,
        }))
    }
}

impl<'i> Parse<'i> for FnArgumentsParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::fn_args, "FnArgumentsParse")?, input);

        let mut args = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let fn_arg: FnArgParse = input.parse()?;
            args.push(fn_arg.0);
        }
        Ok(FnArgumentsParse(FnArguments { args }))
    }
}

impl<'i> Parse<'i> for FnArgParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::named_ty, "FnArgParse")?, input);
        let name: IdentifierParse<identifier::FnArgName> = input.parse()?;
        let ty: TyParse = input.parse()?;
        Ok(FnArgParse(FnArg {
            name: name.0,
            ty: ty.0,
        }))
    }
}
