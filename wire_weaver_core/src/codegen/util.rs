use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Path, PathArguments, PathSegment};
use ww_self::{ApiBundleOwned, ApiLevelOwned};

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

pub(crate) fn mod_name(api_level: &ApiLevelOwned, api_bundle: &ApiBundleOwned) -> Ident {
    let crate_name = api_level.crate_name(api_bundle).unwrap();
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

pub(crate) fn source_marker(api_level: &ApiLevelOwned, api_bundle: &ApiBundleOwned) -> TokenStream {
    let crate_name = api_level.crate_name(api_bundle).unwrap();
    let crate_name = Ident::new(crate_name, Span::call_site());
    let trait_full_gid = Ident::new(
        &format!("{}_FULL_GID", api_level.trait_name).to_case(Case::Constant),
        Span::call_site(),
    );
    quote! {
        #[allow(unused_imports)]
        use #crate_name::#trait_full_gid as _SOURCE_MARKER;
    }
}

pub(crate) fn str_to_path(path: &str) -> Path {
    Path {
        leading_colon: None,
        segments: path
            .split("::")
            .map(|s| PathSegment {
                ident: Ident::new(s, Span::call_site()),
                arguments: PathArguments::None,
            })
            .collect(),
    }
}
