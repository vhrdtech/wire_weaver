use crate::ast::identifier::Identifier;
use crate::ast::ty::Ty;
use parser::ast::def_fn::{
    FnArg as FnArgParser,
    FnArguments as FnArgumentsParser,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FnArguments {
    pub args: Vec<FnArg>
}

impl<'i> From<FnArgumentsParser<'i>> for FnArguments {
    fn from(args: FnArgumentsParser<'i>) -> Self {
        FnArguments {
            args: args.args.iter().map(|a| a.clone().into()).collect()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FnArg {
    pub name: Identifier,
    pub ty: Ty
}

impl<'i> From<FnArgParser<'i>> for FnArg {
    fn from(arg: FnArgParser<'i>) -> Self {
        FnArg {
            name: arg.name.into(),
            ty: arg.ty.into()
        }
    }
}