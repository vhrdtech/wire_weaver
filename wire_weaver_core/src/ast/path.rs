use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};

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

    pub fn new_path(path: &str) -> Self {
        Path {
            segments: path.split("::").map(|s| Ident::new(s)).collect(),
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
