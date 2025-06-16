use proc_macro2::{Literal, TokenStream};
use quote::quote;
use std::env::var; // cannot use var! as that would be proc macro info itself instead of a target crate
use syn::Lit;

pub(crate) fn full_version(_args: TokenStream) -> TokenStream {
    let crate_name = var("CARGO_PKG_NAME").unwrap();
    let crate_name = Lit::new(Literal::string(crate_name.as_str()));
    let major = var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let major: u32 = major.parse().unwrap();
    let minor = var("CARGO_PKG_VERSION_MINOR").unwrap();
    let minor: u32 = minor.parse().unwrap();
    let patch = var("CARGO_PKG_VERSION_PATCH").unwrap();
    let patch: u32 = patch.parse().unwrap();
    quote! { ww_version::FullVersion::new(#crate_name, ww_version::Version::new(#major, #minor, #patch)) }
}
