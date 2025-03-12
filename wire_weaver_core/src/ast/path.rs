use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};

use crate::ast::ident::Ident;

#[derive(Clone, Debug)]
pub struct Path {
    pub segments: Vec<Ident>,
    // arguments
}

impl Path {
    pub fn new_ident(ident: Ident) -> Self {
        Path {
            segments: vec![ident],
        }
    }
}

impl ToTokens for &Path {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let segments = self
            .segments
            .iter()
            .map(|ident| proc_macro2::Ident::new(ident.sym.as_str(), Span::call_site()));
        tokens.append_all(quote! { #(#segments)::* })
    }
}
