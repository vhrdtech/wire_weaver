//! Observed properties on the server (SRV-18): the `Observe` request handler, `ObserveState`, the
//! `<Trait>Update` types, `mark_changed_<name>()` and `poll_properties`, on top of `ww_stream::observe`.
//!
//! Generated only when the API has `+observe` properties, so that servers without them don't need `ww_stream`.
//! A level is observable when it isn't under a resource array (the bound of an array's indices is not known yet,
//! API-29): `Observe` on a level under an array answers `OperationNotSupported`. Properties are observable when
//! declared with `+observe`, not in an array themselves and not `#[optional]`.

use crate::codegen::CodegenBundle;
use crate::codegen::omit::Omissions;
use crate::codegen::server::scaffold::DEFAULT_BATCH;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{
    Handlers, add_prefix, dotted, fixed_err_seq, maybe_quote, maybe_quote_cl,
};
use crate::property_model::{PropertyModel, PropertyModelKind};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use ww_self::{ApiItemKindOwned, ApiLevelOwned, Multiplicity, PropertyAccess, TypeOwned};

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
    /// Sampled through observe cells (`observe_cells = ".."`, API-50) instead of `<Level>Sampler::sample`
    cells: bool,
}

struct Prop {
    index: u32,
    ident: Ident,
    /// With the trait item chain prefix: handler and field names
    prefixed: Ident,
    /// Dotted item path, for `handler_model` (SRV-21)
    dotted: String,
    ty: TypeOwned,
    model: PropertyModelKind,
    /// `+batched` (API-39)
    batched: bool,
}

impl Level {
    fn has_batched(&self) -> bool {
        self.props.iter().any(|p| p.batched)
    }

    /// Table slots of the `+batched` properties, in order: column `c` of the level's rows is slot `batched_ks()[c]`.
    fn batched_ks(&self) -> Vec<usize> {
        self.props
            .iter()
            .enumerate()
            .filter(|(_, p)| p.batched)
            .map(|(k, _)| k)
            .collect()
    }

    /// `<field>_outbox` of `ObserveState`
    fn outbox(&self) -> Ident {
        Ident::new(&format!("{}_outbox", self.field), Span::call_site())
    }

    /// `<field>`'s batcher in `ObserveBatches`
    fn batches(&self) -> Ident {
        self.field.clone()
    }

    /// `<Field>Sampler`
    fn sampler(&self) -> Ident {
        self.pascal("Sampler")
    }

    /// `<Field><suffix>`
    fn pascal(&self, suffix: &str) -> Ident {
        Ident::new(
            &format!("{}{suffix}", self.field.to_string().to_case(Case::Pascal)),
            Span::call_site(),
        )
    }

    /// `<field>_cells` of `ObserveBatches`: the reader of a cells level
    fn reader(&self) -> Ident {
        Ident::new(&format!("{}_cells", self.field), Span::call_site())
    }

    /// `<field>_src` of `ObserveBatchConsumers`: what a cells level reads its rows from
    fn src(&self) -> Ident {
        Ident::new(&format!("{}_src", self.field), Span::call_site())
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
        api_bundle: &CodegenBundle,
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
        api_bundle: &CodegenBundle,
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
                        PropertyAccess::ReadOnly { observe: true }
                        | PropertyAccess::ReadWrite { observe: true },
                    batched,
                    ..
                } if flat => {
                    let ident = Ident::new(&item.ident, Span::call_site());
                    props.push(Prop {
                        index: item.id.0,
                        prefixed: add_prefix(prefix, &ident),
                        dotted: dotted(item_path, &item.ident),
                        model: property_model
                            .pick(&item.ident)
                            .unwrap_or(PropertyModelKind::GetSet),
                        ident,
                        ty: ty.clone(),
                        batched: batched.is_some(),
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
            cells: false,
        });
    }

    /// Marks the levels named in `observe_cells` (their `ObserveState` field) as cells levels (API-50): a
    /// compile error for a name that isn't a level with `+batched` properties.
    pub(crate) fn use_cells(&mut self, names: &[String]) -> TokenStream {
        let mut errors = TokenStream::new();
        for name in names {
            match self
                .levels
                .iter_mut()
                .find(|l| l.field == name && l.has_batched())
            {
                Some(level) => level.cells = true,
                None => {
                    let known = self.batched_levels();
                    let msg = format!(
                        "observe_cells: `{name}` is not a level with +batched properties; levels: {}",
                        if known.is_empty() {
                            "none".to_string()
                        } else {
                            known.join(", ")
                        }
                    );
                    errors.extend(quote! { compile_error!(#msg); });
                }
            }
        }
        errors
    }

    /// `ObserveState` fields of the levels with `+batched` properties: what `observe_cells` can name.
    pub(crate) fn batched_levels(&self) -> Vec<String> {
        self.tables()
            .filter(|l| l.has_batched())
            .map(|l| l.field.to_string())
            .collect()
    }

    /// Whether some level is sampled through observe cells: `ObserveBatches` then has a lifetime.
    fn has_cells(&self) -> bool {
        self.tables().any(|l| l.cells)
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
        api_bundle: &CodegenBundle,
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
                if self.has_cells() {
                    quote! { <B: ObserveBatchSource = ObserveBatches<'static>> }
                } else {
                    quote! { <B: ObserveBatchSource = ObserveBatches> }
                },
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
                    let gates = self
                        .tables()
                        .filter(|l| l.has_batched())
                        .map(|l| gate_ident(&l.batches()));
                    // rows taken before the gates closed go too: a later observer would get them with old times
                    let drop_rows = self.tables().filter(|l| l.has_batched()).map(|l| {
                        let batches = l.batches();
                        if l.cells {
                            let src = l.src();
                            quote! { c.#batches.discard_all(c.#src); }
                        } else {
                            quote! {
                                c.#batches.flush();
                                c.#batches.discard_sealed();
                            }
                        }
                    });
                    quote! {
                        let mut c = self.batches.consumers();
                        #(c.#gates.set(None);)*
                        #(#drop_rows)*
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
        let (batches_pat, batches_write) = if batched {
            (
                quote! { batches: _ },
                quote! { core::ptr::addr_of_mut!((*at).batches).write(batches); },
            )
        } else {
            (quote! {}, quote! {})
        };
        let level_access = self.level_access();
        let batch_items = if batched {
            self.batch_items(api_bundle, &bases, observe_batch, observe_outbox)
        } else {
            quote! {}
        };
        let signal_items = self.signal_items(&state_impl, &state_ty);
        quote! {
            /// What the server keeps about observed properties (SRV-18): one table per trait level with `+observe`
            /// properties, and the limits every `Observe` request is clamped to. The server struct holds it and
            /// hands it out with `fn observe_state(&mut self) -> &mut ObserveState`.
            pub struct ObserveState #state_def {
                pub limits: ww_stream::observe::ObserveLimits,
                #(
                    #[doc = #docs]
                    pub #fields: ww_stream::observe::ObserveTable<[ww_stream::observe::Slot; #ns]>,
                )*
                #(
                    #[doc = #outbox_docs]
                    pub #outboxes: ww_stream::batch::Outbox<OBSERVE_OUTBOX>,
                )*
                #batches_field
                /// Changes marked and wakes raised outside the server state (SRV-25), see
                /// [with_signal](Self::with_signal).
                signal: Option<&'static ObserveSignal>,
            }

            impl #state_impl ObserveState #state_ty {
                /// Nothing observed yet.
                pub const fn new(limits: ww_stream::observe::ObserveLimits #batches_arg) -> Self {
                    ObserveState {
                        limits,
                        #(#fields: ww_stream::observe::ObserveTable::new(),)*
                        #(#outboxes: ww_stream::batch::Outbox::new(),)*
                        #batches_init
                        signal: None,
                    }
                }

                /// [new](Self::new) in place: builds the state in `slot` (a `static`'s, `StaticCell::uninit()`)
                /// and returns it, the tables and outboxes written where they stay, never on the stack. With
                /// `ObserveBatches::init` for the batches, a firmware sets up observe with a few hundred bytes of
                /// stack at most.
                pub fn init<'s>(
                    slot: &'s mut core::mem::MaybeUninit<Self>,
                    limits: ww_stream::observe::ObserveLimits #batches_arg
                ) -> &'s mut Self {
                    // every field is written below: a new one fails to compile here until it is
                    let _ = |s: Self| {
                        let ObserveState { limits: _, #(#fields: _,)* #(#outboxes: _,)* signal: _, #batches_pat } = s;
                    };
                    let at = slot.as_mut_ptr();
                    // SAFETY: every field written in place, each through its own pointer
                    unsafe {
                        core::ptr::addr_of_mut!((*at).limits).write(limits);
                        #(
                            ww_stream::observe::ObserveTable::init(
                                &mut *core::ptr::addr_of_mut!((*at).#fields)
                                    .cast::<core::mem::MaybeUninit<ww_stream::observe::ObserveTable<[ww_stream::observe::Slot; #ns]>>>(),
                            );
                        )*
                        #(
                            ww_stream::batch::Outbox::init(
                                &mut *core::ptr::addr_of_mut!((*at).#outboxes)
                                    .cast::<core::mem::MaybeUninit<ww_stream::batch::Outbox<OBSERVE_OUTBOX>>>(),
                            );
                        )*
                        #batches_write
                        core::ptr::addr_of_mut!((*at).signal).write(None);
                        slot.assume_init_mut()
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

                /// Whether any property of any level is observed.
                pub fn is_active(&self) -> bool {
                    false #(|| self.#fields.is_active())*
                }

                #level_access
            }

            #updates
            #batch_items
            #signal_items
        }
    }

    /// `ObserveSignal` (SRV-25): a changed bit per observable property and the server task's wake, for a `static`
    /// any task or interrupt marks changes through; `ObserveState::take_changes` moves the bits into the tables.
    fn signal_items(&self, state_impl: &TokenStream, state_ty: &TokenStream) -> TokenStream {
        let mut marks = vec![];
        let mut takes = vec![];
        let mut i = 0usize;
        for level in self.tables() {
            let field = &level.field;
            for (k, p) in level.props.iter().enumerate() {
                let name = mark_changed_ident(&p.prefixed);
                let doc = format!(
                    " `{}` changed outside the server state: sent to hosts that observe it on change, the server \
                    task woken for it. Callable from any task or interrupt.",
                    p.ident
                );
                marks.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #name(&self) {
                        self.0.mark(#i);
                    }
                });
                let (w, bit) = (i / 32, 1u32 << (i % 32));
                takes.push((
                    w,
                    quote! {
                        if bits & #bit != 0 {
                            self.#field.mark_changed(#k);
                        }
                    },
                ));
                i += 1;
            }
        }
        let words = i.div_ceil(32);
        let level_wakes = self.tables().map(|level| {
            let field = &level.field;
            let min = quote! {
                if let Some(at) = at {
                    next = Some(next.map_or(at, |n: u64| n.min(at)));
                }
            };
            if !level.has_batched() {
                return quote! {{
                    let at = self.#field.next_wake(now_us, &|_| false);
                    #min
                }};
            }
            let outbox = level.outbox();
            let batches = level.batches();
            let batches_wake = if level.cells {
                let src = level.src();
                quote! { c.#batches.next_wake(c.#src, now_us, Some(max_age as u64)) }
            } else {
                quote! { c.#batches.next_wake(now_us, Some(max_age as u64)) }
            };
            quote! {{
                // a slot in `Samples` mode is due while the outbox holds a batch of it, as in `plan_with`
                let at = {
                    let oldest = self.#outbox.oldest();
                    let column_at = core::cell::Cell::new(0usize);
                    self.#field.next_wake(now_us, &|k| {
                        oldest.is_some_and(|b| {
                            let mut a = column_at.get();
                            let found = b.column_after(k as u16, &mut a).is_some();
                            column_at.set(a);
                            found
                        })
                    })
                };
                #min
                // the batches being filled: sealed or started since the last poll, or as old as allowed
                if let Some((_, max_age)) = self.#field.level_samples() {
                    let mut c = self.batches.consumers();
                    let at = #batches_wake;
                    #min
                }
            }}
        });
        let takes = (0..words).map(|w| {
            let ifs = takes.iter().filter(|(tw, _)| *tw == w).map(|(_, t)| t);
            quote! {
                let bits = signal.take(#w);
                if bits != 0 {
                    #(#ifs)*
                }
            }
        });
        quote! {
            /// Changes to observed properties made outside the server state, and the server task's wake (SRV-25):
            /// a `static` that any task or interrupt marks a property changed through
            /// (`SIGNAL.mark_changed_<name>()`) or wakes the server task with (`wake()`, `as_wake()` for a sampler
            /// or a cells writer). The server task selects on `wait()` next to the server's `wait_until(next_wake)`,
            /// and `poll_properties` takes the changes (`ObserveState::with_signal`).
            pub struct ObserveSignal(ww_stream::wake::Signal<#words>);

            impl ObserveSignal {
                pub const fn new() -> Self {
                    ObserveSignal(ww_stream::wake::Signal::new())
                }

                #(#marks)*
            }

            impl Default for ObserveSignal {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl core::ops::Deref for ObserveSignal {
                type Target = ww_stream::wake::Signal<#words>;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }

            impl #state_impl ObserveState #state_ty {
                /// Marks the properties `signal` says changed since the last call, and clears them.
                pub fn take_changes(&mut self, signal: &ObserveSignal) {
                    #(#takes)*
                }

                /// Takes changes from `signal` (SRV-25): `poll_properties` and [next_wake](Self::next_wake) mark
                /// what it says changed (`SIGNAL.mark_changed_<name>()` from any task or interrupt), and the server
                /// task waits on `signal.wait()` next to its next-wake timer.
                pub fn with_signal(&mut self, signal: &'static ObserveSignal) -> &mut Self {
                    self.signal = Some(signal);
                    self
                }

                /// The signal given to [with_signal](Self::with_signal).
                pub fn signal(&self) -> Option<&'static ObserveSignal> {
                    self.signal
                }

                /// When `poll_properties` next has something to do (SRV-25), at the earliest `now_us`: a property
                /// due (periodic, a change after its `min`, a beat), a level's bucket refilled for what it held
                /// back, a batch of samples sealed or as old as the host allows. `None` while nothing is observed
                /// or nothing comes due without a change. The server task sleeps until then, or until the server
                /// or the signal wakes it; ask again after every wake, a change or a request can make it earlier.
                pub fn next_wake(&mut self, now_us: u64) -> Option<u64> {
                    if let Some(signal) = self.signal {
                        self.take_changes(signal);
                    }
                    // the server task asks after every wake: with nothing observed, don't walk the levels and
                    // their batches (FW-40 on Donguru: USB requests 17 % slower without this)
                    if !self.is_active() {
                        return None;
                    }
                    let mut next: Option<u64> = None;
                    #(#level_wakes)*
                    next.map(|at| at.max(now_us))
                }
            }
        }
    }

    /// `ObserveState::level` / `oldest_batch` / `remove_oldest_batch`: a level's table and outbox by its number,
    /// what the shared loop of `poll_properties` (SRV-24) reaches them through.
    fn level_access(&self) -> TokenStream {
        let levels: Vec<&Level> = self.tables().collect();
        let Some(last) = levels.len().checked_sub(1) else {
            return quote! {};
        };
        let pat = |i: usize| {
            if i == last {
                quote! { _ }
            } else {
                quote! { #i }
            }
        };
        let level = levels.iter().enumerate().map(|(i, level)| {
            let pat = pat(i);
            let field = &level.field;
            let oldest = if level.has_batched() {
                let outbox = level.outbox();
                quote! { self.#outbox.oldest() }
            } else {
                quote! { None }
            };
            quote! { #pat => (&mut self.#field as &mut ww_stream::observe::ObserveTable, #oldest), }
        });
        let oldest = levels
            .iter()
            .enumerate()
            .filter(|(_, l)| l.has_batched())
            .map(|(i, level)| {
                let outbox = level.outbox();
                quote! { #i => self.#outbox.oldest(), }
            });
        let remove = levels
            .iter()
            .enumerate()
            .filter(|(_, l)| l.has_batched())
            .map(|(i, level)| {
                let outbox = level.outbox();
                quote! { #i => self.#outbox.remove_oldest(), }
            });
        quote! {
            /// The table of trait level `level` (in `poll_properties` order; the last one beyond) and the oldest
            /// batch of samples in its outbox (API-39).
            pub fn level(
                &mut self,
                level: usize,
            ) -> (&mut ww_stream::observe::ObserveTable, Option<ww_stream::batch::LevelBatch<'_>>) {
                match level {
                    #(#level)*
                }
            }

            /// The oldest batch of samples in the outbox of trait level `level` (API-39), None without one.
            pub fn oldest_batch(&self, level: usize) -> Option<ww_stream::batch::LevelBatch<'_>> {
                #[allow(clippy::match_single_binding)]
                match level {
                    #(#oldest)*
                    _ => None,
                }
            }

            /// Drop the oldest batch of samples of trait level `level` (API-39): the samples it held.
            pub fn remove_oldest_batch(&mut self, level: usize) -> Option<u64> {
                #[allow(clippy::match_single_binding)]
                match level {
                    #(#remove)*
                    _ => None,
                }
            }
        }
    }

    /// `ObserveBatches`, `ObserveBatchConsumers`, `ObserveBatchSource`, `ObserveSamplers`, a `<Level>Sampler` per
    /// level, and per trait with `+batched` properties a `<Trait>Samples` and a `<Trait>Row` (API-39): the rows of a
    /// level share one clock, one batcher per level.
    fn batch_items(
        &self,
        api_bundle: &CodegenBundle,
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

        let mut row_items = TokenStream::new();
        let mut samplers = TokenStream::new();
        let mut sampler_fields = vec![];
        let mut sampler_inits = vec![];
        let mut split_inits = vec![];
        // per level: batcher, gate, cursor, row type, doc
        let mut names = vec![];
        let mut gates = vec![];
        let mut cursors = vec![];
        let mut rows = vec![];
        let mut docs = vec![];
        // per cells level (API-50): staged batch, reader, gate, source, row type, doc
        let mut cell_names = vec![];
        let mut cell_readers = vec![];
        let mut cell_gates = vec![];
        let mut cell_srcs = vec![];
        let mut cell_rows = vec![];
        let mut cell_docs = vec![];
        let mut cells_items = TokenStream::new();
        // trait types that need cells: a level of the trait is a cells level
        let cells_bases: Vec<&String> = self
            .tables()
            .zip(bases)
            .filter(|(l, _)| l.cells)
            .map(|(_, (base, _))| base)
            .collect();
        for (level, (base, first)) in self.tables().zip(bases) {
            if !level.has_batched() {
                continue;
            }
            let samples = Ident::new(&format!("{base}Samples"), Span::call_site());
            let samples_store = Ident::new(&format!("{base}SamplesStore"), Span::call_site());
            let row = Ident::new(&format!("{base}Row"), Span::call_site());
            let row_store = Ident::new(&format!("{base}RowStore"), Span::call_site());
            let props: Vec<&Prop> = level.props.iter().filter(|p| p.batched).collect();
            let idents: Vec<&Ident> = props.iter().map(|p| &p.ident).collect();
            let ptys: Vec<TokenStream> = props.iter().map(|p| item_ty(p)).collect();
            let columns = props.len();
            let column_ids: Vec<usize> = (0..columns).collect();
            if *first {
                let doc = format!(
                    " One sample of the `+batched` properties of `{}` (API-39), for `<Level>Sampler::sample`.",
                    level.trait_name
                );
                let row_doc = format!(
                    " A row of `{}`'s samples (API-39): the device time of one `sample()` and its values. A level's \
                     batches keep rows as a struct of arrays and send each column as a `Series` of its own, the \
                     times once for all properties.",
                    level.trait_name
                );
                row_items.extend(quote! {
                    #[doc = #doc]
                    #[derive(Debug, Clone)]
                    pub struct #samples {
                        #(pub #idents: #ptys,)*
                    }

                    #[doc(hidden)]
                    pub struct #samples_store<const N: usize> {
                        #(#idents: <#ptys as wire_weaver::shrink_wrap::series::SeriesStore>::Store<N>,)*
                    }

                    impl wire_weaver::shrink_wrap::series::SeriesStore for #samples {
                        type Store<const N: usize> = #samples_store<N>;

                        fn empty_store<const N: usize>() -> #samples_store<N> {
                            wire_weaver::shrink_wrap::series::empty_store_from_init::<Self, N>()
                        }

                        fn init_store<const N: usize>(
                            slot: &mut core::mem::MaybeUninit<#samples_store<N>>,
                        ) -> &mut #samples_store<N> {
                            let at = slot.as_mut_ptr();
                            // SAFETY: every field of the store written in place, each through its own pointer
                            unsafe {
                                #(wire_weaver::shrink_wrap::series::init_store_at::<#ptys, N>(core::ptr::addr_of_mut!((*at).#idents));)*
                                slot.assume_init_mut()
                            }
                        }

                        #[inline(always)]
                        fn set<const N: usize>(store: &mut #samples_store<N>, i: usize, item: &Self) {
                            #(<#ptys as wire_weaver::shrink_wrap::series::SeriesStore>::set::<N>(&mut store.#idents, i, &item.#idents);)*
                        }

                        fn get<const N: usize>(store: &#samples_store<N>, i: usize) -> Self {
                            #samples {
                                #(#idents: <#ptys as wire_weaver::shrink_wrap::series::SeriesStore>::get::<N>(&store.#idents, i),)*
                            }
                        }
                    }

                    #[doc = #row_doc]
                    #[derive(Debug, Clone)]
                    pub struct #row {
                        pub t: ww_stream::observe::Second,
                        pub values: #samples,
                    }

                    #[doc(hidden)]
                    pub struct #row_store<const N: usize> {
                        t: <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesStore>::Store<N>,
                        values: #samples_store<N>,
                    }

                    impl wire_weaver::shrink_wrap::series::SeriesStore for #row {
                        type Store<const N: usize> = #row_store<N>;

                        fn empty_store<const N: usize>() -> #row_store<N> {
                            wire_weaver::shrink_wrap::series::empty_store_from_init::<Self, N>()
                        }

                        fn init_store<const N: usize>(
                            slot: &mut core::mem::MaybeUninit<#row_store<N>>,
                        ) -> &mut #row_store<N> {
                            let at = slot.as_mut_ptr();
                            // SAFETY: both fields of the store written in place, each through its own pointer
                            unsafe {
                                wire_weaver::shrink_wrap::series::init_store_at::<ww_stream::observe::Second, N>(core::ptr::addr_of_mut!((*at).t));
                                wire_weaver::shrink_wrap::series::init_store_at::<#samples, N>(core::ptr::addr_of_mut!((*at).values));
                                slot.assume_init_mut()
                            }
                        }

                        #[inline(always)]
                        fn set<const N: usize>(store: &mut #row_store<N>, i: usize, item: &Self) {
                            <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesStore>::set::<N>(&mut store.t, i, &item.t);
                            <#samples as wire_weaver::shrink_wrap::series::SeriesStore>::set::<N>(&mut store.values, i, &item.values);
                        }

                        fn get<const N: usize>(store: &#row_store<N>, i: usize) -> Self {
                            #row {
                                t: <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesStore>::get::<N>(&store.t, i),
                                values: <#samples as wire_weaver::shrink_wrap::series::SeriesStore>::get::<N>(&store.values, i),
                            }
                        }
                    }

                    impl ww_stream::batch::LevelRow for #row {
                        const COLUMNS: usize = #columns;
                        const TIME_BITS: (usize, usize) = ww_stream::batch::column_bits::<ww_stream::observe::Second>();
                        const COLUMN_BITS: &'static [(usize, usize)] = &[
                            #(ww_stream::batch::column_bits::<#ptys>(),)*
                        ];

                        fn encode_column<P: wire_weaver::shrink_wrap::series::SeriesPolicy + Copy, const N: usize, const B: usize>(
                            store: &#row_store<N>,
                            rows: core::ops::Range<usize>,
                            column: Option<usize>,
                            policy: P,
                            scratch: &mut ww_stream::batch::Scratch<B>,
                            buf: Option<&mut [u8]>,
                        ) -> Result<usize, wire_weaver::shrink_wrap::Error> {
                            match column {
                                None => scratch.encode_column::<ww_stream::observe::Second, P, N>(&store.t, rows, policy, buf),
                                #(Some(#column_ids) => scratch.encode_column::<#ptys, P, N>(&store.values.#idents, rows, policy, buf),)*
                                Some(_) => Err(wire_weaver::shrink_wrap::Error::OutOfBoundsWriteU8),
                            }
                        }
                    }
                });
            }
            if *first && cells_bases.contains(&base) {
                let samples_cells = Ident::new(&format!("{base}SamplesCells"), Span::call_site());
                let row_cells = Ident::new(&format!("{base}RowCells"), Span::call_site());
                row_items.extend(quote! {
                    #[doc(hidden)]
                    pub struct #samples_cells<const R: usize> {
                        #(pub #idents: <#ptys as wire_weaver::shrink_wrap::series::SeriesCells>::Cells<R>,)*
                    }

                    // SAFETY: the fields' cells, all valid all zero
                    unsafe impl wire_weaver::shrink_wrap::series::SeriesCells for #samples {
                        type Cells<const R: usize> = #samples_cells<R>;

                        #[inline]
                        fn put<const R: usize>(cells: &#samples_cells<R>, i: usize, item: &Self) {
                            #(<#ptys as wire_weaver::shrink_wrap::series::SeriesCells>::put::<R>(&cells.#idents, i, &item.#idents);)*
                        }

                        fn copy_rows<const R: usize, const N: usize>(
                            cells: &#samples_cells<R>,
                            from: usize,
                            store: &mut #samples_store<N>,
                            to: usize,
                            n: usize,
                        ) {
                            #(<#ptys as wire_weaver::shrink_wrap::series::SeriesCells>::copy_rows::<R, N>(&cells.#idents, from, &mut store.#idents, to, n);)*
                        }
                    }

                    #[doc(hidden)]
                    pub struct #row_cells<const R: usize> {
                        pub t: <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesCells>::Cells<R>,
                        pub values: #samples_cells<R>,
                    }

                    // SAFETY: the fields' cells, all valid all zero
                    unsafe impl wire_weaver::shrink_wrap::series::SeriesCells for #row {
                        type Cells<const R: usize> = #row_cells<R>;

                        #[inline]
                        fn put<const R: usize>(cells: &#row_cells<R>, i: usize, item: &Self) {
                            <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesCells>::put::<R>(&cells.t, i, &item.t);
                            <#samples as wire_weaver::shrink_wrap::series::SeriesCells>::put::<R>(&cells.values, i, &item.values);
                        }

                        fn copy_rows<const R: usize, const N: usize>(
                            cells: &#row_cells<R>,
                            from: usize,
                            store: &mut #row_store<N>,
                            to: usize,
                            n: usize,
                        ) {
                            <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesCells>::copy_rows::<R, N>(&cells.t, from, &mut store.t, to, n);
                            <#samples as wire_weaver::shrink_wrap::series::SeriesCells>::copy_rows::<R, N>(&cells.values, from, &mut store.values, to, n);
                        }
                    }

                    impl ww_stream::batch::LevelRowCells for #row {
                        #[inline]
                        fn put_time<const R: usize>(cells: &#row_cells<R>, i: usize, t: &ww_stream::observe::Second) {
                            <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesCells>::put::<R>(&cells.t, i, t);
                        }

                        // the time (or not) and the observed columns only
                        fn copy_columns<const R: usize, const N: usize>(
                            cells: &#row_cells<R>,
                            from: usize,
                            store: &mut #row_store<N>,
                            to: usize,
                            n: usize,
                            time: bool,
                            columns: ww_stream::batch::Columns,
                        ) {
                            if time {
                                <ww_stream::observe::Second as wire_weaver::shrink_wrap::series::SeriesCells>::copy_rows::<R, N>(&cells.t, from, &mut store.t, to, n);
                            }
                            #(
                                if columns & (1 << #column_ids) != 0 {
                                    <#ptys as wire_weaver::shrink_wrap::series::SeriesCells>::copy_rows::<R, N>(&cells.values.#idents, from, &mut store.values.#idents, to, n);
                                }
                            )*
                        }
                    }
                });
            }
            let name = level.batches();
            let gate = gate_ident(&name);
            let cursor = cursor_ident(&name);
            let sampler = level.sampler();
            let field = &level.field;
            if level.cells {
                let cells_ty = level.pascal("Cells");
                let writer_ty = level.pascal("Writer");
                let path = level
                    .path
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join("/");
                let cells_doc = format!(
                    " The observe cells of `{}` at `/{path}` (API-50, `observe_cells`): a ring of `R` rows of its \
                     `+batched` properties as atomics, written in place by the one `{writer_ty}` (a control loop, an \
                     interrupt), read by `poll_properties` through `ObserveBatches`. Put it in a `static` (`new` is \
                     `const`) or share it (an `Arc` on `std`, `CellsReader::shared`). `R` rows wait between two \
                     `poll_properties` calls, more are lost (counted as `missed`).",
                    level.trait_name
                );
                let writer_doc = format!(
                    " Writes the samples of `{}` at `/{path}` into its cells: one setter per `+batched` property \
                     (relaxed stores into the row), then `commit(t)`. Every value of a row has to be set before \
                     `commit` (debug builds check). `Send`, not `Sync`; dropping it gives it back.",
                    level.trait_name
                );
                let setter_docs = props.iter().map(|p| format!(" `{}` of this row.", p.ident));
                cells_items.extend(quote! {
                    #[doc = #cells_doc]
                    pub struct #cells_ty<const R: usize>(ww_stream::batch::RowCells<#row, R>);

                    impl<const R: usize> #cells_ty<R> {
                        /// Empty, sampling nothing until a host observes the level in `Samples` mode.
                        pub const fn new() -> Self {
                            #cells_ty(ww_stream::batch::RowCells::new())
                        }

                        /// The writer, if no one holds it.
                        pub fn writer(&self) -> Option<#writer_ty<'_, R>> {
                            self.0.writer().map(|w| #writer_ty { w })
                        }

                        /// The reader for `ObserveBatches::new`, if no one holds it.
                        pub fn reader(&self) -> Option<ww_stream::batch::CellsReader<'_, #row, OBSERVE_BATCH>> {
                            self.0.reader()
                        }

                        /// Rows committed so far (wrapping).
                        pub fn committed(&self) -> u32 {
                            self.0.committed()
                        }
                    }

                    impl<const R: usize> Default for #cells_ty<R> {
                        fn default() -> Self {
                            Self::new()
                        }
                    }

                    impl<const R: usize> AsRef<ww_stream::batch::RowCells<#row, R>> for #cells_ty<R> {
                        fn as_ref(&self) -> &ww_stream::batch::RowCells<#row, R> {
                            &self.0
                        }
                    }

                    #[doc = #writer_doc]
                    pub struct #writer_ty<'a, const R: usize> {
                        w: ww_stream::batch::RowWriter<'a, #row, R>,
                    }

                    impl<'a, const R: usize> #writer_ty<'a, R> {
                        /// Wake the server task (SRV-25, `ObserveSignal::as_wake()`) at the rows its next
                        /// `poll_properties` waits for: a batch's first row, or the one that fills it. Without, the
                        /// server has to poll the level every `max_age`.
                        pub fn with_wake(self, wake: &'a ww_stream::wake::Wake) -> Self {
                            #writer_ty { w: self.w.with_wake(wake) }
                        }

                        #(
                            #[doc = #setter_docs]
                            #[inline(always)]
                            pub fn #idents(&mut self, value: #ptys) {
                                <#ptys as wire_weaver::shrink_wrap::series::SeriesCells>::put::<R>(&self.w.rows().values.#idents, self.w.slot(), &value);
                                self.w.mark(#column_ids);
                            }
                        )*

                        /// Publishes the row with its device time `t` when the level's gate takes it (a host
                        /// observes the level in `Samples` mode and `t` is on its grid). Returns whether it did.
                        #[inline]
                        pub fn commit(&mut self, t: ww_stream::observe::Second) -> bool {
                            self.w.commit(t)
                        }

                        /// Whether a host observes the level in `Samples` mode: skip working out the values
                        /// otherwise.
                        #[inline]
                        pub fn is_sampling(&self) -> bool {
                            self.w.is_sampling()
                        }
                    }
                });
                cell_docs.push(format!(
                    " Samples of `{}` (observe cells)",
                    level.trait_name
                ));
                cell_names.push(name);
                cell_readers.push(level.reader());
                cell_gates.push(gate);
                cell_srcs.push(level.src());
                cell_rows.push(row);
                continue;
            }
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
                    rows: ww_stream::batch::Producer<'a, #row, OBSERVE_BATCH>,
                    gate: &'a ww_stream::observe::SampleGate,
                    cursor: &'a mut ww_stream::observe::GateCursor,
                    wake: Option<&'a ww_stream::wake::Wake>,
                }

                impl<'a> #sampler<'a> {
                    /// One sample of the level, taken at device time `t`: when a host observes any of its
                    /// properties in `Samples` mode (and the sample is on the level's grid), the time and the
                    /// values go into the level's batch as one row (a copy and a few atomic operations, never an
                    /// encode or a wait: fine in a control loop or an interrupt), else they are dropped.
                    /// `poll_properties` sends the batches. Returns the rows lost because the event loop didn't
                    /// keep up.
                    pub fn sample(&mut self, t: ww_stream::observe::Second, values: #samples) -> usize {
                        if !self.cursor.take(self.gate, &t) {
                            return 0;
                        }
                        let seal = ww_stream::batch::Push::push(&mut self.rows, &#row { t, values });
                        // a batch's first row (its age starts when a poll sees it) or a sealed one: the server
                        // task has something to do
                        if let Some(wake) = self.wake
                            && (!matches!(seal, ww_stream::batch::Seal::No) || self.rows.filling() == 1)
                        {
                            wake.wake();
                        }
                        match seal {
                            ww_stream::batch::Seal::Lost { items, .. } => items,
                            _ => 0,
                        }
                    }

                    /// Wake the server task (SRV-25, `ObserveSignal::as_wake()`) when a batch gets its first row
                    /// or is sealed. Without, the server has to poll the level every `max_age`.
                    pub fn with_wake(mut self, wake: &'a ww_stream::wake::Wake) -> Self {
                        self.wake = Some(wake);
                        self
                    }

                    /// Whether a host observes any property of the level in `Samples` mode: skip working out
                    /// the values otherwise.
                    pub fn is_sampling(&self) -> bool {
                        self.gate.is_open()
                    }
                }
            });
            sampler_fields.push(quote! { pub #field: #sampler<'a>, });
            sampler_inits.push(quote! {
                #field: #sampler {
                    rows: ww_stream::batch::Batcher::producer(&mut self.#name),
                    gate: &self.#gate,
                    cursor: &mut self.#cursor,
                    wake: None,
                },
            });
            split_inits.push(quote! {
                #field: #sampler {
                    rows: #name.0,
                    gate: &self.#gate,
                    cursor: &mut self.#cursor,
                    wake: None,
                },
            });
            docs.push(format!(" Samples of `{}`", level.trait_name));
            names.push(name);
            gates.push(gate);
            cursors.push(cursor);
            rows.push(row);
        }
        let has_cells = !cell_names.is_empty();
        // `ObserveBatches<'c>` borrows the cells it reads when a level is a cells level
        let (lt_def, lt_use, lt_any) = if has_cells {
            (quote! { <'c> }, quote! { <'c> }, quote! { <'_> })
        } else {
            (quote! {}, quote! {}, quote! {})
        };
        let samplers_where = if has_cells {
            quote! { pub fn samplers<'s, 'c: 's>(&'s mut self) -> ObserveSamplers<'s> where B: core::borrow::BorrowMut<ObserveBatches<'c>> }
        } else {
            quote! { pub fn samplers(&mut self) -> ObserveSamplers<'_> where B: core::borrow::BorrowMut<ObserveBatches> }
        };
        let cells_doc = maybe_quote(
            has_cells,
            quote! {
                /// Levels on observe cells (`observe_cells`) keep one staging batch here and read their rows from
                /// the `<Level>Cells` whose reader `new` takes.
            },
        );
        quote! {
            #errors
            /// Samples per batch of each level with `+batched` properties (`observe_batch = N` of `ww_codegen!`).
            pub const OBSERVE_BATCH: usize = #batch;
            /// Bytes of the outbox of each level with `+batched` properties (`observe_outbox = N`).
            pub const OBSERVE_OUTBOX: usize = #outbox;

            #row_items
            #cells_items

            /// The batches of every level with `+batched` properties (API-39): two stores of `OBSERVE_BATCH` rows
            /// each (a time and every value), filled by the samplers, encoded by `poll_properties`.
            /// [split](Self::split) it to sample in another context (an interrupt, another core), or put it into
            /// `ObserveState` as it is and sample through [samplers](Self::samplers).
            #cells_doc
            pub struct ObserveBatches #lt_def {
                #(
                    #[doc = #docs]
                    #names: ww_stream::batch::Batcher<#rows, OBSERVE_BATCH>,
                    #gates: ww_stream::observe::SampleGate,
                    #cursors: ww_stream::observe::GateCursor,
                )*
                #(
                    #[doc = #cell_docs]
                    #cell_names: ww_stream::batch::Staged<#cell_rows, OBSERVE_BATCH>,
                    #cell_readers: ww_stream::batch::CellsReader<'c, #cell_rows, OBSERVE_BATCH>,
                )*
            }

            impl #lt_def ObserveBatches #lt_use {
                /// Batches encoded with `policy`: `ww_stream::batch::DEFAULT_POLICY` (every codec on `std`, delta
                /// and packed on `no_std`, so a firmware links only those), or another value of its type; a cells
                /// level reads its rows through the reader of its `<Level>Cells`.
                pub fn new(
                    policy: ww_stream::batch::DefaultPolicy,
                    #(#cell_readers: ww_stream::batch::CellsReader<'c, #cell_rows, OBSERVE_BATCH>,)*
                ) -> Self {
                    let mut slot = core::mem::MaybeUninit::uninit();
                    Self::init(&mut slot, policy, #(#cell_readers,)*);
                    // SAFETY: `init` initialized it
                    unsafe { slot.assume_init() }
                }

                /// [new](Self::new) in place: builds the batches in `slot` and returns them, the stores written
                /// where they stay, never on the stack (they take a few KB per level). For a `static`:
                /// `ObserveBatches::init(CELL.uninit(), DEFAULT_POLICY)` with a `static_cell::StaticCell`, or a
                /// `static mut` `MaybeUninit`.
                pub fn init<'s>(
                    slot: &'s mut core::mem::MaybeUninit<Self>,
                    policy: ww_stream::batch::DefaultPolicy,
                    #(#cell_readers: ww_stream::batch::CellsReader<'c, #cell_rows, OBSERVE_BATCH>,)*
                ) -> &'s mut Self {
                    // every field is written below: a new one fails to compile here until it is
                    let _ = |b: Self| {
                        let ObserveBatches { #(#names: _, #gates: _, #cursors: _,)* #(#cell_names: _, #cell_readers: _,)* } = b;
                    };
                    let at = slot.as_mut_ptr();
                    // SAFETY: every field written in place, each through its own pointer
                    unsafe {
                        #(
                            ww_stream::batch::Batcher::init_rows(
                                &mut *core::ptr::addr_of_mut!((*at).#names)
                                    .cast::<core::mem::MaybeUninit<ww_stream::batch::Batcher<#rows, OBSERVE_BATCH>>>(),
                                policy,
                            );
                            core::ptr::addr_of_mut!((*at).#gates).write(ww_stream::observe::SampleGate::new());
                            core::ptr::addr_of_mut!((*at).#cursors).write(ww_stream::observe::GateCursor::new());
                        )*
                        #(
                            ww_stream::batch::Staged::init(
                                &mut *core::ptr::addr_of_mut!((*at).#cell_names)
                                    .cast::<core::mem::MaybeUninit<ww_stream::batch::Staged<#cell_rows, OBSERVE_BATCH>>>(),
                                policy,
                            );
                            core::ptr::addr_of_mut!((*at).#cell_readers).write(#cell_readers);
                        )*
                        slot.assume_init_mut()
                    }
                }

                /// The samplers of each level and the consumer half for `ObserveState`, for samplers that run in
                /// another context: split a batches that outlives both (a `static`, a `StaticCell`).
                pub fn split(&mut self) -> (ObserveSamplers<'_>, ObserveBatchConsumers<'_>) {
                    #(let #names = ww_stream::batch::Batcher::split(&mut self.#names);)*
                    (
                        ObserveSamplers { #(#split_inits)* _p: core::marker::PhantomData },
                        ObserveBatchConsumers {
                            #(
                                #names: #names.1,
                                #gates: &self.#gates,
                            )*
                            #(
                                #cell_names: &mut self.#cell_names,
                                #cell_gates: self.#cell_readers.gate(),
                                #cell_srcs: self.#cell_readers.source(),
                            )*
                            _p: core::marker::PhantomData,
                        },
                    )
                }

                /// The samplers, for a device that samples from the task that polls.
                pub fn samplers(&mut self) -> ObserveSamplers<'_> {
                    ObserveSamplers { #(#sampler_inits)* _p: core::marker::PhantomData }
                }
            }

            /// The sampler of each level with `+batched` properties, see `ObserveBatches::split`.
            pub struct ObserveSamplers<'a> {
                #(#sampler_fields)*
                _p: core::marker::PhantomData<&'a ()>,
            }

            #samplers

            /// The consumer half of `ObserveBatches`, what `poll_properties` encodes from.
            pub struct ObserveBatchConsumers<'a> {
                #(
                    #names: ww_stream::batch::Consumer<'a, #rows, OBSERVE_BATCH>,
                    #gates: &'a ww_stream::observe::SampleGate,
                )*
                #(
                    #cell_names: &'a mut ww_stream::batch::Staged<#cell_rows, OBSERVE_BATCH>,
                    #cell_gates: &'a ww_stream::observe::SampleGate,
                    #cell_srcs: &'a (dyn ww_stream::batch::CellsSource<#cell_rows, OBSERVE_BATCH> + Sync),
                )*
                _p: core::marker::PhantomData<&'a ()>,
            }

            /// Where `ObserveState` takes the consumer half of the batches from: `ObserveBatches` itself, or the
            /// `ObserveBatchConsumers` split off it.
            pub trait ObserveBatchSource {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_>;
            }

            impl ObserveBatchSource for ObserveBatches #lt_any {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_> {
                    ObserveBatchConsumers {
                        #(
                            #names: ww_stream::batch::Batcher::consumer(&mut self.#names),
                            #gates: &self.#gates,
                        )*
                        #(
                            #cell_names: &mut self.#cell_names,
                            #cell_gates: self.#cell_readers.gate(),
                            #cell_srcs: self.#cell_readers.source(),
                        )*
                        _p: core::marker::PhantomData,
                    }
                }
            }

            /// Batches kept elsewhere (a `static` built with `ObserveBatches::init`), lent to `ObserveState`.
            impl<T: ObserveBatchSource + ?Sized> ObserveBatchSource for &mut T {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_> {
                    (**self).consumers()
                }
            }

            impl ObserveBatchSource for ObserveBatchConsumers<'_> {
                fn consumers(&mut self) -> ObserveBatchConsumers<'_> {
                    ObserveBatchConsumers {
                        #(
                            #names: ww_stream::batch::BatchSource::consumer(&mut self.#names),
                            #gates: self.#gates,
                        )*
                        #(
                            #cell_names: &mut *self.#cell_names,
                            #cell_gates: self.#cell_gates,
                            #cell_srcs: self.#cell_srcs,
                        )*
                        _p: core::marker::PhantomData,
                    }
                }
            }

            impl<B: ObserveBatchSource> ObserveState<B> {
                /// The samplers of the batches the state owns (API-39).
                #samplers_where
                {
                    self.batches.borrow_mut().samplers()
                }
            }
        }
    }

    /// `observe_request`, `mark_changed_<name>()` and `poll_properties`, in the server struct's impl.
    pub(crate) fn methods(
        &self,
        api_bundle: &CodegenBundle,
        handlers: Handlers<'_>,
        medium_ty: &TokenStream,
        no_alloc: bool,
    ) -> TokenStream {
        if !self.is_enabled() {
            return quote! {};
        }
        let observe_request = self.observe_request();
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
        let poll = self.poll_properties(api_bundle, handlers, medium_ty, no_alloc);
        quote! {
            #observe_request
            #(#mark_changed)*
            #poll
        }
    }

    fn observe_request(&self) -> TokenStream {
        let (es_malformed, es_refused, es_ser, es_array, es_path) = (
            fixed_err_seq("OBSERVE_MALFORMED"),
            fixed_err_seq("OBSERVE_REFUSED"),
            fixed_err_seq("OBSERVE_SER"),
            fixed_err_seq("OBSERVE_ARRAY"),
            fixed_err_seq("OBSERVE_PATH"),
        );
        // levels without observable properties: every entry refused, by one shared path
        let empty = self
            .levels
            .iter()
            .filter(|l| l.props.is_empty())
            .map(|level| {
                let path = &level.path;
                let item_count = level.item_count;
                quote! { [#(UNib32(#path)),*] => Some(#item_count), }
            });
        let arms = self.levels.iter().filter(|l| !l.props.is_empty()).map(|level| {
            let path = &level.path;
            let n = level.props.len();
            let item_count = level.item_count;
            let indices = level.props.iter().map(|p| p.index);
            let batched = level.props.iter().map(|p| p.batched);
            let desc = quote! {
                const DESC: ww_stream::observe::LevelDesc<'static> = ww_stream::observe::LevelDesc {
                    item_count: #item_count,
                    observable: &[#(#indices),*],
                    batched: &[#(#batched),*],
                };
            };
            let field = &level.field;
            // the level's sampler takes rows while any property is in `Samples` mode, on the level's grid; columns of
            // the others leave the waiting batches (API-39)
            let sync_gates = maybe_quote_cl(level.has_batched(), || {
                let outbox = level.outbox();
                let gate = gate_ident(&level.batches());
                quote! {
                    let state = self.observe_state();
                    let c = state.batches.consumers();
                    c.#gate.set(state.#field.level_samples().map(|(every, _)| every));
                    let table = &state.#field;
                    state.#outbox.retain_columns(|k| table.samples(k as usize).is_some());
                }
            });
            let table = quote! {
                let state = self.observe_state();
                let limits = state.limits;
                let others = state.budget_in_use().saturating_sub(state.#field.budget());
                let table = &mut state.#field;
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
                let empty_level: Option<u32> = match path {
                    #(#empty)*
                    _ => None,
                };
                if let Some(item_count) = empty_level {
                    let refused_len = match ww_stream::observe::refuse_all(cfg, item_count, &mut refused) {
                        Ok(len) => len,
                        Err(ww_stream::observe::ObserveError::Malformed(_)) => {
                            return Err(Error::new(#es_malformed, ErrorKind::ArgsDesFailed));
                        }
                        Err(ww_stream::observe::ObserveError::TooManyRefused) => {
                            return Err(Error::new(#es_refused, ErrorKind::BadIndex));
                        }
                    };
                    let granted = ObserveGranted {
                        entries: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] },
                        refused: wire_weaver::shrink_wrap::RefVec::Slice { slice: &refused[..refused_len] },
                        budget: UNib32(0),
                    };
                    if granted.ser_shrink_wrap(wr).is_err() {
                        return Err(Error::new(#es_ser, ErrorKind::ResponseSerFailed));
                    }
                    event_kind_builder.finish_with_kind(EventKindDiscriminants::Value, wr);
                    return Ok(());
                }
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
        api_bundle: &CodegenBundle,
        handlers: Handlers<'_>,
        medium_ty: &TokenStream,
        no_alloc: bool,
    ) -> TokenStream {
        let use_async = handlers.use_async;
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
        let levels: Vec<&Level> = self.tables().collect();
        let level_count = levels.len();
        let max_n = levels.iter().map(|l| l.props.len()).max().unwrap_or(0);
        let descs = levels.iter().map(|level| {
            let item_count = level.item_count;
            let indices = level.props.iter().map(|p| p.index);
            let batched = level.props.iter().map(|p| p.batched);
            quote! {
                ww_stream::observe::LevelDesc {
                    item_count: #item_count,
                    observable: &[#(#indices),*],
                    batched: &[#(#batched),*],
                }
            }
        });
        let paths = levels.iter().map(|level| {
            let path = &level.path;
            quote! { &[#(UNib32(#path)),*] }
        });
        // a value read through an async getter is read before the update is put together, into `value_<i>_<k>`
        // (None: not read yet, Some(None): the getter failed), so that the shared, sync part of the loop
        // (`ww_stream::observe::select` / `write`) gets it from there; sync getters and fields are read in place
        let is_async =
            |p: &Prop| matches!(p.model, PropertyModelKind::GetSet) && handlers.is_async(&p.dotted);
        let cache_ident =
            |i: usize, k: usize| Ident::new(&format!("value_{i}_{k}"), Span::call_site());
        let max_len_of = |p: &Prop, ty: &TokenStream, owned: bool| {
            if owned {
                // no bound for a value that can grow
                quote! { ww_stream::observe::max_write_len::<wire_weaver::shrink_wrap::OwnedSer<'static, #ty>>() }
            } else if p.batched {
                quote! { ww_stream::observe::max_batched_one_len::<#ty>() }
            } else {
                quote! { ww_stream::observe::max_write_len::<#ty>() }
            }
        };
        let mut caches = vec![];
        let mut read_unbounded = vec![];
        let mut read_chosen = vec![];
        let mut measure = vec![];
        let mut write = vec![];
        let mut encode = vec![];
        for (i, level) in levels.iter().enumerate() {
            let field = &level.field;
            let outbox = level.outbox();
            let mut unbounded_i = vec![];
            let mut chosen_i = vec![];
            for (k, p) in level.props.iter().enumerate() {
                // the handlers of a `no_alloc = false` server give owned values (API-21): written through `OwnedSer`
                // when they aren't the borrowed type (a batched property's type never borrows)
                let owned = !no_alloc && p.ty.is_lifetime(api_bundle).unwrap();
                let ty = ty_def(api_bundle, &p.ty, !no_alloc, TyPos::Annotation).unwrap();
                let value = if owned {
                    quote! { &wire_weaver::shrink_wrap::OwnedSer(value) }
                } else {
                    quote! { value }
                };
                let index = p.index;
                let max_len = max_len_of(p, &ty, owned);
                let get = Ident::new(&format!("get_{}", p.prefixed), Span::call_site());
                let get_call = handlers.call(&p.dotted, quote! { self.#get(&mut cx) });
                let v = cache_ident(i, k);
                if is_async(p) {
                    // a `+batched` property in `Samples` mode is sent as a column of its level's batch, never read
                    let not_column = maybe_quote(
                        p.batched,
                        quote! { && self.observe_state().#field.samples(#k).is_none() },
                    );
                    let read = quote! {
                        #v = Some(match #get_call {
                            GetResult::Value(value) => Some(value),
                            _ => None,
                        });
                    };
                    caches.push(quote! { let mut #v: Option<Option<#ty>> = None; });
                    unbounded_i.push(quote! {
                        if #max_len.is_none() && plan.is_due(#k) #not_column {
                            #read
                        }
                    });
                    chosen_i.push(quote! {
                        if #v.is_none() && round.needs_value(#k) {
                            #read
                        }
                    });
                }
                // upper bound of the bytes slot `k` takes, `Batched::One` of a `+batched` property (its tag in
                // front)
                let measured = if p.batched {
                    quote! { ww_stream::observe::measure(&ww_client_server::observe::BatchedOne(value), &mut *buf) }
                } else {
                    quote! { ww_stream::observe::measure(#value, &mut *buf) }
                };
                // slot `k`'s value into the update
                let put = if p.batched {
                    quote! { wr.write(#index, &ww_client_server::observe::BatchedOne(value)).is_ok() }
                } else {
                    quote! { wr.write(#index, #value).is_ok() }
                };
                let (size_one, write_one) = match p.model {
                    PropertyModelKind::GetSet if is_async(p) => (
                        quote! {
                            match #max_len {
                                Some(len) => Some(len),
                                None => match &#v {
                                    Some(Some(value)) => #measured,
                                    _ => None,
                                },
                            }
                        },
                        quote! {
                            match &#v {
                                Some(Some(value)) => #put,
                                _ => false,
                            }
                        },
                    ),
                    PropertyModelKind::GetSet => (
                        quote! {
                            match #max_len {
                                Some(len) => Some(len),
                                None => match #get_call {
                                    GetResult::Value(value) => {
                                        let value: #ty = value;
                                        let value = &value;
                                        #measured
                                    }
                                    _ => None,
                                },
                            }
                        },
                        quote! {
                            match #get_call {
                                GetResult::Value(value) => {
                                    let value: #ty = value;
                                    let value = &value;
                                    #put
                                }
                                _ => false,
                            }
                        },
                    ),
                    PropertyModelKind::ValueOnChanged => {
                        let f = &p.prefixed;
                        (
                            quote! {{
                                let value: &#ty = &self.#f;
                                #measured
                            }},
                            quote! {{
                                let value: &#ty = &self.#f;
                                #put
                            }},
                        )
                    }
                };
                measure.push(quote! { (#i, #k) => #size_one, });
                // in `Samples` mode its column of the level's oldest batch
                if p.batched {
                    write.push(quote! {
                        (#i, #k) if column => {
                            let state = self.observe_state();
                            match state.#outbox.oldest().and_then(|b| b.column_after(#k as u16, &mut column_at)) {
                                Some(column) => wr.write(#index, &ww_client_server::observe::BatchedRaw(column.bytes)).is_ok(),
                                None => false,
                            }
                        }
                    });
                }
                write.push(quote! { (#i, #k) => #write_one, });
            }
            if !unbounded_i.is_empty() {
                read_unbounded.push(quote! { #i => { #(#unbounded_i)* } });
                read_chosen.push(quote! { #i => { #(#chosen_i)* } });
            }
            if level.has_batched() {
                let body = self.encode_batches(level);
                encode.push(quote! { #i => { #body } });
            }
        }
        let has_batched = self.has_batched();
        let encode = maybe_quote_cl(!encode.is_empty(), || {
            quote! {
                // the level's sealed rows into its outbox, encoded once
                match level {
                    #(#encode)*
                    _ => {}
                }
            }
        });
        let read_unbounded = maybe_quote_cl(!read_unbounded.is_empty(), || {
            quote! {
                // async getters of values without a size bound, read to be measured
                match level {
                    #(#read_unbounded)*
                    _ => {}
                }
            }
        });
        let read_chosen = maybe_quote_cl(!read_chosen.is_empty(), || {
            quote! {
                // async getters of the values chosen
                match level {
                    #(#read_chosen)*
                    _ => {}
                }
            }
        });
        // the level's oldest batch of samples (API-39), offered whole by a slot in `Samples` mode
        let batch_offer = if has_batched {
            quote! {
                let mut batch_ks = [0usize; #max_n];
                let offer = self
                    .observe_state()
                    .oldest_batch(level)
                    .map(|batch| ww_stream::observe::BatchOffer::new(&batch, &mut batch_ks));
            }
        } else {
            quote! { let offer = None; }
        };
        // the time column of a batch goes into the event's head, `sample_times`
        let sample_times = if has_batched {
            quote! {
                let sample_times = if update.has_sample_times() {
                    self.observe_state().oldest_batch(level).map(|batch| batch.time)
                } else {
                    None
                };
            }
        } else {
            quote! { let sample_times = None; }
        };
        // a sent batch leaves the outbox, one that never fits a message goes as missed; while more wait (and the
        // budget lasts), another update
        let next = if has_batched {
            quote! {
                let state = self.observe_state();
                let more = if sent.is_some() && round.has_batch() {
                    state.remove_oldest_batch(level);
                    true
                } else if round.batch_dropped() {
                    let samples = state.remove_oldest_batch(level).unwrap_or(0);
                    state.level(level).0.add_missed(samples);
                    true
                } else {
                    false
                };
                rounds += 1;
                if !more || rounds >= ww_stream::observe::MAX_UPDATES_PER_POLL {
                    break;
                }
            }
        } else {
            quote! { break; }
        };
        let rounds = maybe_quote(has_batched, quote! { let mut rounds = 0usize; });
        // the batch's columns are written in slot order: each lookup goes on from the last one
        let column_cursor = maybe_quote(has_batched, quote! { let mut column_at = 0usize; });
        let (scratch_generic, scratch_arg, scratch_doc) = if has_batched {
            let doc = " Batches of `+batched` properties observed in `Samples` mode (API-39) are encoded into their \
                level's outbox first (`scratch` is where one column of a batch is rebuilt at a time: \
                `ww_stream::batch::scratch_bytes::<T>(OBSERVE_BATCH)` of the biggest of `ww_stream::observe::Second` \
                and the `+batched` property types), then sent while they wait and the budget lasts, more than one \
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
            /// any microsecond clock. Call it often (every millisecond or so), before `poll_streams`. Async getters
            /// of values without a size bound are read once to measure and send them, sync ones twice.
            #scratch_doc
            pub #maybe_async fn poll_properties #scratch_generic(
                &mut self,
                out: &mut impl #out_trait,
                medium: #medium_ty,
                now_us: u64,
                buf: &mut [u8]
                #scratch_arg
            ) -> Result<(), wire_weaver::SendError> {
                const DESCS: [ww_stream::observe::LevelDesc<'static>; #level_count] = [#(#descs),*];
                const PATHS: [&[UNib32]; #level_count] = [#(#paths),*];
                #[allow(unused_mut)]
                let mut cx = wire_weaver::Context::new(0, medium, out);
                // changes marked outside the server state since the last poll (SRV-25)
                {
                    let state = self.observe_state();
                    if let Some(signal) = state.signal() {
                        state.take_changes(signal);
                    }
                }
                let mut first_err = None;
                let limits = self.observe_state().limits;
                // the plan and the selection of each update, a slot each
                let mut due = [ww_stream::observe::Due::EMPTY; #max_n];
                let mut marks = [ww_stream::observe::Mark::NONE; #max_n];
                // the algorithm (plan, selection, writing, commit) is `ww_stream::observe`'s, and this loop is the
                // same for every level (SRV-24): a level gives the sizes and values of its slots and reads its async
                // getters, the update is sent from one place
                for level in 0..#level_count {
                    let desc = &DESCS[level];
                    let path = PATHS[level];
                    #encode
                    #rounds
                    loop {
                        let plan = {
                            let (table, oldest) = self.observe_state().level(level);
                            ww_stream::observe::plan_level(table, oldest.as_ref(), now_us, &limits, &mut due)
                        };
                        #(#caches)*
                        #read_unbounded
                        #batch_offer
                        let mut round = match ww_stream::observe::select(
                            &plan,
                            desc,
                            path,
                            &mut *buf,
                            &mut marks[..desc.observable.len()],
                            offer.as_ref(),
                            &mut |k, buf: &mut [u8]| match (level, k) {
                                #(#measure)*
                                _ => None,
                            },
                        )? {
                            ww_stream::observe::Selected::Round(round) => round,
                            ww_stream::observe::Selected::Nothing => break,
                            // a missed count waiting alone for budget: `next_wake` waits for its bytes (SRV-25)
                            ww_stream::observe::Selected::Hold(need) => {
                                self.observe_state().level(level).0.hold(need);
                                break;
                            }
                        };
                        #read_chosen
                        #column_cursor
                        let update = ww_stream::observe::write(
                            &mut round,
                            &plan,
                            desc,
                            &mut *buf,
                            &mut |k, column, wr| match (level, k) {
                                #(#write)*
                                _ => false,
                            },
                        );
                        let mut sent = None;
                        if let Some(update) = update {
                            let data = &buf[..update.data_len()];
                            #sample_times
                            let mut event_len = 0;
                            let result = #send_with(&mut cx, |scratch| {
                                event_len = ww_stream::observe::ser_update(scratch, path, &update, data, sample_times)?;
                                Ok(event_len)
                            })#maybe_await;
                            match result {
                                Ok(()) => sent = Some(event_len),
                                Err(e) => {
                                    first_err.get_or_insert(e);
                                }
                            }
                        }
                        self.observe_state().level(level).0.commit(&plan, round.selection(), now_us, sent);
                        #next
                    }
                }
                match first_err {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            }
        }
    }

    /// The sealed rows of a level with `+batched` properties into its outbox (API-39), encoded once: the time
    /// column and the columns of the properties in `Samples` mode; without any, the rows go. In
    /// `poll_properties`, with `desc` and `path` of the level.
    fn encode_batches(&self, level: &Level) -> TokenStream {
        let field = &level.field;
        let outbox = level.outbox();
        let batched_ks = level.batched_ks();
        let columns = batched_ks.len();
        let column_ids: Vec<usize> = (0..columns).collect();
        let batches = level.batches();
        let src = level.src();
        // the sealed rows of a cells level come from its cells, a batch at a time while they make progress
        let tail = if level.cells {
            quote! {
                let mut missed = 0u64;
                loop {
                    let polled = c.#batches.poll_cells(c.#src, now_us, Some(max_age as u64), budget, columns);
                    // lapped rows miss a sample of every observed property
                    missed += polled.lost * observed;
                    match state.#outbox.push_rows_from(&mut *c.#batches, columns, &SLOTS, &mut *scratch) {
                        Ok(pushed) => {
                            missed += pushed.dropped + pushed.lost * observed;
                            if pushed.batches == 0 || c.#src.waiting() == 0 {
                                break;
                            }
                        }
                        // a value that doesn't serialize: its batch goes
                        Err(_) => {
                            let (rows, lost) = c.#batches.discard_sealed();
                            missed += (rows as u64 + lost) * observed;
                            break;
                        }
                    }
                }
                state.#field.add_missed(missed);
            }
        } else {
            quote! {
                let polled = c.#batches.poll_rows(now_us, Some(max_age as u64), budget, columns);
                // lost rows miss a sample of every observed property
                let missed = match state.#outbox.push_rows_from(&mut c.#batches, columns, &SLOTS, &mut *scratch) {
                    Ok(pushed) => pushed.dropped + pushed.lost * observed,
                    // a value that doesn't serialize: its batch goes
                    Err(_) => {
                        let (rows, lost) = c.#batches.discard_sealed();
                        (rows as u64 + lost) * observed
                    }
                };
                state.#field.add_missed(polled.lost * observed + missed);
            }
        };
        // nobody observes the level in `Samples` mode: its rows go
        let drop_rows = if level.cells {
            quote! { c.#batches.discard_all(c.#src); }
        } else {
            quote! {
                c.#batches.flush();
                c.#batches.discard_sealed();
            }
        };
        quote! {
            let overhead = ww_stream::observe::batch_overhead(&mut *buf, path)?;
            let state = self.observe_state();
            let mut c = state.batches.consumers();
            match state.#field.level_samples() {
                Some((_, max_age)) => {
                    // column `c` of the rows is the property in table slot `SLOTS[c]`
                    const SLOTS: [u16; #columns] = [#(#batched_ks as u16),*];
                    let mut columns: ww_stream::batch::Columns = 0;
                    #(
                        if state.#field.samples(#batched_ks).is_some() {
                            columns |= 1 << #column_ids;
                        }
                    )*
                    let observed = u64::from(columns.count_ones());
                    let budget = ww_stream::observe::batch_budget(
                        desc,
                        overhead,
                        buf.len(),
                        OBSERVE_OUTBOX,
                        observed as usize,
                    );
                    #tail
                }
                None => {
                    #drop_rows
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
    api_bundle: &CodegenBundle,
    level: &Level,
    name: &str,
    no_alloc: bool,
) -> TokenStream {
    // a `Batched<'i, T>` slot borrows its series
    let is_lifetime = level
        .props
        .iter()
        .any(|p| p.batched || p.ty.is_lifetime(api_bundle).unwrap());
    let needs_lifetime = no_alloc && is_lifetime;
    // with owned fields (a `no_alloc = false` server, API-21), the name shrink_wrap wants for an owned type
    let name = if !no_alloc && is_lifetime {
        format_ident!("{name}Owned")
    } else {
        Ident::new(name, Span::call_site())
    };
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
