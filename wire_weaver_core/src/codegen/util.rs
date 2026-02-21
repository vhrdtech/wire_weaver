use proc_macro2::{Ident, TokenStream};
use quote::quote;
use shrink_wrap_core::ast::Version;

pub fn maybe_quote(condition: bool, tokens_if_true: TokenStream) -> TokenStream {
    if condition {
        tokens_if_true
    } else {
        TokenStream::new()
    }
}

pub fn add_prefix(prefix: Option<&Ident>, ident: &Ident) -> Ident {
    match prefix {
        Some(prefix) => Ident::new(format!("{}_{}", prefix, ident).as_str(), ident.span()),
        None => ident.clone(),
    }
}

pub(crate) fn maybe_call_since(since: &Option<Version>) -> TokenStream {
    let Some(version) = since else {
        return quote! { None };
    };
    let major = version.major;
    let minor = version.minor;
    let patch = version.patch;
    quote! {
        Some((#major, #minor, #patch))
    }
}
