use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::ApiLevelOwned;

pub fn maybe_quote(condition: bool, tokens_if_true: TokenStream) -> TokenStream {
    if condition {
        tokens_if_true
    } else {
        TokenStream::new()
    }
}

pub fn add_prefix(prefix: Option<&String>, ident: &Ident) -> Ident {
    match prefix {
        Some(prefix) => Ident::new(format!("{}_{}", prefix, ident).as_str(), ident.span()),
        None => ident.clone(),
    }
}

#[derive(Default)]
pub(crate) struct ErrorSeq(u32);

impl ErrorSeq {
    pub(crate) fn next_err(&mut self) -> TokenStream {
        let seq = self.0;
        let ts = quote! { #seq };
        self.0 += 1;
        ts
    }
}

pub(crate) fn mod_name(crate_name: &str, api_level: &ApiLevelOwned) -> Ident {
    Ident::new(
        format!(
            "{}_{}",
            crate_name,
            api_level.trait_name.to_case(Case::Snake)
        )
        .as_str(),
        Span::call_site(),
    )
}
