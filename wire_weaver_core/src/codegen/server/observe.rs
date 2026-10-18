//! Observed properties on the server (SRV-18): the `Observe` request handler, `ObserveState`, the
//! `<Trait>Update` types, `mark_changed_<name>()` and `poll_properties`, on top of `ww_stream::observe`.
//!
//! Generated only when the API has `+observe` properties, so that servers without them don't need `ww_stream`.
//! A level is observable when it isn't under a resource array (the bound of an array's indices is not known yet,
//! API-29): `Observe` on a level under an array answers `OperationNotSupported`. Properties are observable when
//! declared with `+observe`, not in an array themselves and not `#[optional]`.

use crate::codegen::omit::Omissions;
use crate::codegen::server::scaffold::DEFAULT_BATCH;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{ErrorSeq, add_prefix, maybe_quote, maybe_quote_cl};
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
    /// `<level field>_<name>`: its batcher in `ObserveBatches`
    batch: Ident,
}

impl Level {
    fn has_batched(&self) -> bool {
        self.props.iter().any(|p| p.batched)
    }

    /// `<field>_outbox` of `ObserveState`
    fn outbox(&self) -> Ident {
        Ident::new(&format!("{}_outbox", self.field), Span::call_site())
    }

    /// `<Field>Sampler`
    fn sampler(&self) -> Ident {
        Ident::new(
            &format!("{}Sampler", self.field.to_string().to_case(Case::Pascal)),
            Span::call_site(),
        )
    }
}

/// Bytes of a level's outbox unless `observe_outbox = N` says otherwise: one big message.
const DEFAULT_OUTBOX: usize = 4096;

fn gate_ident(batch: &Ident) -> Ident {
    Ident::new(&format!("{batch}_gate"), batch.span())
}

fn cursor_ident(batch: &Ident) -> Ident {
    Ident::new(&format!("{batch}_cursor"), batch.span())
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
                        batch: Ident::new(&format!("{chain}_{}", item.ident), Span::call_site()),
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

    /// Whether any observable property is `+batched` (API-39): `ObserveBatches`, the samplers and the outboxes
    /// are generated only then.
    pub(crate) fn has_batched(&self) -> bool {
        self.tables().any(Level::has_batched)
    }

    /// `<Trait>` names of the level types (`<Trait>Update`, `<Trait>Samples`), one per table level in order, with
    /// whether this level defines them (the first level of its trait): a trait name used by two crates gets the
    /// crate's name in front.
    fn type_bases(&self) -> Vec<(String, bool)> {
        let mut seen: Vec<(String, String)> = vec![];
        let mut out = vec![];
        for level in self.tables() {
            let key = (level.crate_name.clone(), level.trait_name.clone());
            let first = !seen.contains(&key);
            let other_crate = seen
                .iter()
                .take_while(|k| **k != key)
                .any(|(_, t)| *t == level.trait_name);
            let base = if other_crate {
                format!(
                    "{}{}",
                    level.crate_name.to_case(Case::Pascal),
                    level.trait_name
                )
            } else {
                level.trait_name.clone()
            };
            if first {
                seen.push(key);
            }
            out.push((base, first));
        }
        out
    }

    /// `ObserveState` and the `<Trait>Update` types, outside of the server struct's impl; with `+batched`
    /// properties also `ObserveBatches`, its halves and the samplers (API-39).
    pub(crate) fn items(
        &self,
        api_bundle: &ApiBundleOwned,
        no_alloc: bool,
        observe_batch: Option<usize>,
        observe_outbox: Option<usize>,
    ) -> TokenStream {
        if !self.is_enabled() {
            return quote! {};
        }
        let fields: Vec<&Ident> = self.tables().map(|l| &l.field).collect();
        let ns: Vec<usize> = self.tables().map(|l| l.props.len()).collect();
        let docs = self.tables().map(|l| {
            let path = l.path.iter().map(|p| p.to_string()).collect::<Vec<_>>();
            format!(" `{}` at `/{}`", l.trait_name, path.join("/"))
        });
        let bases = self.type_bases();
        let mut updates = TokenStream::new();
        for (level, (base, first)) in self.tables().zip(&bases) {
            if *first {
                updates.extend(update_struct(
                    api_bundle,
                    level,
                    &format!("{base}Update"),
                    no_alloc,
                ));
            }
        }
        let batched = self.has_batched();
        let (
            state_def,
            state_impl,
            state_ty,
            batches_arg,
            batches_init,
            batches_field,
            reset_gates,
        ) = if batched {
            (
                quote! { <B: ObserveBatchSource = ObserveBatches> },
                quote! { <B: ObserveBatchSource> },
                quote! { <B> },
                quote! { , batches: B },
                quote! { batches, },
                quote! {
                    /// The batches of the `+batched` properties (API-39): an `ObserveBatches`, or its
                    /// consumer half (`ObserveBatches::split`) when the samplers run elsewhere.
                    pub batches: B,
                },
                {
                    let gates = self.batched_props().map(|(_, _, p)| gate_ident(&p.batch));
                    quote! {
                        let c = self.batches.consumers();
                        #(c.#gates.set(None);)*
                    }
                },
            )
        } else {
            Default::default()
        };
        let outboxes: Vec<Ident> = self
            .tables()
            .filter(|l| l.has_batched())
            .map(Level::outbox)
            .collect();
        let outbox_docs = self.tables().filter(|l| l.has_batched()).map(|l| {
            format!(
                " Encoded batches of `{}`'s samples, waiting to be sent",
                l.trait_name
            )
        });
        let batch_items = if batched {
            self.batch_items(api_bundle, &bases, observe_batch, observe_outbox)
        } else {
            quote! {}
        };
        quote! {
            /// What the server keeps about observed properties (SRV-18): one table per trait level with `+observe`
            /// properties, and the limits every `Observe` request is clamped to. The server struct holds it and
            /// hands it out with `fn observe_state(&mut self) -> &mut ObserveState`.
            pub struct ObserveState #state_def {
                pub limits: ww_stream::observe::ObserveLimits,
                #(
                    #[doc = #docs]
                    pub #fields: ww_stream::observe::ObserveTable<#ns>,
                )*
                #(
                    #[doc = #outbox_docs]
                    pub #outboxes: ww_stream::batch::Outbox<OBSERVE_OUTBOX>,
                )*
                #batches_field
            }

            impl #state_impl ObserveState #state_ty {
                /// Nothing observed yet.
                pub const fn new(limits: ww_stream::observe::ObserveLimits #batches_arg) -> Self {
                    ObserveState {
                        limits,
                        #(#fields: ww_stream::observe::ObserveTable::new(),)*
                        #(#outboxes: ww_stream::batch::Outbox::new(),)*
                        #batches_init
                    }
                }

                /// Stop observing everything, e.g., when the link went down.
                pub fn reset(&mut self) {
                    #(self.#fields.reset();)*
                    #(self.#outboxes.clear();)*
                    #reset_gates
                }

                /// Bytes per second granted to all levels together.
                pub fn budget_in_use(&self) -> u32 {
                    0u32 #(.saturating_add(self.#fields.budget()))*
                }
            }

            #updates
            #batch_items
        }
    }

    /// The `+batched` properties: level, slot in it, property.
    fn batched_props(&self) -> impl Iterator<Item = (&Level, usize, &Prop)> {
        self.tables().flat_map(|l| {
            l.props
                .iter()
                .enumerate()
                .filter(|(_, p)| p.batched)
                .map(move |(k, p)| (l, k, p))
        })
    }

    /// `ObserveBatches`, `ObserveBatchConsumers`, `ObserveBatchSource`, `ObserveSamplers`, a `<Level>Sampler` per
    /// level and a `<Trait>Samples` per trait with `+batched` properties (API-39).
    fn batch_items(
        &self,
        api_bundle: &ApiBundleOwned,
        bases: &[(String, bool)],
        observe_batch: Option<usize>,
        observe_outbox: Option<usize>,
    ) -> TokenStream {
        let batch = observe_batch.unwrap_or(DEFAULT_BATCH);
        let outbox = observe_outbox.unwrap_or(DEFAULT_OUTBOX);
        let mut errors = TokenStream::new();
        let mut item_ty = |p: &Prop| -> TokenStream {
            if p.ty.is_lifetime(api_bundle).unwrap_or(false) {
                let msg = format!(
                    "batched property {}: its type borrows (bytes, a string, a vector): a batch needs values of a \
                     bounded size, see `series`",
                    p.ident
                );
                errors.extend(quote! { compile_error!(#msg); });
            }
            ty_def(api_bundle, &p.ty, false, TyPos::Annotation).unwrap()
        };
        let names: Vec<Ident> = self
            .batched_props()
            .map(|(_, _, p)| p.batch.clone())
            .collect();
        let gates: Vec<Ident> = names.iter().map(gate_ident).collect();
        let cursors: Vec<Ident> = names.iter().map(cursor_ident).collect();
        let tys: Vec<TokenStream> = self.batched_props().map(|(_, _, p)| item_ty(p)).collect();
        let docs = self
            .batched_props()
            .map(|(l, _, p)| format!(" Samples of `{}` of `{}`", p.ident, l.trait_name));
        let item = |ty: &TokenStream| quote! { ww_stream::batch::Timed<#ty> };
        let items: Vec<TokenStream> = tys.iter().map(item).collect();

        let mut samples_structs = TokenStream::new();
        let mut samplers = TokenStream::new();
        let mut sampler_fields = vec![];
        let mut sampler_inits = vec![];
        for (level, (base, first)) in self.tables().zip(bases) {
            if !level.has_batched() {
                continue;
            }
            let samples = Ident::new(&format!("{base}Samples"), Span::call_site());
            let props: Vec<&Prop> = level.props.iter().filter(|p| p.batched).collect();
            let idents: Vec<&Ident> = props.iter().map(|p| &p.ident).collect();
            let ptys: Vec<TokenStream> = props.iter().map(|p| item_ty(p)).collect();
            if *first {
                let doc = format!(
                    " One sample of the `+batched` properties of `{}` (API-39), for `<Level>Sampler::sample`.",
                    level.trait_name
                );
                samples_structs.extend(quote! {
                    #[doc = #doc]
                    #[derive(Debug, Clone)]
                    pub struct #samples {
                        #(pub #idents: #ptys,)*
                    }
                });
            }
            let sampler = level.sampler();
            let field = &level.field;
            let pnames: Vec<&Ident> = props.iter().map(|p| &p.batch).collect();
            let pgates: Vec<Ident> = pnames.iter().map(|n| gate_ident(n)).collect();
            let pcursors: Vec<Ident> = pnames.iter().map(|n| cursor_ident(n)).collect();
            let pitems: Vec<TokenStream> = ptys.iter().map(item).collect();
            let doc = format!(
                " Takes the samples of `{}` at `/{}` (API-39): the producer half of its batches, for the context \
                 that has the values (a control loop, an interrupt).",
                level.trait_name,
                level
                    .path
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            );
            samplers.extend(quote! {
                #[doc = #doc]
                pub struct #sampler<'a> {
                    #(
                        #pnames: ww_stream::batch::Producer<'a, #pitems, OBSERVE_BATCH>,
                        #pgates: &'a ww_stream::observe::SampleGate,
                        #pcursors: &'a mut ww_stream::observe::GateCursor,
                    )*
                }

                impl #sampler<'_> {
                    /// One sample of the level, taken at device time `t`: each value a host observes in
                    /// `Samples` mode goes into its batches (a copy and a few atomic operations, never an encode
                    /// or a wait: fine in a control loop or an interrupt), the others are dropped.
                    /// `poll_properties` sends the batches. Returns the samples lost because the event loop
                    /// didn't keep up.
                    pub fn sample(&mut self, t: ww_stream::observe::Second, values: #samples) -> usize {
                        let #samples { #(#idents),* } = values;
                        let mut lost = 0;
                        #(
                            if self.#pcursors.take(self.#pgates, &t) {
                                let item = ww_stream::batch::Timed::new(t, #idents);
                                if let ww_stream::batch::Seal::Lost { items, .. } =
                                    ww_stream::batch::Push::push(&mut self.#pnames, &item)
                                {
                                    lost += items;
                                }
                            }
                        )*
                        lost
                    }

                    /// Whether a host observes any property of the level in `Samples` mode: skip working out
                    /// the values otherwise.
                    pub fn is_sampling(&self) -> bool {
                        false #(|| self.#pgates.is_open())*
                    }
                }
            });
            sampler_fields.push(quote! { pub #field: #sampler<'a>, });
            sampler_inits.push(quote! {
                #field: #sampler {
                    #(
                        #pnames: ww_stream::batch::Batcher::producer(&mut self.#pnames),
                        #pgates: &self.#pgates,
                        #pcursors: &mut self.#pcursors,
                    )*
                },
            });
        }
        let split_inits = self.tables().filter(|l| l.has_batched()).map(|level| {
            let sampler = level.sampler();
            let field = &level.field;
            let pnames: Vec<&Ident> = level
                .props
                .iter()
                .filter(|p| p.batched)
                .map(|p| &p.batch)
                .collect();
            let pgates: Vec<Ident> = pnames.iter().map(|n| gate_ident(n)).collect();
            let pcursors: Vec<Ident> = pnames.iter().map(|n| cursor_ident(n)).collect();
            quote! {
                #field: #sampler {
                    #(
                        #pnames: #pnames.0,
                        #pgates: &self.#pgates,
                        #pcursors: &mut self.#pcursors,
                    )*
                },
            }
        });
        quote! {
            #errors
            /// Samples per batch of each `+batched` property (`observe_batch = N` of `ww_codegen!`).
            pub const OBSERVE_BATCH: usize = #batch;
            /// Bytes of the outbox of each level with `+batched` properties (`observe_outbox = N`).
            pub const OBSERVE_OUTBOX: usize = #outbox;

            #samples_structs

            /// The batches of every `+batched` property (API-39): two stores of `OBSERVE_BATCH` samples each,
            /// filled by the samplers, encoded by `poll_properties`. [split](Self::split) it to sample in another
            /// context (an interrupt, another core), or put it into `ObserveState` as it is and sample through
            /// [samplers](Self::samplers).
            pub struct ObserveBatches {
                #(
                    #[doc = #docs]
                    #names: ww_stream::batch::Batcher<#items, OBSERVE_BATCH>,
                    #gates: ww_stream::observe::SampleGate,
                    #cursors: ww_stream::observe::GateCursor,
                )*
            }

            impl ObserveBatches {
                /// Batches encoded with `policy`: `ww_stream::batch::DEFAULT_POLICY` (every codec on `std`, delta
                /// and packed on `no_std`, so a firmware links only those), or another value of its type.
                pub fn new(policy: ww_stream::batch::DefaultPolicy) -> Self {
                    ObserveBatches {
                        #(
                            #names: ww_stream::batch::Batcher::new(policy),
                            #gates: ww_stream::observe::SampleGate::new(),
                            #cursors: ww_stream::observe::GateCursor::new(),
                        )*
                    }
                }

                /// The samplers of each level and the consumer half for `ObserveState`, for samplers that run in
                /// another context: split a batches that outlives both (a `static`, a `StaticCell`).
                pub fn split(&mut self) -> (ObserveSamplers<'_>, ObserveBatchConsumers<'_>) {
                    #(let #names = ww_stream::batch::Batcher::split(&mut self.#names);)*
                    (
                        ObserveSamplers { #(#split_inits)* },
                        ObserveBatchConsumers {
                            #(
                                #names: #names.1,
                                #gates: &self.#gates,
                            )*
                        },
                    )
                }

                /// The samplers, for a device that samples from the task that polls.
                pub fn samplers(&mut self) -> ObserveSamplers<'_> {
                    ObserveSamplers { #(#sampler_inits)* }
                }
            }

            /// The sampler of each level with `+batched` properties, see `ObserveBatches::split`.
            pub struct ObserveSamplers<'a> {
                #(#sampler_fields)*
            }

            #samplers

            /// The consumer half of `ObserveBatches`, what `poll_properties` encodes from.
            pub struct ObserveBatchConsumers<'a> {
                #(
                    #names: ww_stream::batch::Consumer<'a, #items, OBSERVE_BATCH>,
                    #gates: &'a ww_stream::observe::SampleGate,
                )*
            }

            /// Where `ObserveState` takes the consumer half of the batches from: `ObserveBatches` itself, or the
            /// `ObserveBatchConsumers` split off it.
            pub trait ObserveBatchSource {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_>;
            }

            impl ObserveBatchSource for ObserveBatches {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_> {
                    ObserveBatchConsumers {
                        #(
                            #names: ww_stream::batch::Batcher::consumer(&mut self.#names),
                            #gates: &self.#gates,
                        )*
                    }
                }
            }

            impl ObserveBatchSource for ObserveBatchConsumers<'_> {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_> {
                    ObserveBatchConsumers {
                        #(
                            #names: ww_stream::batch::BatchSource::consumer(&mut self.#names),
                            #gates: self.#gates,
                        )*
                    }
                }
            }

            impl<B: ObserveBatchSource> ObserveState<B> {
                /// The samplers of the batches the state owns (API-39).
                pub fn samplers(&mut self) -> ObserveSamplers<'_>
                where
                    B: core::borrow::BorrowMut<ObserveBatches>,
                {
                    self.batches.borrow_mut().samplers()
                }
            }
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
            // samplers take what is observed in `Samples` mode now; batches of the others go (API-39)
            let sync_gates = maybe_quote_cl(level.has_batched(), || {
                let outbox = level.outbox();
                let syncs = level.props.iter().enumerate().filter(|(_, p)| p.batched).map(|(k, p)| {
                    let gate = gate_ident(&p.batch);
                    quote! {
                        match state.#field.samples(#k) {
                            Some((every, _)) => c.#gate.set(Some(every)),
                            None => {
                                c.#gate.set(None);
                                state.#outbox.remove_all_of(#k as u16);
                            }
                        }
                    }
                });
                quote! {
                    let state = self.observe_state();
                    let c = state.batches.consumers();
                    #(#syncs)*
                }
            });
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
                    #sync_gates
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
            let outbox = level.outbox();
            // the value of slot `k` as `value: &T`, from its getter or field
            let value_of = |p: &Prop, ty: &TokenStream| match p.model {
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
            let measure = level.props.iter().enumerate().map(|(k, p)| {
                let ty = ty_def(api_bundle, &p.ty, false, TyPos::Annotation).unwrap();
                // `Batched::One` of a `+batched` property: its tag in front
                let (max_len, measured) = if p.batched {
                    (
                        quote! { ww_stream::observe::max_batched_one_len::<#ty>() },
                        quote! { ww_stream::observe::measure(&ww_client_server::observe::BatchedOne(value), &mut *buf) },
                    )
                } else {
                    (
                        quote! { ww_stream::observe::max_write_len::<#ty>() },
                        quote! { ww_stream::observe::measure(value, &mut *buf) },
                    )
                };
                let one = match p.model {
                    PropertyModelKind::GetSet => {
                        let get = Ident::new(&format!("get_{}", p.prefixed), Span::call_site());
                        quote! {
                            match #max_len {
                                Some(len) => Some(len),
                                None => match self.#get(&mut cx)#maybe_await {
                                    GetResult::Value(value) => {
                                        let value: #ty = value;
                                        let value = &value;
                                        #measured
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
                            #measured
                        }}
                    }
                };
                if p.batched {
                    quote! {
                        if self.observe_state().#field.samples(#k).is_some() {
                            self.observe_state()
                                .#outbox
                                .first_of(#k as u16)
                                .map(|e| ww_stream::observe::batch_slot_len(e.bytes.len()))
                        } else {
                            #one
                        }
                    }
                } else {
                    one
                }
            });
            let write = level.props.iter().zip(&ks).map(|(p, k)| {
                let ty = ty_def(api_bundle, &p.ty, false, TyPos::Annotation).unwrap();
                let index = p.index;
                let value = value_of(p, &ty);
                if p.batched {
                    quote! {
                        if sel.is_chosen(#k) {
                            if self.observe_state().#field.samples(#k).is_some() {
                                let state = self.observe_state();
                                let Some(entry) = state.#outbox.first_of(#k as u16) else {
                                    break 'write None;
                                };
                                if wr.write(#index, &ww_client_server::observe::BatchedRaw(entry.bytes)).is_err() {
                                    break 'write None;
                                }
                            } else {
                                #value
                                if wr.write(#index, &ww_client_server::observe::BatchedOne(value)).is_err() {
                                    break 'write None;
                                }
                            }
                        }
                    }
                } else {
                    quote! {
                        if sel.is_chosen(#k) {
                            #value
                            if wr.write(#index, value).is_err() {
                                break 'write None;
                            }
                        }
                    }
                }
            });
            let has_batched = level.has_batched();
            let batched_ks: Vec<usize> = level
                .props
                .iter()
                .enumerate()
                .filter(|(_, p)| p.batched)
                .map(|(k, _)| k)
                .collect();
            let batched_names: Vec<&Ident> = level.props.iter().filter(|p| p.batched).map(|p| &p.batch).collect();
            // sealed batches into the outbox, encoded once; batches of properties not in `Samples` mode go
            let encode = maybe_quote_cl(has_batched, || {
                quote! {
                    let overhead = ww_stream::observe::batch_overhead(&mut *buf, &path)?;
                    let budget = ww_stream::observe::batch_budget(
                        &DESC,
                        overhead,
                        buf.len(),
                        OBSERVE_OUTBOX - ww_stream::batch::OUTBOX_ENTRY_HEADER,
                    );
                    let state = self.observe_state();
                    let mut c = state.batches.consumers();
                    #(
                        match state.#field.samples(#batched_ks) {
                            Some((_, max_age)) => {
                                let polled = c.#batched_names.poll(now_us, Some(max_age as u64), budget);
                                let missed = match state.#outbox.push_from(#batched_ks as u16, &mut c.#batched_names, &mut *scratch) {
                                    Ok(pushed) => pushed.dropped + pushed.lost,
                                    // a value that doesn't serialize: its batch goes
                                    Err(_) => {
                                        let (items, lost) = c.#batched_names.discard_sealed();
                                        items as u64 + lost
                                    }
                                };
                                state.#field.add_missed(polled.lost + missed);
                            }
                            None => {
                                c.#batched_names.flush();
                                c.#batched_names.discard_sealed();
                            }
                        }
                    )*
                }
            });
            let plan = if has_batched {
                quote! {{
                    let state = self.observe_state();
                    let outbox = &state.#outbox;
                    state.#field.plan_with(now_us, &limits, |k| match k {
                        #(#batched_ks => outbox.first_of(#batched_ks as u16).is_some(),)*
                        _ => false,
                    })
                }}
            } else {
                quote! { self.observe_state().#field.plan(now_us, &limits) }
            };
            // the update's `t` is the time of its values: none when it carries only batches, whose samples have
            // their own
            let t = if has_batched {
                quote! {
                    let one_value = (0..#n).any(|k| {
                        sel.is_chosen(k) && (!DESC.batched[k] || self.observe_state().#field.samples(k).is_none())
                    });
                    let t = if one_value { plan.t() } else { None };
                }
            } else {
                quote! { let t = plan.t(); }
            };
            // sent batches leave the outbox; while more wait (and the budget lasts), another update
            let next = if has_batched {
                quote! {
                    let mut more = false;
                    if sent.is_some() {
                        let state = self.observe_state();
                        #(
                            if sel.is_chosen(#batched_ks) && state.#field.samples(#batched_ks).is_some() {
                                state.#outbox.remove_first_of(#batched_ks as u16);
                                more = true;
                            }
                        )*
                    }
                    rounds += 1;
                    if !more || rounds >= ww_stream::observe::MAX_UPDATES_PER_POLL {
                        break;
                    }
                }
            } else {
                quote! { break; }
            };
            let rounds = maybe_quote(has_batched, quote! { let mut rounds = 0usize; });
            quote! {
                {
                    const DESC: ww_stream::observe::LevelDesc<#n> = ww_stream::observe::LevelDesc {
                        item_count: #item_count,
                        observable: [#(#indices),*],
                        batched: [#(#batched),*],
                    };
                    let path = [#(UNib32(#path)),*];
                    let limits = self.observe_state().limits;
                    #encode
                    #rounds
                    loop {
                        let plan = #plan;
                        if plan.is_empty() && plan.missed().is_none() {
                            break;
                        }
                        let overhead = ww_stream::observe::event_overhead(&mut *buf, &path, plan.t(), plan.missed())?;
                        // nothing due: the missed count goes alone, or waits for budget
                        let missed_only = plan.report_missed(&DESC, overhead);
                        if plan.is_empty() && !missed_only {
                            break;
                        }
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
                        #t
                        if !sel.is_empty() || missed_only {
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
                                        scratch, &path, t, plan.missed(), |b: &mut [u8]| {
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
                        #next
                    }
                }
            }
        });
        let (scratch_generic, scratch_arg, scratch_doc) = if self.has_batched() {
            let doc = " Batches of `+batched` properties observed in `Samples` mode (API-39) are encoded into their \
                level's outbox first (`scratch` is where their samples are rebuilt, as big as the biggest batch, \
                see `ww_stream::batch::scratch_bytes`), then sent while they wait and the budget lasts, more than one \
                update per level if need be.";
            (
                quote! { <const SB: usize> },
                quote! { , scratch: &mut ww_stream::batch::Scratch<SB> },
                quote! { #[doc = ""] #[doc = #doc] },
            )
        } else {
            (quote! {}, quote! {}, quote! {})
        };
        quote! {
            /// Send what observed properties are due (SRV-18): a `PropertyUpdate` per trait level, its
            /// properties earliest deadline first, within the level's granted budget and `buf` (as big as the
            /// biggest update the link carries); the rest slips to the next call and counts as missed. `now_us` is
            /// any microsecond clock. Call it often (every millisecond or so), before `poll_streams`. Getters of
            /// values without a size bound are called twice: once to measure, once to send.
            #scratch_doc
            pub #maybe_async fn poll_properties #scratch_generic(
                &mut self,
                out: &mut impl #out_trait,
                medium: #medium_ty,
                now_us: u64,
                buf: &mut [u8]
                #scratch_arg
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

/// The slot type of an observable property in its level's update: `T`, or for a `+batched` one (API-39)
/// `Batched<'i, T>` (`BatchedOwned<T>` when `owned`).
pub(crate) fn batched_slot_ty(batched: bool, ty: TokenStream, owned: bool) -> TokenStream {
    match (batched, owned) {
        (false, _) => ty,
        (true, false) => quote! { ww_client_server::observe::Batched<'i, #ty> },
        (true, true) => quote! { ww_client_server::observe::BatchedOwned<#ty> },
    }
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
    // a `Batched<'i, T>` slot borrows its series
    let is_lifetime = level
        .props
        .iter()
        .any(|p| p.batched || p.ty.is_lifetime(api_bundle).unwrap());
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
        let ty = batched_slot_ty(p.batched, ty, !no_alloc);
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
