//! Items a server with mount slots (SRV-76) generates for global trait paths into modules it serves in process
//! (SRV-82): `has_global_trait`, whether the server's own tree has the trait a global path names, and the path of each
//! slot's `api`. `ww_device::mount::ModuleTraits` resolves the rest in the modules' models.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{ApiItemKindOwned, ApiLevelOwned, Multiplicity};

use crate::codegen::CodegenBundle;
use crate::codegen::api_server::{
    GlobalPaths, GlobalTrait, global_compact_lookup, global_full_table, global_traits_recursive,
};

/// `has_global_trait` and the slots' `api` paths; nothing for a server without mount slots.
///
/// The lookup is the one dispatch uses with `global_paths = "full"` (SRV-53), whatever `global_paths` says: a server
/// that answers global paths `TraitNotFound` still has its traits, and a module never takes one of them over.
pub(crate) fn mount_items(
    api_bundle: &CodegenBundle,
    global_paths: GlobalPaths,
) -> Result<TokenStream, String> {
    let mut slots = vec![];
    collect_slots(
        api_bundle,
        &api_bundle.root,
        &mut vec![],
        &mut vec![],
        &mut slots,
    );
    if slots.is_empty() {
        return Ok(TokenStream::new());
    }
    let mut traits: Vec<GlobalTrait> = vec![];
    global_traits_recursive(api_bundle, &api_bundle.root, &mut vec![], &mut traits);
    // dispatch emits these already when it resolves global paths itself
    let table = match global_paths {
        GlobalPaths::Full => TokenStream::new(),
        _ => global_full_table(&traits)?,
    };
    let compact = match global_paths {
        GlobalPaths::Full | GlobalPaths::Compact => TokenStream::new(),
        _ => global_compact_lookup(&traits),
    };
    let slot_paths = slots.iter().map(Slot::path_item);
    Ok(quote! {
        #table
        #compact

        /// Whether this server's own tree has the trait the global path `path` names (SRV-82): the first answer to a
        /// global path, before the modules mounted in its slots (`ww_device::mount::ModuleTraits`). `false` for an
        /// absolute path.
        #[allow(dead_code)]
        pub fn has_global_trait(path: &PathKind<'_>) -> bool {
            match path {
                PathKind::Absolute { .. } => false,
                PathKind::GlobalFull { gid, trait_name, .. } => {
                    ww_client_server::dispatch::global_full_path(WW_GLOBAL_TRAITS, gid, trait_name).is_some()
                }
                PathKind::GlobalCompact { gid, .. } => ww_global_compact_trait_path(gid).is_some(),
            }
        }

        #(#slot_paths)*
    })
}

/// A mount slot: its name as its handlers have it (`hub`, `a_hub`), the ids to its `api` with `None` for each array
/// index on the way.
struct Slot {
    name: String,
    dotted: String,
    path: Vec<Option<u32>>,
}

impl Slot {
    /// `HUB_API_PATH`, or for a slot in an array `ext_api_path(index_chain)`.
    fn path_item(&self) -> TokenStream {
        let len = self.path.len();
        let arrays = self.path.iter().filter(|id| id.is_none()).count();
        let mut index = 0usize;
        let ids = self.path.iter().map(|id| match id {
            Some(id) => quote! { ResourceIndex::new(UNib32(#id)) },
            None => {
                let i = index;
                index += 1;
                quote! { index_chain[#i] }
            }
        });
        let ids: Vec<_> = ids.collect();
        let dotted = &self.dotted;
        if arrays == 0 {
            let name = Ident::new(
                &format!("{}_API_PATH", self.name.to_uppercase()),
                Span::call_site(),
            );
            let doc = format!(
                "Path of the `api` of the mount slot `{dotted}` (SRV-82): where `ww_device::mount::ModuleTraits` looks \
                 for the traits of the module it serves."
            );
            quote! {
                #[doc = #doc]
                #[allow(dead_code)]
                pub const #name: [ResourceIndex; #len] = [#(#ids),*];
            }
        } else {
            let name = Ident::new(&format!("{}_api_path", self.name), Span::call_site());
            let doc = format!(
                "Path of the `api` of the mount slot `{dotted}` at `index_chain`, the indices of its arrays as its \
                 handlers take them (SRV-82)."
            );
            quote! {
                #[doc = #doc]
                #[allow(dead_code)]
                pub const fn #name(index_chain: [ResourceIndex; #arrays]) -> [ResourceIndex; #len] {
                    [#(#ids),*]
                }
            }
        }
    }
}

fn collect_slots(
    api_bundle: &CodegenBundle,
    level: &ApiLevelOwned,
    names: &mut Vec<String>,
    path: &mut Vec<Option<u32>>,
    out: &mut Vec<Slot>,
) {
    for item in &level.items {
        let depth = path.len();
        path.push(Some(item.id.0));
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            path.push(None);
        }
        names.push(item.ident.clone());
        match &item.kind {
            ApiItemKindOwned::Mount { .. } => {
                let mut api = path.clone();
                api.push(Some(ww_self::mount::API));
                out.push(Slot {
                    name: names.join("_"),
                    dotted: names.join("."),
                    path: api,
                });
            }
            ApiItemKindOwned::Trait { .. } => {
                if let Ok(child) = item.get_as_level(api_bundle) {
                    collect_slots(api_bundle, child, names, path, out);
                }
            }
            _ => {}
        }
        names.pop();
        path.truncate(depth);
    }
}
