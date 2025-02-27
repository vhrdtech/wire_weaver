use crate::ast::ItemConst;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub fn const_def(item_const: &ItemConst) -> TokenStream {
    let ident: Ident = (&item_const.ident).into();
    let ty = item_const.ty.def(true);
    let expr = &item_const.value;
    quote! {
        pub const #ident: #ty = #expr;
    }
}
