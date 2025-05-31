use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct Ident {
    pub sym: String,
    pub span: Span,
}

impl PartialEq for Ident {
    fn eq(&self, other: &Self) -> bool {
        self.sym == other.sym
    }
}
impl Eq for Ident {}
impl Hash for Ident {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.sym.as_bytes());
    }
}

impl Ident {
    pub fn new(sym: impl AsRef<str>) -> Self {
        Ident {
            sym: sym.as_ref().to_string(),
            span: Span::call_site(),
        }
    }
}

impl From<syn::Ident> for Ident {
    fn from(value: syn::Ident) -> Self {
        Ident {
            sym: value.to_string(),
            span: value.span(),
        }
    }
}

impl From<&syn::Ident> for Ident {
    fn from(value: &syn::Ident) -> Self {
        Ident {
            sym: value.to_string(),
            span: value.span(),
        }
    }
}

impl From<&Ident> for syn::Ident {
    fn from(value: &Ident) -> Self {
        syn::Ident::new(value.sym.as_str(), value.span)
    }
}

impl From<Ident> for syn::Ident {
    fn from(value: Ident) -> Self {
        syn::Ident::new(value.sym.as_str(), value.span)
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = proc_macro2::Ident::new(self.sym.as_str(), self.span);
        tokens.append(ident);
    }
}
