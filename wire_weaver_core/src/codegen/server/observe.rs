//! Observed properties on the server (SRV-18): the `Observe` request handler, `ObserveState`, the
//! `<Trait>Update` types, `mark_changed_<name>()` and `poll_properties`, on top of `ww_stream::observe`.
//!
//! Generated only when the API has `+observe` properties, so that servers without them don't need `ww_stream`.
//! A level is observable when it isn't under a resource array (the bound of an array's indices is not known yet,
//! API-29): `Observe` on a level under an array answers `OperationNotSupported`. Properties are observable when
//! declared with `+observe`, not in an array themselves and not `#[optional]`.

use crate::codegen::omit::Omissions;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{ErrorSeq, add_prefix, maybe_quote};
use crate::property_model::{PropertyModel, PropertyModelKind};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::BTreeSet;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity, PropertyAccess, TypeOwned,
};

/// A trait level that `Observe` can address.
struct Level {
    /// `root`, `root_motor`: field of `ObserveState`
    field: Ident,
    crate_name: String,
    trait_name: String,
    path: Vec<u32>,
    /// Largest resource index + 1
    item_count: u32,
    props: Vec<Prop>,
}

struct Prop {
    index: u32,
    ident: Ident,
    /// With the trait item chain prefix: handler and field names
    prefixed: Ident,
    ty: TypeOwned,
    model: PropertyModelKind,
    /// `+batched` (API-39)
    batched: bool,
}

/// The observable levels and properties of an API.
pub(crate) struct Observe {
    levels: Vec<Level>,
    /// Paths of resource arrays of traits: levels below them can't be observed yet
    arrays: Vec<Vec<u32>>,
}

impl Observe {
    pub(crate) fn collect(
        api_bundle: &ApiBundleOwned,
        omissions: &Omissions,
        property_model: &PropertyModel,
    ) -> Self {
        let mut observe = Observe {
            levels: vec![],
            arrays: vec![],
        };
        observe.walk(
            api_bundle,
            &api_bundle.root,
            omissions,
            property_model,
            &[],
            None,
            "root",
            &mut vec![],
        );
        observe
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        api_bundle: &ApiBundleOwned,
        level: &ApiLevelOwned,
        omissions: &Omissions,
        property_model: &PropertyModel,
        item_path: &[String],
        prefix: Option<&String>,
        chain: &str,
        path: &mut Vec<u32>,
    ) {
        let omitted = omissions.level(item_path);
        let mut props = vec![];
        for item in &level.items {
            if omitted.contains_key(&item.id.0) {
                continue;
            }
            let flat = matches!(item.multiplicity, Multiplicity::Flat);
            match &item.kind {
                ApiItemKindOwned::Property {
                    ty,
                    access:
                        PropertyAccess::ReadOnly {
                            observe: true,
                            batched,
                        }
                        | PropertyAccess::ReadWrite {
                            observe: true,
                            batched,
                        },
                    ..
                } if flat => {
                    let ident = Ident::new(&item.ident, Span::call_site());
                    props.push(Prop {
                        index: item.id.0,
                        prefixed: add_prefix(prefix, &ident),
                        model: property_model
                            .pick(&item.ident)
                            .unwrap_or(PropertyModelKind::GetSet),
                        ident,
                        ty: ty.clone(),
                        batched: *batched,
                    });
                }
                ApiItemKindOwned::Trait { .. } => {
                    path.push(item.id.0);
                    if flat {
                        let prefix = match prefix {
                            Some(prefix) => format!("{prefix}_{}", item.ident),
                            None => item.ident.clone(),
                        };
                        let mut item_path = item_path.to_vec();
                        item_path.push(item.ident.clone());
                        self.walk(
                            api_bundle,
                            item.get_as_level(api_bundle).unwrap(),
                            omissions,
                            property_model,
                            &item_path,
                            Some(&prefix),
                            &format!("{chain}_{}", item.ident),
                            path,
                        );
                    } else {
                        self.arrays.push(path.clone());
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        self.levels.push(Level {
            field: Ident::new(chain, Span::call_site()),
            crate_name: level.crate_name(api_bundle).unwrap().to_string(),
            trait_name: level.trait_name.clone(),
            path: path.clone(),
            item_count: level.items.iter().map(|i| i.id.0 + 1).max().unwrap_or(0),
            props,
        });
    }

    /// Whether the API has anything to observe: the rest is generated only then.
    pub(crate) fn is_enabled(&self) -> bool {
        self.levels.iter().any(|l| !l.props.is_empty())
    }

    /// Prefixed names of the observable properties: `mark_changed_<name>()` exists for them.
    pub(crate) fn observable(&self) -> BTreeSet<String> {
        self.levels
            .iter()
            .flat_map(|l| l.props.iter().map(|p| p.prefixed.to_string()))
            .collect()
    }

    fn tables(&self) -> impl Iterator<Item = &Level> {
        self.levels.iter().filter(|l| !l.props.is_empty())
    }

    /// `ObserveState` and the `<Trait>Update` types, outside of the server struct's impl.
    pub(crate) fn items(&self, api_bundle: &ApiBundleOwned, no_alloc: bool) -> TokenStream {
        if !self.is_enabled() {
            return quote! {};
        }
        let fields: Vec<&Ident> = self.tables().map(|l| &l.field).collect();
        let ns: Vec<usize> = self.tables().map(|l| l.props.len()).collect();
        let docs = self.tables().map(|l| {
            let path = l.path.iter().map(|p| p.to_string()).collect::<Vec<_>>();
            format!(" `{}` at `/{}`", l.trait_name, path.join("/"))
        });
        let mut updates = TokenStream::new();
        let mut seen: Vec<(String, String)> = vec![];
        for level in self.tables() {
            let key = (level.crate_name.clone(), level.trait_name.clone());
            if seen.contains(&key) {
                continue;
            }
            let mut name = format!("{}Update", level.trait_name);
            if seen.iter().any(|(_, t)| *t == level.trait_name) {
                name = format!(
                    "{}{}Update",
                    level.crate_name.to_case(Case::Pascal),
                    level.trait_name
                );
            }
            seen.push(key);
            updates.extend(update_struct(api_bundle, level, &name, no_alloc));
        }
        quote! {
            /// What the server keeps about observed properties (SRV-18): one table per trait level with `+observe`
            /// properties, and the limits every `Observe` request is clamped to. The server struct holds it and
            /// hands it out with `fn observe_state(&mut self) -> &mut ObserveState`.
            pub struct ObserveState {
                pub limits: ww_stream::observe::ObserveLimits,
                #(
                    #[doc = #docs]
                    pub #fields: ww_stream::observe::ObserveTable<#ns>,
                )*
            }

            impl ObserveState {
                /// Nothing observed yet.
                pub const fn new(limits: ww_stream::observe::ObserveLimits) -> Self {
                    ObserveState {
                        limits,
                        #(#fields: ww_stream::observe::ObserveTable::new(),)*
                    }
                }

                /// Stop observing everything, e.g., when the link went down.
                pub fn reset(&mut self) {
                    #(self.#fields.reset();)*
                }

                /// Bytes per second granted to all levels together.
                pub fn budget_in_use(&self) -> u32 {
                    0u32 #(.saturating_add(self.#fields.budget()))*
                }
            }

            #updates
        }
    }

    /// `observe_request`, `mark_changed_<name>()` and `poll_properties`, in the server struct's impl.
    pub(crate) fn methods(
        &self,
        api_bundle: &ApiBundleOwned,
        use_async: bool,
        medium_ty: &TokenStream,
        error_seq: &mut ErrorSeq,
    ) -> TokenStream {
        if !self.is_enabled() {
            return quote! {};
        }
        let observe_request = self.observe_request(error_seq);
        let mark_changed = self.tables().map(|level| {
            let field = &level.field;
            let fns = level.props.iter().enumerate().map(|(k, p)| {
                let name = mark_changed_ident(&p.prefixed);
                let doc = format!(
                    " `{}` changed: sent to hosts that observe it on change. Called by the generated code after a \
                    write that changed a `value_on_changed` property; call it whenever the device changes it.",
                    p.ident
                );
                quote! {
                    #[doc = #doc]
                    pub fn #name(&mut self) {
                        self.observe_state().#field.mark_changed(#k);
                    }
                }
            });
            quote! { #(#fns)* }
        });
        let poll = self.poll_properties(api_bundle, use_async, medium_ty);
        quote! {
            #observe_request
            #(#mark_changed)*
            #poll
        }
    }

    fn observe_request(&self, error_seq: &mut ErrorSeq) -> TokenStream {
        let (es_malformed, es_refused, es_ser, es_array, es_path) = (
            error_seq.next(),
            error_seq.next(),
            error_seq.next(),
            error_seq.next(),
            error_seq.next(),
        );
        let arms = self.levels.iter().map(|level| {
            let path = &level.path;
            let n = level.props.len();
            let item_count = level.item_count;
            let indices = level.props.iter().map(|p| p.index);
            let batched = level.props.iter().map(|p| p.batched);
            let desc = quote! {
                const DESC: ww_stream::observe::LevelDesc<#n> = ww_stream::observe::LevelDesc {
                    item_count: #item_count,
                    observable: [#(#indices),*],
                    batched: [#(#batched),*],
                };
            };
            let field = &level.field;
            let table = if level.props.is_empty() {
                quote! {
                    let limits = self.observe_state().limits;
                    let table = &mut ww_stream::observe::ObserveTable::<0>::new();
                    let others = 0;
                }
            } else {
                quote! {
                    let state = self.observe_state();
                    let limits = state.limits;
                    let others = state.budget_in_use().saturating_sub(state.#field.budget());
                    let table = &mut state.#field;
                }
            };
            quote! {
                [#(UNib32(#path)),*] => {
                    #desc
                    #table
                    // put back when the answer can't be written: the host must not get an error for settings the
                    // device applied
                    let before = table.clone();
                    let refused_len = match table.apply(cfg, &DESC, &limits, others, &mut refused) {
                        Ok(len) => len,
                        Err(ww_stream::observe::ObserveError::Malformed(_)) => {
                            return Err(Error::new(#es_malformed, ErrorKind::ArgsDesFailed));
                        }
                        Err(ww_stream::observe::ObserveError::TooManyRefused) => {
                            return Err(Error::new(#es_refused, ErrorKind::BadIndex));
                        }
                    };
                    let mut entries = [ObserveEntry { index: UNib32(0), mode: ObserveMode::Off }; #n];
                    let len = table.granted(&DESC, &mut entries);
                    let granted = ObserveGranted {
                        entries: wire_weaver::shrink_wrap::RefVec::Slice { slice: &entries[..len] },
                        refused: wire_weaver::shrink_wrap::RefVec::Slice { slice: &refused[..refused_len] },
                        budget: UNib32(table.budget()),
                    };
                    if granted.ser_shrink_wrap(wr).is_err() {
                        *table = before;
                        return Err(Error::new(#es_ser, ErrorKind::ResponseSerFailed));
                    }
                }
            }
        });
        let arrays = self.arrays.iter().map(|path| {
            quote! { [#(UNib32(#path),)* ..] => { return Err(Error::new(#es_array, ErrorKind::OperationNotSupported)); } }
        });
        quote! {
            /// `RequestKind::Observe` on the trait level at `path` (SRV-18): validated and clamped by
            /// `ww_stream::observe::ObserveTable::apply`, answered with `ObserveGranted`.
            fn observe_request(
                &mut self,
                path: &[UNib32],
                cfg: &ww_client_server::observe::ObserveRequest<'_>,
                wr: &mut BufWriter<'_>,
            ) -> Result<(), Error<'static>> {
                #![allow(unreachable_patterns)]
                use ww_client_server::observe::{ObserveEntry, ObserveGranted, ObserveMode, ObserveRefusal, ObserveRefused};
                // more refused entries than this fail the request as a whole
                const REFUSED_MAX: usize = 16;
                let mut refused = [ObserveRefused { index: UNib32(0), reason: ObserveRefusal::NoSuchIndex }; REFUSED_MAX];
                let event_kind_builder = EventKindBuilder::new(wr).map_err(|_| Error::new(#es_ser, ErrorKind::ResponseSerFailed))?;
                match path {
                    #(#arms)*
                    #(#arrays)*
                    _ => return Err(Error::bad_path(#es_path)),
                }
                event_kind_builder.finish_with_kind(EventKindDiscriminants::Value, wr);
                Ok(())
            }
        }
    }

    fn poll_properties(
        &self,
        api_bundle: &ApiBundleOwned,
        use_async: bool,
        medium_ty: &TokenStream,
    ) -> TokenStream {
        let maybe_async = maybe_quote(use_async, quote! { async });
        let maybe_await = maybe_quote(use_async, quote! { .await });
        let (out_trait, send_with) = if use_async {
            (
                quote! { wire_weaver::EventOut },
                quote! { wire_weaver::EventOut::send_with },
            )
        } else {
            (
                quote! { wire_weaver::BlockingEventOut },
                quote! { wire_weaver::BlockingEventOut::send_with_blocking },
            )
        };
        let levels = self.tables().map(|level| {
            let field = &level.field;
            let path = &level.path;
            let n = level.props.len();
            let item_count = level.item_count;
            let indices: Vec<u32> = level.props.iter().map(|p| p.index).collect();
            let batched: Vec<bool> = level.props.iter().map(|p| p.batched).collect();
            let ks: Vec<usize> = (0..n).collect();
            let measure = level.props.iter().map(|p| {
                let ty = ty_def(api_bundle, &p.ty, false, TyPos::Annotation).unwrap();
                match p.model {
                    PropertyModelKind::GetSet => {
                        let get = Ident::new(&format!("get_{}", p.prefixed), Span::call_site());
                        quote! {
                            match ww_stream::observe::max_write_len::<#ty>() {
                                Some(len) => Some(len),
                                None => match self.#get(&mut cx)#maybe_await {
                                    GetResult::Value(value) => {
                                        let value: #ty = value;
                                        ww_stream::observe::measure(&value, &mut *buf)
                                    }
                                    _ => None,
                                },
                            }
                        }
                    }
                    PropertyModelKind::ValueOnChanged => {
                        let f = &p.prefixed;
                        quote! {{
                            let value: &#ty = &self.#f;
                            ww_stream::observe::measure(value, &mut *buf)
                        }}
                    }
                }
            });
            let write = level.props.iter().zip(&ks).map(|(p, k)| {
                let ty = ty_def(api_bundle, &p.ty, false, TyPos::Annotation).unwrap();
                let index = p.index;
                let value = match p.model {
                    PropertyModelKind::GetSet => {
                        let get = Ident::new(&format!("get_{}", p.prefixed), Span::call_site());
                        quote! {
                            let value: #ty = match self.#get(&mut cx)#maybe_await {
                                GetResult::Value(value) => value,
                                _ => break 'write None,
                            };
                            let value = &value;
                        }
                    }
                    PropertyModelKind::ValueOnChanged => {
                        let f = &p.prefixed;
                        quote! { let value: &#ty = &self.#f; }
                    }
                };
                quote! {
                    if sel.is_chosen(#k) {
                        #value
                        if wr.write(#index, value).is_err() {
                            break 'write None;
                        }
                    }
                }
            });
            quote! {
                {
                    const DESC: ww_stream::observe::LevelDesc<#n> = ww_stream::observe::LevelDesc {
                        item_count: #item_count,
                        observable: [#(#indices),*],
                        batched: [#(#batched),*],
                    };
                    let path = [#(UNib32(#path)),*];
                    let limits = self.observe_state().limits;
                    let plan = self.observe_state().#field.plan(now_us, &limits);
                    if !plan.is_empty() {
                        let overhead = ww_stream::observe::event_overhead(&mut *buf, &path, plan.t(), plan.missed())?;
                        let mut sel = plan.selection(&DESC, overhead, buf.len());
                        for &k in plan.edf() {
                            let size = match k {
                                #(#ks => #measure,)*
                                _ => None,
                            };
                            if !sel.offer(k, size) {
                                break;
                            }
                        }
                        let mut sent = None;
                        if !sel.is_empty() {
                            let data_len = 'write: {
                                let Ok(mut wr) = wire_weaver::shrink_wrap::option_any_vec::OptionAnyVecWriter::new(&mut *buf, sel.present(&DESC)) else {
                                    break 'write None;
                                };
                                #(#write)*
                                wr.finish_and_take().ok().map(|data| data.len())
                            };
                            if let Some(data_len) = data_len {
                                let data = &buf[..data_len];
                                let mut event_len = 0;
                                let result = #send_with(&mut cx, |scratch| {
                                    let event = ww_client_server::observe::ser_property_update_event(
                                        scratch, &path, plan.t(), plan.missed(), |b: &mut [u8]| {
                                            let b = b.get_mut(..data.len()).ok_or(ShrinkWrapError::OutOfBoundsWriteRawSlice)?;
                                            b.copy_from_slice(data);
                                            Ok(data.len())
                                        })?;
                                    event_len = event.len();
                                    Ok(event.len())
                                })#maybe_await;
                                match result {
                                    Ok(()) => sent = Some(event_len),
                                    Err(e) => {
                                        first_err.get_or_insert(e);
                                    }
                                }
                            }
                        }
                        self.observe_state().#field.commit(&plan, &sel, now_us, sent);
                    }
                }
            }
        });
        quote! {
            /// Send what observed properties are due (SRV-18): at most one `PropertyUpdate` per trait level, its
            /// properties earliest deadline first, within the level's granted budget and `buf` (as big as the
            /// biggest update the link carries); the rest slips to the next call and counts as missed. `now_us` is
            /// any microsecond clock. Call it often (every millisecond or so), before `poll_streams`. Getters of
            /// values without a size bound are called twice: once to measure, once to send.
            pub #maybe_async fn poll_properties(
                &mut self,
                out: &mut impl #out_trait,
                medium: #medium_ty,
                now_us: u64,
                buf: &mut [u8],
            ) -> Result<(), wire_weaver::SendError> {
                #[allow(unused_mut)]
                let mut cx = wire_weaver::Context::new(0, medium, out);
                let mut first_err = None;
                #(#levels)*
                match first_err {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            }
        }
    }
}

/// `mark_changed_<name>`
pub(crate) fn mark_changed_ident(prefixed: &Ident) -> Ident {
    Ident::new(&format!("mark_changed_{prefixed}"), prefixed.span())
}

/// `<Trait>Update`: a sparse struct with one `Option` per observable property of a level, in the slot of its
/// resource index, the data of the level's `PropertyUpdate`.
fn update_struct(
    api_bundle: &ApiBundleOwned,
    level: &Level,
    name: &str,
    no_alloc: bool,
) -> TokenStream {
    let name = Ident::new(name, Span::call_site());
    let is_lifetime = level
        .props
        .iter()
        .any(|p| p.ty.is_lifetime(api_bundle).unwrap());
    let needs_lifetime = no_alloc && is_lifetime;
    let maybe_lifetime = maybe_quote(needs_lifetime, quote! { <'i> });
    let disambiguate = maybe_quote(
        !needs_lifetime,
        if no_alloc {
            quote! { borrowed, }
        } else {
            quote! { owned, }
        },
    );
    let fields = level.props.iter().map(|p| {
        let ident = &p.ident;
        let index = p.index;
        let ty = ty_def(api_bundle, &p.ty, !no_alloc, TyPos::Def).unwrap();
        quote! {
            #[slot = #index]
            pub #ident: Option<#ty>
        }
    });
    let doc = format!(
        " Data of a `PropertyUpdate` of `{}` (SRV-18): its observable properties by resource index, the ones sent \
        present.",
        level.trait_name
    );
    quote! {
        #[doc = #doc]
        #[wire_weaver::shrink_wrap::derive_shrink_wrap(sparse, #disambiguate derive(Debug))]
        pub struct #name #maybe_lifetime {
            #(#fields),*
        }
    }
}
