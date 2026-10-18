//! Observed properties in the std client (CLT-28): per trait level with `+observe` properties, the `<Trait>Update`
//! the level's `PropertyUpdate` decodes into (the same sparse struct as on the server), the `<Trait>Latest` cache,
//! the `<Trait>Observe` settings and the level's `observe(cfg)`; `observe_<name>(mode)` per property.
//!
//! Same rules as the server (`codegen/server/observe.rs`): a level under a resource array can't be observed yet, a
//! property is observable when declared with `+observe` and not in an array itself.

use crate::codegen::ty_def::{TyPos, ty_def};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity, PropertyAccess};

/// The observable properties of a level, by resource index: name and type.
pub(crate) fn observable_props(api_level: &ApiLevelOwned) -> Vec<(u32, &str, &ww_self::TypeOwned)> {
    api_level
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ApiItemKindOwned::Property {
                ty,
                access:
                    PropertyAccess::ReadOnly { observe: true, .. }
                    | PropertyAccess::ReadWrite { observe: true, .. },
                ..
            } if matches!(item.multiplicity, Multiplicity::Flat) => {
                Some((item.id.0, item.ident.as_str(), ty))
            }
            _ => None,
        })
        .collect()
}

pub(crate) struct LevelNames {
    pub(crate) update: Ident,
    pub(crate) config: Ident,
}

/// `<Trait>Update`, `<Trait>UpdateOwned` when a property holds owned data (`String`, `Vec`), like other owned
/// types.
pub(crate) fn level_names(api_bundle: &ApiBundleOwned, api_level: &ApiLevelOwned) -> LevelNames {
    let t = &api_level.trait_name;
    let owned = observable_props(api_level)
        .iter()
        .any(|(_, _, ty)| ty.is_lifetime(api_bundle).unwrap());
    let suffix = if owned { "Owned" } else { "" };
    LevelNames {
        update: Ident::new(&format!("{t}Update{suffix}"), Span::call_site()),
        config: Ident::new(&format!("{t}Observe"), Span::call_site()),
    }
}

/// `<Trait>Update`, `<Trait>Latest` and `<Trait>Observe` of an observable level, in its client module.
pub(crate) fn level_types(api_bundle: &ApiBundleOwned, api_level: &ApiLevelOwned) -> TokenStream {
    let props = observable_props(api_level);
    let LevelNames { update, config } = level_names(api_bundle, api_level);
    let latest = Ident::new(
        &format!("{}Latest", api_level.trait_name),
        Span::call_site(),
    );
    let idents: Vec<Ident> = props
        .iter()
        .map(|(_, name, _)| Ident::new(name, Span::call_site()))
        .collect();
    let indices: Vec<u32> = props.iter().map(|(index, _, _)| *index).collect();
    let tys: Vec<TokenStream> = props
        .iter()
        .map(|(_, _, ty)| ty_def(api_bundle, ty, true, TyPos::Def).unwrap())
        .collect();
    let trait_name = &api_level.trait_name;
    let update_doc = format!(
        " Data of a `PropertyUpdate` of `{trait_name}` (CLT-28): its observable properties by resource index, the \
        ones the device sent present."
    );
    let latest_doc = format!(
        " The latest value of each observable property of `{trait_name}`, with when it arrived."
    );
    let config_doc = format!(
        " How the observable properties of `{trait_name}` are observed: `None` (or `Off`) is not observed. Also what \
        the device granted, in `Granted::modes`."
    );
    let setter_docs = idents
        .iter()
        .map(|i| format!(" Observe `{i}` with `mode`."));
    quote! {
        #[doc = #update_doc]
        #[wire_weaver::shrink_wrap::derive_shrink_wrap(sparse, owned, derive(Debug))]
        pub struct #update {
            #(
                #[slot = #indices]
                pub #idents: Option<#tys>,
            )*
        }

        #[doc = #latest_doc]
        #[derive(Debug, Default)]
        pub struct #latest {
            #(pub #idents: Option<wire_weaver_client::observe::Sample<#tys>>,)*
        }

        #[doc = #config_doc]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct #config {
            #(pub #idents: Option<wire_weaver_client::observe::ObserveMode>,)*
        }

        impl #config {
            #(
                #[doc = #setter_docs]
                pub fn #idents(mut self, mode: wire_weaver_client::observe::ObserveMode) -> Self {
                    self.#idents = Some(mode);
                    self
                }
            )*
        }

        impl wire_weaver_client::observe::LevelConfig for #config {
            fn to_entries(&self) -> std::collections::BTreeMap<u32, wire_weaver_client::observe::ObserveMode> {
                let mut entries = std::collections::BTreeMap::new();
                #(
                    if let Some(mode) = self.#idents {
                        entries.insert(#indices, mode);
                    }
                )*
                entries
            }

            fn from_entries(entries: &std::collections::BTreeMap<u32, wire_weaver_client::observe::ObserveMode>) -> Self {
                Self {
                    #(#idents: entries.get(&#indices).copied(),)*
                }
            }
        }

        impl wire_weaver_client::observe::LevelUpdate for #update {
            type Latest = #latest;
            type Config = #config;

            #[allow(unused_variables)]
            fn update_latest(
                self,
                latest: &mut #latest,
                received: std::time::Instant,
                t: Option<wire_weaver_client::observe::Second>,
            ) {
                #(
                    if let Some(value) = self.#idents {
                        latest.#idents = Some(wire_weaver_client::observe::Sample { value, received, t });
                    }
                )*
            }
        }
    }
}

/// `observe(cfg)` of an observable level's client, `index_chain` set to the level's path.
pub(crate) fn level_observe_fn(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    level_chain: TokenStream,
    path_kind: &TokenStream,
) -> TokenStream {
    let LevelNames { update, config } = level_names(api_bundle, api_level);
    let doc = format!(
        " Observe the `+observe` properties of `{}` (CLT-28): every update the device sends for the level, in one \
        `{update}`. `.subscribe().await` returns once the device answered with what it applies (`granted()`).",
        api_level.trait_name
    );
    quote! {
        #[doc = #doc]
        pub fn observe(&self, cfg: #config) -> wire_weaver_client::observe::PreparedObserve<#update, #update> {
            #level_chain
            let path_kind = #path_kind;
            self.cmd.prepare_observe(path_kind, cfg, Some)
        }
    }
}

/// `observe_<name>(mode)` of a `+observe` property: fanned out from its level.
pub(crate) fn property_observe_fn(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    prop: &Ident,
    ty: &TokenStream,
    level_chain: TokenStream,
    path_kind: &TokenStream,
) -> TokenStream {
    let LevelNames { update, config } = level_names(api_bundle, api_level);
    let observe = Ident::new(&format!("observe_{prop}"), Span::call_site());
    let doc = format!(
        " Observe `{prop}` with `mode` (CLT-28): its values as the device sends them, fanned out from the observed \
        level (one subscription per level on the device, see `observe()`)."
    );
    quote! {
        #[doc = #doc]
        pub fn #observe(&self, mode: wire_weaver_client::observe::ObserveMode) -> wire_weaver_client::observe::PreparedObserve<#update, #ty> {
            #level_chain
            let path_kind = #path_kind;
            self.cmd.prepare_observe(path_kind, #config::default().#prop(mode), |u: #update| u.#prop)
        }
    }
}
