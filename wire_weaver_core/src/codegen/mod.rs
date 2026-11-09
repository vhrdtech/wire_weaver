pub mod api_client;
mod api_common;
pub mod api_server;
mod client;
mod index_chain;
pub mod local_trait;
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

    /// Wire version of the API root, what link setup decides on (EVO-25). Codegen reads it only here, so where it is
    /// declared (the root's attribute today, the crate's `Cargo.toml` later) is the bundle's business alone.
    pub(crate) fn root_wire_version(&self) -> Option<ww_version::WireVersion> {
        self.bundle.wire
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

impl CodegenBundle<'_> {
    /// Modules from its crate's root to the trait `trait_name` of `crate_name`, empty at the crate root (EVO-37).
    pub(crate) fn trait_module(&self, crate_name: &str, trait_name: &str) -> &[String] {
        self.crate_paths.trait_module(crate_name, trait_name)
    }

    /// How generated code names the module of `level`'s trait, where the consts `#[ww_api_root]`/`#[ww_trait]` emit
    /// next to the trait are (`<TRAIT>_FULL_GID`): its crate's path, and the trait's module in a crate with several
    /// roots (`thermo_api::wire_v2`, EVO-37).
    pub(crate) fn trait_module_path(
        &self,
        level: &ww_self::ApiLevelOwned,
    ) -> proc_macro2::TokenStream {
        let crate_name = level.crate_name(self.bundle).unwrap();
        let crate_path = self.crate_path(crate_name);
        let modules = self
            .crate_paths
            .trait_module(crate_name, &level.trait_name)
            .iter()
            .map(|s| quote::format_ident!("{s}"));
        quote::quote! { #crate_path #(:: #modules)* }
    }
}

impl Deref for CodegenBundle<'_> {
    type Target = ApiBundleOwned;

    fn deref(&self) -> &ApiBundleOwned {
        self.bundle
    }
}
