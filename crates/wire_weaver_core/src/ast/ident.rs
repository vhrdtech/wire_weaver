// #[derive(Debug)]
// pub struct Span {
//     pub byte_range: Range<usize>
// }

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};

#[derive(Debug, Clone)]
pub struct Ident {
    pub sym: String,
    // pub span: Span,
}

impl Ident {
    pub(crate) fn new(sym: impl AsRef<str>) -> Self {
        Ident {
            sym: sym.as_ref().to_string(),
        }
    }
}

impl From<syn::Ident> for Ident {
    fn from(value: syn::Ident) -> Self {
        Ident {
            sym: value.to_string(),
            // span: Span {
            //     byte_range: value.span().byte_range()
            // }
        }
    }
}

impl From<&Ident> for syn::Ident {
    fn from(value: &Ident) -> Self {
        let ident = value.sym.as_str();
        syn::Ident::new(ident, proc_macro2::Span::call_site())
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = proc_macro2::Ident::new(self.sym.as_str(), Span::call_site());
        tokens.append(ident);
    }
}
