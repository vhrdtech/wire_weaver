use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Path {
    pub segments: Vec<Ident>,
}

impl Path {
    pub fn new_ident(ident: Ident) -> Self {
        Path {
            segments: vec![ident],
        }
    }

    pub fn new_path(path: &str) -> Self {
        Path {
            segments: path
                .split("::")
                .map(|s| Ident::new(s, Span::call_site()))
                .collect(),
        }
    }

    pub fn make_owned(&mut self) {
        if let Some(last_segment) = self.segments.last_mut() {
            *last_segment =
                Ident::new(format!("{}Owned", last_segment).as_str(), Span::call_site());
        }
    }
}

impl ToTokens for &Path {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let segments = self.segments.iter();
        tokens.append_all(quote! { #(#segments)::* })
    }
}
