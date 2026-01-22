use proc_macro2::{Literal, TokenStream};
use quote::quote;
// cannot use var! as that would be proc macro info itself instead of a target crate
use std::env::var;
use syn::Lit;

pub(crate) fn full_version(_args: TokenStream) -> TokenStream {
    let crate_name = var("CARGO_PKG_NAME").unwrap();
    let crate_name = Lit::new(Literal::string(crate_name.as_str()));
    let (major, minor, patch) = get_version();
    quote! { ww_version::FullVersion::new(#crate_name, ww_version::Version::new(#major, #minor, #patch)) }
}

pub(crate) fn compact_version(gid: TokenStream) -> TokenStream {
    let (major, minor, patch) = get_version();
    if gid.is_empty() {
        quote! { None }
    } else {
        quote! { Some(ww_version::CompactVersion::new(#gid, #major, #minor, #patch)) }
    }
}

fn get_version() -> (u32, u32, u32) {
    let major = var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let major: u32 = major.parse().unwrap();
    let minor = var("CARGO_PKG_VERSION_MINOR").unwrap();
    let minor: u32 = minor.parse().unwrap();
    let patch = var("CARGO_PKG_VERSION_PATCH").unwrap();
    let patch: u32 = patch.parse().unwrap();
    (major, minor, patch)
}
