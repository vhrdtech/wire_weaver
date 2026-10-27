//! Observed properties in the std client (CLT-28): per trait level with `+observe` properties, the `<Trait>Update`
//! the level's `PropertyUpdate` decodes into (the same sparse struct as on the server), the `<Trait>Latest` cache,
//! the `<Trait>Observe` settings and the level's `observe(cfg)`; `observe_<name>(mode)` per property.
//!
//! Same rules as the server (`codegen/server/observe.rs`): a level under a resource array can't be observed yet, a
//! property is observable when declared with `+observe` and not in an array itself.

use crate::codegen::CodegenBundle;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::maybe_quote;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{ApiItemKindOwned, ApiLevelOwned, Multiplicity, PropertyAccess};

/// An observable property of a level.
pub(crate) struct ObservableProp<'a> {
    pub(crate) index: u32,
    pub(crate) name: &'a str,
    pub(crate) ty: &'a ww_self::TypeOwned,
    /// `+batched` (API-39): its slot is a `BatchedOwned<T>`
    pub(crate) batched: bool,
}

/// The observable properties of a level, by resource index.
pub(crate) fn observable_props(api_level: &ApiLevelOwned) -> Vec<ObservableProp<'_>> {
    api_level
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ApiItemKindOwned::Property {
                ty,
                access:
                    PropertyAccess::ReadOnly { observe: true }
                    | PropertyAccess::ReadWrite { observe: true },
                batched,
                ..
            } if matches!(item.multiplicity, Multiplicity::Flat) => Some(ObservableProp {
                index: item.id.0,
                name: item.ident.as_str(),
                ty,
                batched: batched.is_some(),
            }),
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
pub(crate) fn level_names(api_bundle: &CodegenBundle, api_level: &ApiLevelOwned) -> LevelNames {
    let t = &api_level.trait_name;
    let owned = observable_props(api_level)
        .iter()
        .any(|p| p.ty.is_lifetime(api_bundle).unwrap());
    let suffix = if owned { "Owned" } else { "" };
    LevelNames {
        update: Ident::new(&format!("{t}Update{suffix}"), Span::call_site()),
        config: Ident::new(&format!("{t}Observe"), Span::call_site()),
    }
}

/// `<Trait>Update`, `<Trait>Latest` and `<Trait>Observe` of an observable level, in its client module.
pub(crate) fn level_types(api_bundle: &CodegenBundle, api_level: &ApiLevelOwned) -> TokenStream {
    let props = observable_props(api_level);
    let LevelNames { update, config } = level_names(api_bundle, api_level);
    let latest = Ident::new(
        &format!("{}Latest", api_level.trait_name),
        Span::call_site(),
    );
    let idents: Vec<Ident> = props
        .iter()
        .map(|p| Ident::new(p.name, Span::call_site()))
        .collect();
    let indices: Vec<u32> = props.iter().map(|p| p.index).collect();
    let tys: Vec<TokenStream> = props
        .iter()
        .map(|p| ty_def(api_bundle, p.ty, true, TyPos::Def).unwrap())
        .collect();
    let slot_tys: Vec<TokenStream> = props
        .iter()
        .zip(&tys)
        .map(|(p, ty)| {
            crate::codegen::server::observe::batched_slot_ty(p.batched, ty.clone(), true)
        })
        .collect();
    // a value into `latest`: the last sample of a batched slot, with its time from the update's `sample_times` (a
    // batch whose times don't match is malformed: left out)
    let to_latest = props.iter().zip(&idents).map(|(p, ident)| {
        if p.batched {
            quote! {
                if let Some(batched) = self.#ident {
                    if let Some((value, t)) = batched
                        .into_samples(t, sample_times)
                        .ok()
                        .and_then(|mut samples| samples.pop())
                    {
                        latest.#ident = Some(wire_weaver_client::observe::Sample { value, received, t });
                    }
                }
            }
        } else {
            quote! {
                if let Some(value) = self.#ident {
                    latest.#ident = Some(wire_weaver_client::observe::Sample { value, received, t });
                }
            }
        }
    });
    let batched_idents: Vec<&Ident> = props
        .iter()
        .zip(&idents)
        .filter(|(p, _)| p.batched)
        .map(|(_, i)| i)
        .collect();
    let samples_fn = maybe_quote(
        !batched_idents.is_empty(),
        quote! {
            /// Every sample of each `+batched` property of the level, in batches at most `max_age` old (API-39,
            /// `ObserveMode::Samples`).
            pub fn samples(mut self, max_age: std::time::Duration) -> Self {
                #(self.#batched_idents = Some(wire_weaver_client::observe::samples(max_age));)*
                self
            }
        },
    );
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
                pub #idents: Option<#slot_tys>,
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
            #samples_fn
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
                sample_times: Option<&[wire_weaver_client::observe::Second]>,
            ) {
                #(#to_latest)*
            }
        }
    }
}

/// `observe(cfg)` of an observable level's client, `index_chain` set to the level's path.
pub(crate) fn level_observe_fn(
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
    api_level: &ApiLevelOwned,
    prop: &Ident,
    ty: &TokenStream,
    batched: bool,
    level_chain: TokenStream,
    path_kind: &TokenStream,
) -> TokenStream {
    let LevelNames { update, config } = level_names(api_bundle, api_level);
    let observe = Ident::new(&format!("observe_{prop}"), Span::call_site());
    let doc = format!(
        " Observe `{prop}` with `mode` (CLT-28): its values as the device sends them, fanned out from the observed \
        level (one subscription per level on the device, see `observe()`)."
    );
    let (doc, prepare) = if batched {
        (
            format!(
                "{doc} `+batched` (API-39): with `observe::samples(max_age)` every sample the device takes, one \
                `Observed` each with its own device time."
            ),
            quote! { prepare_observe_samples },
        )
    } else {
        (doc, quote! { prepare_observe })
    };
    quote! {
        #[doc = #doc]
        pub fn #observe(&self, mode: wire_weaver_client::observe::ObserveMode) -> wire_weaver_client::observe::PreparedObserve<#update, #ty> {
            #level_chain
            let path_kind = #path_kind;
            self.cmd.#prepare(path_kind, #config::default().#prop(mode), |u: #update| u.#prop)
        }
    }
}
