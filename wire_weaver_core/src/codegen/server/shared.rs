//! Shared items (SRV-65, `receiver_model`): which items of an API run side by side through `&self` handlers, and
//! the generated `is_shared(bytes)` that tells a runtime which entry a request goes to.
//!
//! The `&self` dispatch tree itself (`process_shared_<level>`) is the server's level tree in shared mode, see
//! `api_server::process_request_inner_recursive`.
use crate::codegen::CodegenBundle;
use crate::codegen::api_server::keys_level_of;
use crate::codegen::omit::{Omission, Omissions};
use crate::codegen::util::dotted;
use crate::receiver_model::ReceiverModel;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};
use ww_self::{ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Multiplicity, PropertyAccess};

/// The request a shareable item serves through the `&self` tree.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum SharedAs {
    /// A method: `Call`, `MultiCall`.
    Call,
    /// A readable property: `Read`, `MultiRead`.
    Read,
}

/// What an item of `kind` would serve if shared: methods and readable properties only.
pub(crate) fn shareable(kind: &ApiItemKindOwned) -> Option<SharedAs> {
    match kind {
        ApiItemKindOwned::Method { .. } => Some(SharedAs::Call),
        ApiItemKindOwned::Property {
            access: PropertyAccess::ReadOnly { .. } | PropertyAccess::ReadWrite { .. },
            ..
        } => Some(SharedAs::Read),
        _ => None,
    }
}

/// Which items of a server are shared.
#[derive(Copy, Clone)]
pub(crate) struct Receivers<'a> {
    pub(crate) model: &'a ReceiverModel,
}

impl Receivers<'_> {
    /// What the item `ident` of the level at `level_path` serves through the `&self` tree, None when it is
    /// exclusive (or can't be shared). Levels and key companions are never shared themselves.
    pub(crate) fn shared_as(
        &self,
        level_path: &[String],
        ident: &str,
        kind: &ApiItemKindOwned,
    ) -> Option<SharedAs> {
        shareable(kind).filter(|_| self.model.is_shared(&dotted(level_path, ident)))
    }

    /// Whether `item` of `level` (at `level_path`) is shared or has a shared item below it: it then has an arm in the
    /// `&self` tree.
    pub(crate) fn item_has_shared(
        &self,
        api_bundle: &CodegenBundle,
        level: &ApiLevelOwned,
        item: &ApiItemOwned,
        level_path: &mut Vec<String>,
    ) -> bool {
        match &item.kind {
            ApiItemKindOwned::Trait { .. } => {
                level_path.push(item.ident.clone());
                let has = self.level_has_shared(
                    api_bundle,
                    item.get_as_level(api_bundle)
                        .expect("a trait item has its level"),
                    level_path,
                );
                level_path.pop();
                has
            }
            ApiItemKindOwned::Keys { .. } => keys_level_of(level, item).is_ok_and(|(keys, _)| {
                level_path.push(item.ident.clone());
                let has = keys
                    .items
                    .iter()
                    .any(|k| self.shared_as(level_path, &k.ident, &k.kind).is_some());
                level_path.pop();
                has
            }),
            kind => self.shared_as(level_path, &item.ident, kind).is_some(),
        }
    }

    /// Whether the `valid_indices_*` of the array `array` of `level` (at `level_path`) takes `&self`: when the array
    /// has a shared item, or a key companion of it a shared `key`, which the companion answers per element.
    pub(crate) fn valid_indices_shared(
        &self,
        api_bundle: &CodegenBundle,
        level: &ApiLevelOwned,
        array: &ApiItemOwned,
        level_path: &mut Vec<String>,
    ) -> bool {
        self.item_has_shared(api_bundle, level, array, level_path)
            || level.items.iter().any(|keys| {
                let ApiItemKindOwned::Keys { of, .. } = &keys.kind else {
                    return false;
                };
                let Ok((level_of_keys, _)) = keys_level_of(level, keys) else {
                    return false;
                };
                level_path.push(keys.ident.clone());
                let shared = *of == array.id
                    && level_of_keys.items.iter().any(|k| {
                        k.id.0 == ww_self::keys::KEY
                            && self.shared_as(level_path, &k.ident, &k.kind).is_some()
                    });
                level_path.pop();
                shared
            })
    }

    /// Whether any item of `level` (at `level_path`) or below is shared.
    pub(crate) fn level_has_shared(
        &self,
        api_bundle: &CodegenBundle,
        level: &ApiLevelOwned,
        level_path: &mut Vec<String>,
    ) -> bool {
        level
            .items
            .iter()
            .any(|item| self.item_has_shared(api_bundle, level, item, level_path))
    }
}

/// Dotted paths of every item that can be shared, for checking `receiver_model` patterns.
pub(crate) fn shareable_paths(
    api_bundle: &CodegenBundle,
    level: &ApiLevelOwned,
    level_path: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    for item in &level.items {
        match &item.kind {
            ApiItemKindOwned::Trait { .. } => {
                level_path.push(item.ident.clone());
                shareable_paths(
                    api_bundle,
                    item.get_as_level(api_bundle)
                        .expect("a trait item has its level"),
                    level_path,
                    out,
                );
                level_path.pop();
            }
            ApiItemKindOwned::Keys { .. } => {
                if let Ok((keys, _)) = keys_level_of(level, item) {
                    level_path.push(item.ident.clone());
                    out.extend(
                        keys.items
                            .iter()
                            .filter(|k| shareable(&k.kind).is_some())
                            .map(|k| dotted(level_path, &k.ident)),
                    );
                    level_path.pop();
                }
            }
            kind => {
                if shareable(kind).is_some() {
                    out.push(dotted(level_path, &item.ident));
                }
            }
        }
    }
}

/// `is_shared(bytes)` and the walk of the path it uses (`ww_is_shared_<level>`), one fn per level with a shared item.
///
/// `global_arms` resolve a global trait path to its base like the entries do, answering `false` when the trait is
/// not found.
pub(crate) fn is_shared_fns(
    api_bundle: &CodegenBundle,
    omissions: &Omissions,
    receivers: Receivers<'_>,
    global_arms: &TokenStream,
) -> TokenStream {
    let walk = walk_level(
        api_bundle,
        omissions,
        receivers,
        &api_bundle.root,
        &mut vec![],
        "root",
        &quote! {},
    );
    quote! {
        /// Whether the request in `bytes` goes to `process_shared_bytes` (SRV-65): a call, a read, a multi call or a
        /// multi read of a shared item, with a non-zero `seq`. Everything else goes to `process_request_bytes`.
        /// Takes no `self`, so a runtime can ask while an exclusive request holds the server. A request that doesn't
        /// decode is not shared: the exclusive entry answers it.
        #[allow(dead_code)]
        pub fn is_shared(bytes: &[u8]) -> bool {
            let mut rd = BufReader::new(bytes);
            let Ok(request) = Request::des_shrink_wrap(&mut rd) else {
                return false;
            };
            if request.seq.0 == 0 {
                return false;
            }
            // a multi request is one more path element per index, plus `in_each_array_id` (see multi_req_handlers)
            let (read, multi) = match &request.kind {
                RequestKind::Call { .. } => (false, None),
                RequestKind::Read => (true, None),
                RequestKind::MultiCall { in_each_array_id, .. } => (false, Some(*in_each_array_id)),
                RequestKind::MultiRead { in_each_array_id, .. } => (true, Some(*in_each_array_id)),
                _ => return false,
            };
            let (base, path): (&[ResourceIndex], _) = match &request.path_kind {
                PathKind::Absolute { path } => (&[], path),
                #global_arms
            };
            let tail = match multi {
                Some(in_each_array_id) => [Some(ResourceIndex::new(UNib32(0))), in_each_array_id],
                None => [None, None],
            };
            let mut ids = base
                .iter()
                .map(|id| Ok(*id))
                .chain(path.iter())
                .chain(tail.into_iter().flatten().map(Ok));
            Self::ww_is_shared_root(&mut ids, read)
        }

        #walk
    }
}

/// `ww_is_shared_<level>(ids, read)` of `level` and of its levels with shared items: whether the rest of the path
/// names a shared item that serves a read (`read`) or a call.
fn walk_level(
    api_bundle: &CodegenBundle,
    omissions: &Omissions,
    receivers: Receivers<'_>,
    level: &ApiLevelOwned,
    level_path: &mut Vec<String>,
    level_name_chain: &str,
    cfg_attrs: &TokenStream,
) -> TokenStream {
    let fn_name = walk_fn(level_name_chain);
    let omitted = omissions.level(level_path);
    let mut arms = vec![];
    let mut below = TokenStream::new();
    for item in &level.items {
        let arm_cfg = match omitted.get(&item.id.0) {
            Some(Omission::Always) => continue,
            Some(Omission::When(cfg)) => quote! { #[cfg(not(#cfg))] },
            None => quote! {},
        };
        if !receivers.item_has_shared(api_bundle, level, item, level_path) {
            continue;
        }
        let check = match &item.kind {
            ApiItemKindOwned::Trait { .. } => {
                let chain = format!("{level_name_chain}_{}", item.ident);
                let child = walk_fn(&chain);
                level_path.push(item.ident.clone());
                below.extend(walk_level(
                    api_bundle,
                    omissions,
                    receivers,
                    item.get_as_level(api_bundle)
                        .expect("a trait item has its level"),
                    level_path,
                    &chain,
                    &quote! { #cfg_attrs #arm_cfg },
                ));
                level_path.pop();
                quote! { Self::#child(ids, read) }
            }
            ApiItemKindOwned::Keys { .. } => {
                level_path.push(item.ident.clone());
                let check = walk_keys(receivers, level, item, level_path);
                level_path.pop();
                check
            }
            kind => serves(receivers.shared_as(level_path, &item.ident, kind)),
        };
        let check = match item.multiplicity {
            Multiplicity::Flat => check,
            // reading an array without an index answers its valid indices, through `&self` when it has a shared item
            Multiplicity::Array { .. } => quote! {
                match ids.next() {
                    Some(Ok(_)) => { #check }
                    None => read,
                    Some(Err(_)) => false,
                }
            },
        };
        let id = id_lit(item.id.0);
        arms.push(quote! { #arm_cfg #id => { #check } });
    }
    quote! {
        #[allow(dead_code)]
        #cfg_attrs
        fn #fn_name(ids: &mut impl Iterator<Item = Result<ResourceIndex, ShrinkWrapError>>, read: bool) -> bool {
            let Some(Ok(id)) = ids.next() else {
                return false;
            };
            match id.get().0 {
                #(#arms)*
                _ => false,
            }
        }

        #below
    }
}

/// The check of a key companion `keys` of `level` (`index_of` a call, `key[I]` a read), at `level_path` (which ends
/// with the companion's name).
fn walk_keys(
    receivers: Receivers<'_>,
    level: &ApiLevelOwned,
    keys: &ApiItemOwned,
    level_path: &[String],
) -> TokenStream {
    let Ok((level_of_keys, _)) = keys_level_of(level, keys) else {
        return quote! { false };
    };
    let arms = level_of_keys.items.iter().filter_map(|k| {
        let shared_as = receivers.shared_as(level_path, &k.ident, &k.kind)?;
        let id = id_lit(k.id.0);
        let check = serves(Some(shared_as));
        // `key` is answered per element of the keyed array (see handle_keys)
        let check = if let ApiItemKindOwned::Property { .. } = k.kind {
            quote! {
                match ids.next() {
                    Some(Ok(_)) => { #check }
                    None => read,
                    Some(Err(_)) => false,
                }
            }
        } else {
            check
        };
        Some(quote! { #id => { #check } })
    });
    quote! {
        match ids.next() {
            Some(Ok(id)) => match id.get().0 {
                #(#arms)*
                _ => false,
            },
            _ => false,
        }
    }
}

/// Whether an item that serves `shared_as` takes this request: `read` for a property, a call for a method.
fn serves(shared_as: Option<SharedAs>) -> TokenStream {
    match shared_as {
        Some(SharedAs::Read) => quote! { read },
        Some(SharedAs::Call) => quote! { !read },
        None => quote! { false },
    }
}

fn walk_fn(level_name_chain: &str) -> Ident {
    Ident::new(
        &format!("ww_is_shared_{level_name_chain}"),
        Span::call_site(),
    )
}

fn id_lit(id: u32) -> Lit {
    Lit::Int(LitInt::new(&format!("{id}u32"), Span::call_site()))
}
