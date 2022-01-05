use proc_macro2::{TokenStream};
use quote::{quote, TokenStreamExt, ToTokens};
use parser::ast::item::Docs;

pub struct CGDocs<'i, 'c> {
    pub inner: &'c Docs<'i>
}

impl<'i, 'c> ToTokens for CGDocs<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let lines = &self.inner.lines;
        tokens.append_all(quote! {
            #(#[doc = #lines])*
        });
    }
}