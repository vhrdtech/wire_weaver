use crate::ast::docs::Docs;
use crate::ast::ty::Type;
use crate::ast::util::Version;
use proc_macro2::Ident;
use syn::Expr;

#[derive(Clone, Debug)]
pub(crate) struct ItemStruct {
    pub(crate) docs: Docs,
    // pub(crate) ident: Ident,
    pub(crate) fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(crate) docs: Docs,
    pub(crate) _id: u32,
    pub(crate) ident: Ident,
    pub(crate) ty: Type,
    pub(crate) _since: Option<Version>,
    pub(crate) default: Option<Expr>,
}

impl ItemStruct {
    pub(crate) fn make_owned(&mut self) {
        for f in &mut self.fields {
            f.ty.make_owned();
        }
    }
}
