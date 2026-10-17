pub mod api_client;
mod api_common;
pub mod api_server;
mod client;
mod index_chain;
pub mod omit;
pub(crate) mod server;
mod ty_def;
mod util;

use std::ops::Deref;
use ww_self::ApiBundleOwned;

/// An API model ([ApiBundleOwned]) together with what code generation needs on top of it, loaded with
/// [crate::load_for_codegen] or [crate::load_dep]. Derefs to the bundle.
#[derive(Clone, Debug)]
pub struct CodegenBundle {
    pub bundle: ApiBundleOwned,
    /// For each entry of `bundle.types`, the paths by which other crates name a struct or enum (API-46): the API
    /// model only has its name. `None` for the other types.
    pub type_paths: Vec<Option<TypePath>>,
}

/// Paths after the crate name by which other crates name a definition: `["units", "Volt"]`, `["Volt"]` when
/// re-exported at the crate root, `["Outer"]` for `pub use ty::Inner as Outer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypePath {
    pub path: Vec<String>,
    /// Path of its `<Name>Owned` variant (used for a definition with a lifetime), `None` when it isn't reachable from
    /// other crates.
    pub owned: Option<Vec<String>>,
}

impl Deref for CodegenBundle {
    type Target = ApiBundleOwned;

    fn deref(&self) -> &ApiBundleOwned {
        &self.bundle
    }
}
