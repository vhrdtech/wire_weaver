use proc_macro2::{Literal, TokenStream};
use quote::quote;
// cannot use var! as that would be proc macro info itself instead of a target crate
use std::env::var;
use syn::{Ident, Lit};

pub(crate) fn full_version(_args: TokenStream) -> TokenStream {
    let crate_name = var("CARGO_PKG_NAME").unwrap();
    let crate_name = Lit::new(Literal::string(crate_name.as_str()));
    let (major, minor, patch) = get_version();
    quote! { ww_version::FullVersion::new(#crate_name, ww_version::Version::new(#major, #minor, #patch)) }
}

/// `compact_version!(Trait)`: the id of `this_crate::Trait` in the `ww_global` registry, `compact_version!()`: the id
/// of this crate. `None` when the registry has no id for it. Ids are never written in code (API-20).
pub(crate) fn compact_version(args: TokenStream) -> TokenStream {
    let crate_name = var("CARGO_PKG_NAME").unwrap();
    let gid = if args.is_empty() {
        ww_global::crate_id(&crate_name)
    } else {
        let trait_name: Ident = match syn::parse2(args) {
            Ok(ident) => ident,
            Err(e) => {
                let msg = format!(
                    "compact_version! takes a trait name (or nothing for the crate), ids live only in ww_global/wire_weaver_gid.json: {e}"
                );
                return quote! { compile_error!(#msg) };
            }
        };
        ww_global::trait_id(&crate_name, &trait_name.to_string())
    };
    let (major, minor, patch) = get_version();
    match gid {
        Some(gid) => {
            let id = gid.id.0;
            quote! { Some(ww_version::CompactVersion::new(ww_version::GlobalTypeId::new(#id), #major, #minor, #patch)) }
        }
        None => quote! { None },
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
