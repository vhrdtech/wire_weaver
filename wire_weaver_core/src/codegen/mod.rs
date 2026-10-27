pub mod api_client;
mod api_common;
pub mod api_server;
mod client;
mod index_chain;
pub mod omit;
pub(crate) mod server;
mod ty_def;
mod util;

use crate::transform::CratePaths;
use std::ops::Deref;
use ww_self::ApiBundleOwned;

/// The API bundle code is generated from, with how the generated code names the crates of its types and traits
/// (API-60).
pub(crate) struct CodegenBundle<'a> {
    bundle: &'a ApiBundleOwned,
    crate_paths: &'a CratePaths,
}

impl<'a> CodegenBundle<'a> {
    pub(crate) fn new(bundle: &'a ApiBundleOwned, crate_paths: &'a CratePaths) -> Self {
        Self {
            bundle,
            crate_paths,
        }
    }

    /// How generated code names the crate `crate_name`: through the crate that re-exports it, or by its own name.
    pub(crate) fn crate_path(&self, crate_name: &str) -> proc_macro2::TokenStream {
        let segments: Vec<proc_macro2::Ident> = match self.crate_paths.get(crate_name) {
            Some(path) => path.iter().map(|s| quote::format_ident!("{s}")).collect(),
            None => vec![quote::format_ident!("{crate_name}")],
        };
        quote::quote! { #(#segments)::* }
    }
}

impl Deref for CodegenBundle<'_> {
    type Target = ApiBundleOwned;

    fn deref(&self) -> &ApiBundleOwned {
        self.bundle
    }
}
