//! How the code these macros emit names the crates it needs, by the names the user's `Cargo.toml` gives them.

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// The `ww_api` crate: as the user's `Cargo.toml` names it, else through `wire_weaver`'s re-export (a server
/// crate declaring an API of its own). Neither found: `::ww_api`, so the compiler names the missing dependency.
pub(crate) fn ww_api() -> TokenStream {
    match crate_name("ww_api") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let name = ident(&name);
            quote! { ::#name }
        }
        Err(_) => match crate_name("wire_weaver") {
            Ok(FoundCrate::Itself) => quote! { crate::ww_api },
            Ok(FoundCrate::Name(name)) => {
                let name = ident(&name);
                quote! { ::#name::ww_api }
            }
            Err(_) => quote! { ::ww_api },
        },
    }
}

/// [ww_api] as an intra-doc link path: no leading `::`.
pub(crate) fn ww_api_doc_path() -> String {
    ww_api()
        .to_string()
        .replace(' ', "")
        .trim_start_matches("::")
        .to_owned()
}

/// The `ww_version` crate: a direct dependency under its name, else through [ww_api].
pub(crate) fn ww_version() -> TokenStream {
    match crate_name("ww_version") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let name = ident(&name);
            quote! { ::#name }
        }
        Err(_) => {
            let ww_api = ww_api();
            quote! { #ww_api::ww_version }
        }
    }
}

/// The `ww_keyed` crate, a dependency of every API crate with a `keyed!` item: as the user's `Cargo.toml` names it,
/// else `::ww_keyed`, so the compiler names the missing dependency.
pub(crate) fn ww_keyed() -> TokenStream {
    match crate_name("ww_keyed") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let name = ident(&name);
            quote! { ::#name }
        }
        Err(_) => quote! { ::ww_keyed },
    }
}

fn ident(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}
