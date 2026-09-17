use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Path {
    pub(crate) segments: Vec<Ident>,
}

impl Path {
    pub(crate) fn new_ident(ident: Ident) -> Self {
        Path {
            segments: vec![ident],
        }
    }

    pub(crate) fn make_owned(&mut self) {
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
