//! # Implementation details:
//! * Server's index chain contains only array indices on the way to a resource
use crate::codegen::index_chain::IndexChain;
use crate::codegen::omit::{Omission, Omissions, Omit};
use crate::codegen::server::observe::{Observe, mark_changed_ident};
use crate::codegen::server::stream::stream_ser_methods_recursive;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{
    Handlers, add_prefix, dotted, fixed_err_seq, item_err_seq, maybe_quote, maybe_quote_cl,
};
use crate::codegen::{api_common, util};
use crate::const_model::{ConstModel, ConstModelKind};
use crate::handler_model::HandlerModel;
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::{BTreeMap, BTreeSet};
use syn::{Lit, LitInt, Path};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Delivery,
    FlowControl, Multiplicity, PropertyAccess, StreamFnRef, TypeOwned,
};
use ww_version::FullVersionOwned;

/// API server code generation configuration.
pub struct GenServerConfig {
    /// Set to false to generate no_std no-alloc compatible code.
    pub no_alloc: bool,
    /// Set to true for an async server: handlers are async fn's, except those `handler_model` makes sync.
    pub use_async: bool,
    /// Which user handlers are `async fn` and which plain `fn` (SRV-21); empty: all follow `use_async`.
    pub handler_model: HandlerModel,
    /// Configure which user handlers return immediate response or answer later by using request id.
    pub method_model: MethodModel,
    /// Configure which properties are using get-set model and which are direct struct field access + on_changed fn model.
    pub property_model: PropertyModel,
    /// Configure which `const` items are Rust consts and which are `fn`s of the generated `<Trait>Consts` trait.
    pub const_model: ConstModel,
    /// Path to the user struct on which server dispatcher method will be implemented (process_request_bytes).
    /// Dispatcher expects all the user handlers also to be implemented on this struct by user code.
    pub server_struct_path: String,
    /// Generate ww_self introspect bytes, fully describing all API methods and data types used.
    pub introspect_mode: IntrospectMode,
    /// Generate multi read, multi write and multi call support code.
    /// Takes a bit more FLASH, but allows for more efficient requests in some cases.
    pub multi_req: bool,
    /// Path to a user type (usually an enum), telling handlers which medium (USB, CAN, ...) a request came from,
    /// see `wire_weaver::Context::medium()`. `()` if None.
    pub medium: Option<String>,
    /// `#[optional]` items to leave out (API-16): no dispatch arm, no handler, a call answers `Unimplemented`.
    pub omit: Vec<Omit>,
    /// Samples per batch of each `+batched` property (API-39), `observe_batch = N`; None:
    /// `ww_stream::batch::DEFAULT_BATCH` (32).
    pub observe_batch: Option<usize>,
    /// Bytes of the outbox of each trait level with `+batched` properties, `observe_outbox = N`; None: 4096.
    pub observe_outbox: Option<usize>,
    /// Levels sampled through observe cells (API-50), by their `ObserveState` field (`root_foc`):
    /// `observe_cells = "root_foc, .."`. The others keep `<Level>Sampler::sample`.
    pub observe_cells: Vec<String>,
}

pub enum IntrospectMode {
    Disabled,
    NoDocs,
    WithDocs,
}

/// API server code generation configuration.
/// Same as [GenServerConfig], but with syn::Path instead of String for `server_struct_path`.
/// More convenient to use from proc-macro context where Path is already available.
pub struct GenServerConfigRaw {
    /// Set to false to generate no_std no-alloc compatible code.
    pub no_alloc: bool,
    /// Set to true for an async server: handlers are async fn's, except those `handler_model` makes sync.
    pub use_async: bool,
    /// Which user handlers are `async fn` and which plain `fn` (SRV-21); empty: all follow `use_async`.
    pub handler_model: HandlerModel,
    /// Configure which user handlers return immediate response or answer later by using request id.
    pub method_model: MethodModel,
    /// Configure which properties are using get-set model and which are direct struct field access + on_changed fn model.
    pub property_model: PropertyModel,
    /// Configure which `const` items are Rust consts and which are `fn`s of the generated `<Trait>Consts` trait.
    pub const_model: ConstModel,
    /// Path to the user struct on which server dispatcher method will be implemented (process_request_bytes).
    /// Dispatcher expects all the user handlers also to be implemented on this struct by user code.
    pub server_struct_path: Path,
    /// Generate ww_self introspect bytes, fully describing all API methods and data types used.
    pub introspect_mode: IntrospectMode,
    /// Generate multi read, multi write and multi call support code.
    /// Takes a bit more FLASH, but allows for more efficient requests in some cases.
    pub multi_req: bool,
    /// Path to a user type (usually an enum), telling handlers which medium (USB, CAN, ...) a request came from,
    /// see `wire_weaver::Context::medium()`. `()` if None.
    pub medium: Option<Path>,
    /// `#[optional]` items to leave out (API-16): no dispatch arm, no handler, a call answers `Unimplemented`.
    pub omit: Vec<Omit>,
    /// Samples per batch of each `+batched` property (API-39), `observe_batch = N`; None:
    /// `ww_stream::batch::DEFAULT_BATCH` (32).
    pub observe_batch: Option<usize>,
    /// Bytes of the outbox of each trait level with `+batched` properties, `observe_outbox = N`; None: 4096.
    pub observe_outbox: Option<usize>,
    /// Levels sampled through observe cells (API-50), by their `ObserveState` field (`root_foc`):
    /// `observe_cells = "root_foc, .."`. The others keep `<Level>Sampler::sample`.
    pub observe_cells: Vec<String>,
}

impl From<GenServerConfig> for GenServerConfigRaw {
    fn from(config: GenServerConfig) -> Self {
        Self {
            no_alloc: config.no_alloc,
            use_async: config.use_async,
            handler_model: config.handler_model,
            method_model: config.method_model,
            property_model: config.property_model,
            const_model: config.const_model,
            server_struct_path: super::util::str_to_path(&config.server_struct_path),
            introspect_mode: config.introspect_mode,
            multi_req: config.multi_req,
            medium: config.medium.as_deref().map(super::util::str_to_path),
            omit: config.omit,
            observe_batch: config.observe_batch,
            observe_outbox: config.observe_outbox,
            observe_cells: config.observe_cells,
        }
    }
}

/// Generates API server code for the given API bundle and configuration.
/// ApiBundleOwned can be loaded using [crate::load] or [crate::load_dep].
/// Pass a [GenServerConfig] or [GenServerConfigRaw] to configure code generation.
///
/// Alternatively, use `wire_weaver_derive::ww_codegen!` proc-macro if you do not want to use build.rs.
pub fn gen_server(
    api_bundle: &ApiBundleOwned,
    config: impl Into<GenServerConfigRaw>,
) -> TokenStream {
    let config: GenServerConfigRaw = config.into();
    let omissions = match Omissions::resolve(api_bundle, &config.omit) {
        Ok(omissions) => omissions,
        Err(e) => return quote! { compile_error!(#e); },
    };
    // owned argument structs of a `no_alloc = false` server are read with `des_shrink_wrap_owned` (API-21)
    let additional_use = if config.no_alloc {
        quote! { #[allow(unused_imports)] use wire_weaver::shrink_wrap::RefVecIter; }
    } else {
        quote! { #[allow(unused_imports)] use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned; }
    };
    let maybe_async = maybe_quote(config.use_async, quote! { async });
    let handlers = Handlers {
        model: &config.handler_model,
        use_async: config.use_async,
    };
    let mut item_paths = vec![];
    collect_item_paths(api_bundle, &api_bundle.root, &mut vec![], &mut item_paths);
    let handler_model_errors = config
        .handler_model
        .check(item_paths.iter().map(String::as_str), config.use_async);
    if !handler_model_errors.is_empty() {
        return quote! { #(compile_error!(#handler_model_errors);)* };
    }
    let api_level = &api_bundle.root;
    let deferred_return_methods = deferred_method_return_ser_methods(
        api_bundle,
        api_level,
        None,
        config.no_alloc,
        &config.method_model,
    );
    let out_trait = if config.use_async {
        quote! { wire_weaver::EventOut }
    } else {
        quote! { wire_weaver::BlockingEventOut }
    };
    let crate_name = api_level.crate_name(api_bundle).unwrap();
    let medium_ty = match &config.medium {
        Some(path) => quote! { #path },
        None => quote! { () },
    };
    let mut observe = Observe::collect(api_bundle, &omissions, &config.property_model);
    let cells_errors = observe.use_cells(&config.observe_cells);
    let cx = ApiServerCGContext {
        observable: observe.observable(),
        ident_prefix: None,
        level_path: vec![],
        no_alloc: config.no_alloc,
        use_async: config.use_async,
        handlers,
        method_model: &config.method_model,
        property_model: &config.property_model,
        const_model: &config.const_model,
        consts_trait: consts_trait_ident(api_level),
        multi_req: config.multi_req,
        medium_ty: medium_ty.clone(),
    };
    let (introspect_enabled, include_docs) = match config.introspect_mode {
        IntrospectMode::Disabled => (false, false),
        IntrospectMode::NoDocs => (true, false),
        IntrospectMode::WithDocs => (true, true),
    };
    // the root level awaits when anything below it does, or to send the introspection events
    let root_await = maybe_quote(
        level_awaits(api_bundle, &api_bundle.root, &mut vec![], &cx)
            || (introspect_enabled && cx.use_async),
        quote! { .await },
    );
    let (handle_introspect, api_signature) = super::server::introspect::introspect(
        api_bundle,
        &omissions,
        introspect_enabled,
        include_docs,
        cx.use_async,
    );
    let process_request_inner = process_request_inner_recursive(
        "root".into(),
        &[],
        &quote! {},
        &omissions,
        api_bundle,
        api_level,
        IndexChain::new(),
        crate_name,
        &cx,
        Some(handle_introspect),
    );
    let stream_send_methods = stream_ser_methods_recursive(
        api_bundle,
        api_level,
        IndexChain::new(),
        config.no_alloc,
        true,
    );
    let mut args_structs = TokenStream::new();
    let mut seen = vec![];
    args_structs_recursive(
        api_bundle,
        api_level,
        config.no_alloc,
        &mut seen,
        &mut args_structs,
    );
    let mut max_len_consts = TokenStream::new();
    // the owned types of a `no_alloc = false` server have no MAX_SIZE
    if config.no_alloc {
        max_len_consts_recursive(
            api_bundle,
            api_level,
            None,
            &[],
            config.no_alloc,
            &mut max_len_consts,
        );
    }
    let poll_streams = poll_streams(api_bundle, &omissions, cx.handlers);
    let observe_items = observe.items(
        api_bundle,
        config.no_alloc,
        config.observe_batch,
        config.observe_outbox,
    );
    let observe_methods = observe.methods(api_bundle, cx.handlers, &medium_ty, config.no_alloc);
    let observe_arm = maybe_quote_cl(observe.is_enabled(), || {
        quote! {
            RequestKind::Observe { cfg } => {
                match self.observe_request(path, cfg, &mut wr) {
                    Ok(()) => {
                        if request.seq.0 == 0 {
                            return Ok(&[])
                        }
                        event_builder.finish(true, &mut wr);
                        wr.finish_and_take()
                    }
                    Err(e) => {
                        wr.restore_state(before_process);
                        wr.write(&e)?;
                        event_builder.finish(false, &mut wr);
                        wr.finish_and_take()
                    }
                }
            }
        }
    });
    let consts_trait = consts_trait(api_bundle, &omissions, &cx);
    let global_trait_path = global_trait_path(api_bundle);
    let (es1, es2) = (
        fixed_err_seq("REQUEST_TRAIT_NOT_FOUND"),
        fixed_err_seq("REQUEST_PATH_TOO_DEEP"),
    );
    let server_struct_path = config.server_struct_path;
    let maybe_in_multi_false = maybe_quote_cl(cx.multi_req, || quote! { false, });
    let maybe_multi_req = maybe_quote_cl(cx.multi_req, || multi_req_handlers(&root_await));
    let written_ok_variant = if cx.multi_req {
        quote! { WrittenOk(Option<EventKindBuilder>) }
    } else {
        quote! { WrittenOk(EventKindBuilder) }
    };
    let finalize_event_kind = if cx.multi_req {
        // normal requests create EventKindBuilder reserving a discriminant and return it wrapped in Some()
        // multi requests do nothing and simply write to wr
        quote! {
            if let Some(event_kind_builder) = event_kind_builder {
                event_kind_builder.finish_with_kind(request.kind.discriminants(), &mut wr);
            }
        }
    } else {
        quote! {
            event_kind_builder.finish_with_kind(request.kind.discriminants(), &mut wr);
        }
    };
    quote! {
        #args_structs
        #max_len_consts

        #[allow(unused_imports)]
        use wire_weaver::{RpcResult, SetResult, GetResult};
        #[allow(unused_imports)]
        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter,
            Error as ShrinkWrapError, nib32::UNib32, ElementSize, RefVec,
            buf_writer::BufWriterState,
            tail_bytes::TailBytes,
        };
        #[allow(unused_imports)]
        use ww_client_server::{
            Request, RequestKind, Event, EventKind, EventKindDiscriminants,
            PathKind, Error, ErrorKind, StreamSideband, ErrorKindDiscriminants,
            util::{ser_ok_event, ser_err_event, ser_unit_return_event},
            builder::{EventBuilder, EventKindBuilder, ErrorBuilder},
        };
        use core::slice::Iter;
        #additional_use
        #api_signature

        enum WrAction {
            #written_ok_variant,
            WrittenErr,
            /// Request handled, nothing to send back: answered with Written
            Acknowledged,
            Deferred
        }

        impl #server_struct_path {
            /// Returns an Error only if request deserialization or error serialization failed.
            /// If there are any other errors, they are sent to the remote.
            /// Events sent by handlers through their `wire_weaver::Context` go to `out`, `medium` is passed to them as is.
            pub #maybe_async fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl #out_trait,
                medium: #medium_ty,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd)?;
                let mut cx = wire_weaver::Context::new(request.seq.0, medium, out);

                let mut wr = BufWriter::new(scratch);
                let event_builder = EventBuilder::new(request.seq.0, &mut wr)?;

                // a global trait path continues from wherever the trait sits in this API (API-20)
                let (base, path): (&[UNib32], _) = match &request.path_kind {
                    PathKind::Absolute { path } => (&[], path),
                    PathKind::GlobalFull { gid, trait_name, path_from_trait } => {
                        match ww_global_trait_path(gid, trait_name) {
                            Some(base) => (base, path_from_trait),
                            None => {
                                wr.write(&Error::new(#es1, ErrorKind::TraitNotFound))?;
                                event_builder.finish(false, &mut wr);
                                return wr.finish_and_take();
                            }
                        }
                    }
                    PathKind::GlobalCompact { gid, path_from_trait } => {
                        match ww_global_compact_trait_path(gid) {
                            Some(base) => (base, path_from_trait),
                            None => {
                                wr.write(&Error::new(#es1, ErrorKind::TraitNotFound))?;
                                event_builder.finish(false, &mut wr);
                                return wr.finish_and_take();
                            }
                        }
                    }
                };

                const MAX_DEPTH: usize = 16; // TODO: calculate max depth
                let mut path_arr = [UNib32(0); MAX_DEPTH];
                let mut path_len: usize = 0;
                for (idx, resource_id) in base.iter().map(|id| Ok(*id)).chain(path.iter()).enumerate() {
                    let resource_id = resource_id?;
                    if idx < MAX_DEPTH {
                        path_arr[idx] = resource_id;
                        path_len += 1;
                    } else {
                        wr.write(&Error::new(#es2, ErrorKind::BadPath))?;
                        event_builder.finish(false, &mut wr);
                        return wr.finish_and_take();
                    }
                }

                let path = &path_arr[..path_len];
                let mut iter = path.iter();
                let before_process = wr.save_state();
                match &request.kind {
                    #maybe_multi_req
                    #observe_arm
                    _ => {
                        match self.process_root(path, &mut iter, &request, &mut wr, #maybe_in_multi_false &mut cx)#root_await {
                            Ok(WrAction::WrittenOk(event_kind_builder)) => {
                                if request.seq.0 == 0 {
                                    return Ok(&[])
                                }
                                #finalize_event_kind
                                event_builder.finish(true, &mut wr);
                                wr.finish_and_take()
                            }
                            Ok(WrAction::WrittenErr) => {
                                event_builder.finish(false, &mut wr);
                                wr.finish_and_take()
                            }
                            Ok(WrAction::Acknowledged) => {
                                if request.seq.0 == 0 {
                                    return Ok(&[])
                                }
                                let event_kind_builder = EventKindBuilder::new(&mut wr)?;
                                event_kind_builder.finish_with_kind(EventKindDiscriminants::Written, &mut wr);
                                event_builder.finish(true, &mut wr);
                                wr.finish_and_take()
                            }
                            Ok(WrAction::Deferred) => {
                                Ok(&[])
                            }
                            Err(e) => {
                                wr.restore_state(before_process); // handler could have started to write to wr but failed
                                wr.write(&e)?;
                                event_builder.finish(false, &mut wr);
                                wr.finish_and_take()
                            }
                        }
                    }
                }
            }

            #process_request_inner

            #deferred_return_methods

            #poll_streams

            #observe_methods
        }

        #observe_items
        #cells_errors

        #stream_send_methods

        #global_trait_path

        #consts_trait
    }
}

/// A stream `poll_streams` sends for: a reliable one, or a batched one of any delivery (API-41).
struct PolledStream {
    /// Handler name with the trait chain prefix
    ident: Ident,
    /// Dotted item path, for `handler_model`
    dotted: String,
    path: Vec<u32>,
    delivery: Delivery,
    batched: bool,
}

/// Streams of the API `poll_streams` sends for, or an error message for one it can't send.
/// Items left out in this combination of `omit_if` conditions (`active`, see [Omissions::cfg_combinations]) and their
/// streams are skipped.
#[allow(clippy::too_many_arguments)]
fn polled_streams_recursive(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    omissions: &Omissions,
    active: &[bool],
    prefix: Option<&String>,
    names: &mut Vec<String>,
    path: &mut Vec<u32>,
    under_array: bool,
    out: &mut Vec<Result<PolledStream, String>>,
) {
    let omitted = omissions.omitted_in(names, active);
    for item in &level.items {
        if omitted.contains(&item.id.0) {
            continue;
        }
        let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
        path.push(item.id.0);
        match &item.kind {
            ApiItemKindOwned::StreamFn {
                delivery, batched, ..
            } if *delivery == Delivery::Reliable || batched.is_some() => {
                if under_array || is_array {
                    let what = if batched.is_some() {
                        "batched"
                    } else {
                        "reliable"
                    };
                    out.push(Err(format!(
                        "{what} stream {} in a resource array: poll_streams can't send it yet (API-31)",
                        item.ident
                    )));
                } else {
                    let ident = Ident::new(&item.ident, Span::call_site());
                    out.push(Ok(PolledStream {
                        ident: add_prefix(prefix, &ident),
                        dotted: dotted(names, &item.ident),
                        path: path.clone(),
                        delivery: *delivery,
                        batched: batched.is_some(),
                    }));
                }
            }
            ApiItemKindOwned::Trait { .. } => {
                let child = item.get_as_level(api_bundle).unwrap();
                let prefix = match prefix {
                    Some(prefix) => format!("{prefix}_{}", item.ident),
                    None => item.ident.clone(),
                };
                names.push(item.ident.clone());
                polled_streams_recursive(
                    api_bundle,
                    child,
                    omissions,
                    active,
                    Some(&prefix),
                    names,
                    path,
                    under_array || is_array,
                    out,
                );
                names.pop();
            }
            _ => {}
        }
        path.pop();
    }
}

/// `poll_streams` on the server: encodes sealed batches of batched streams (API-41) and sends those of counted and
/// plain ones right away, then sends what reliable streams have in their stores, as the send scheduler of
/// `ww_stream` picks (priority, live before backfill, round-robin), and their queries. Not generated without
/// reliable or batched streams. A blocking server gets a blocking one for batched counted and plain streams
/// (reliable streams need an async server).
///
/// With `omit_if` conditions that leave out polled streams (or the traits holding them), one `poll_streams` per
/// combination of the conditions, each under its `#[cfg(..)]`, with the same signature.
fn poll_streams(
    api_bundle: &ApiBundleOwned,
    omissions: &Omissions,
    handlers: Handlers<'_>,
) -> TokenStream {
    let mut errors: Vec<String> = vec![];
    let mut combinations = vec![];
    for (cfg, active) in omissions.cfg_combinations() {
        let mut streams = vec![];
        polled_streams_recursive(
            api_bundle,
            &api_bundle.root,
            omissions,
            &active,
            None,
            &mut vec![],
            &mut vec![],
            false,
            &mut streams,
        );
        let mut ok = vec![];
        for s in streams {
            match s {
                Ok(s) => ok.push(s),
                Err(e) if !errors.contains(&e) => errors.push(e),
                Err(_) => {}
            }
        }
        if !handlers.use_async {
            // a blocking server with reliable streams already got a compile error from its dispatcher
            ok.retain(|s| s.delivery != Delivery::Reliable);
        }
        combinations.push((cfg, ok));
    }
    let errors = quote! { #(compile_error!(#errors);)* };
    if combinations.iter().all(|(_, ok)| ok.is_empty()) {
        return errors;
    }
    let any_batched = combinations
        .iter()
        .any(|(_, ok)| ok.iter().any(|s| s.batched));
    let paths = |ok: &[PolledStream]| ok.iter().map(|s| s.path.clone()).collect::<Vec<_>>();
    let first = paths(&combinations[0].1);
    if combinations.iter().all(|(_, ok)| paths(ok) == first) {
        let ok = &combinations[0].1;
        let poll_streams = poll_streams_fn(ok, None, any_batched, handlers);
        return quote! { #errors #poll_streams };
    }
    let fns = combinations
        .iter()
        .map(|(cfg, ok)| poll_streams_fn(ok, cfg.as_ref(), any_batched, handlers));
    quote! { #errors #(#fns)* }
}

/// `poll_streams` sending for `ok`, under `#[cfg(cfg)]`; `any_batched` when a batched stream is in any combination
/// of `omit_if` conditions (the `scratch` argument).
fn poll_streams_fn(
    ok: &[PolledStream],
    cfg: Option<&TokenStream>,
    any_batched: bool,
    handlers: Handlers<'_>,
) -> TokenStream {
    let use_async = handlers.use_async;
    let maybe_async = maybe_quote(use_async, quote! { async });
    // `self.stream_<name>()`, the stream's state
    let state_of = |s: &PolledStream| {
        let state_fn = Ident::new(&format!("stream_{}", s.ident), s.ident.span());
        handlers.call(&s.dotted, quote! { self.#state_fn() })
    };
    let path_of = |s: &PolledStream| {
        let path = &s.path;
        quote! { [#(UNib32(#path)),*] }
    };
    let send_with = |closure: TokenStream| {
        if use_async {
            quote! { wire_weaver::EventOut::send_with(out, #closure).await }
        } else {
            quote! { wire_weaver::BlockingEventOut::send_with_blocking(out, #closure) }
        }
    };
    let this_batched = ok.iter().any(|s| s.batched);

    // batched counted and plain streams: every sealed batch goes out right away
    let mut batched_direct = TokenStream::new();
    // batched reliable streams: sealed batches go into the store
    let mut batched_store = TokenStream::new();
    for s in ok.iter().filter(|s| s.batched) {
        let state = state_of(s);
        let path = path_of(s);
        match s.delivery {
            Delivery::Counted => {
                let send = send_with(quote! {
                    |scratch| {
                        ww_client_server::stream::ser_stream_item_event(scratch, &path, &head, |b: &mut [u8]| {
                            if bytes.len() > b.len() {
                                return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                            }
                            b[..bytes.len()].copy_from_slice(bytes);
                            Ok(bytes.len())
                        })
                        .map(|bytes| bytes.len())
                    }
                });
                batched_direct.extend(quote! {
                    loop {
                        let state = #state;
                        let max_age = Some(state.max_age_us() as u64);
                        // a batch the series writer can't write stays sealed until the next turn
                        let Ok(Some((head, bytes))) = state.poll(now_us, max_age, scratch, buf) else {
                            break;
                        };
                        let path = #path;
                        let r = #send;
                        if let Err(e) = r {
                            #state.stream.unsent(&head);
                            return Err(e);
                        }
                    }
                });
            }
            Delivery::Plain => {
                let send = send_with(quote! {
                    |scratch| {
                        ww_client_server::stream::ser_stream_data_event(scratch, &path, |b: &mut [u8]| {
                            if bytes.len() > b.len() {
                                return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                            }
                            b[..bytes.len()].copy_from_slice(bytes);
                            Ok(bytes.len())
                        })
                        .map(|bytes| bytes.len())
                    }
                });
                batched_direct.extend(quote! {
                    loop {
                        let state = #state;
                        let max_age = Some(state.max_age_us() as u64);
                        let Ok(Some((_items, bytes))) = state.poll(now_us, max_age, scratch, buf) else {
                            break;
                        };
                        let path = #path;
                        #send?;
                    }
                });
            }
            Delivery::Reliable => {
                batched_store.extend(quote! {
                    {
                        let state = #state;
                        let max_age = Some(state.max_age_us() as u64);
                        // losses are counted in the stream, a store error shows in its status
                        let _ = state.poll(now_us, max_age, scratch, buf).await;
                    }
                });
            }
        }
    }

    let reliable: Vec<&PolledStream> = ok
        .iter()
        .filter(|s| s.delivery == Delivery::Reliable)
        .collect();
    let reliable_part = if reliable.is_empty() {
        quote! {}
    } else {
        let n = reliable.len();
        let states: Vec<TokenStream> = reliable
            .iter()
            .map(|s| {
                let state = state_of(s);
                let inner = maybe_quote(s.batched, quote! { .stream });
                quote! { #state #inner }
            })
            .collect();
        let paths: Vec<TokenStream> = reliable.iter().map(|s| path_of(s)).collect();
        let idx = 0..n;
        quote! {
            #(
                if let Some(sideband) = #states.poll_query(now_ms) {
                    let path = #paths;
                    wire_weaver::EventOut::send_with(out, |scratch| {
                        ser_ok_event(scratch, 0, EventKind::StreamSideband { path: RefVec::Slice { slice: &path }, sideband })
                            .map(|bytes| bytes.len())
                    }).await?;
                }
            )*
            let mut sched = ww_stream::sched::Scheduler::<#n>::new(budget_bytes);
            sched.start_turn();
            loop {
                let ready = [#(#states.ready()),*];
                let Some((i, run)) = sched.pick(&ready) else {
                    break;
                };
                let sent = match i {
                    #(
                        #idx => {
                            let ev = match #states.next_event(run, buf).await {
                                Ok(Some(ev)) => ev,
                                Ok(None) | Err(_) => {
                                    sched.skip(i, run);
                                    continue;
                                }
                            };
                            let path = #paths;
                            wire_weaver::EventOut::send_with(out, |scratch| {
                                ww_client_server::stream::ser_stream_item_event(scratch, &path, &ev.head, |b: &mut [u8]| {
                                    if ev.item.len() > b.len() {
                                        return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                                    }
                                    b[..ev.item.len()].copy_from_slice(ev.item);
                                    Ok(ev.item.len())
                                })
                                .map(|bytes| bytes.len())
                            }).await?;
                            ev.item.len()
                        }
                    )*
                    _ => break,
                };
                sched.sent(sent);
            }
        }
    };
    let (scratch_arg, now_us, scratch_doc) = if any_batched {
        (
            quote! { scratch: &mut ww_stream::batch::Scratch<SCRATCH>, },
            if this_batched {
                quote! { let now_us = now_ms.saturating_mul(1000); }
            } else {
                // batched streams are left out in this combination of omit_if conditions
                quote! { let _ = scratch; }
            },
            quote! {
                ///
                /// Batched streams (API-41): a batch is sealed once its first item is as old as the host allows (seen
                /// at a call, so call it at least that often), and encoded once into `buf` (so a batch is at most
                /// `buf.len()` bytes, leave room for the event's header in the link's messages); counted and plain
                /// ones are sent right away, reliable ones go into the store. `scratch` is where a batch's items
                /// are rebuilt while it is encoded, one for every batched stream:
                /// `Scratch<{ scratch_bytes::<Item>(N) }>` of the biggest `N` items.
            },
        )
    } else {
        (quote! {}, quote! {}, quote! {})
    };
    // without reliable streams the scheduler's inputs go unused
    let mut unused = maybe_quote(
        reliable.is_empty(),
        quote! { let _ = (now_ms, budget_bytes); },
    );
    if ok.is_empty() {
        // every polled stream is left out in this combination of omit_if conditions
        unused.extend(quote! { let _ = (out, buf); });
    }
    let cfg = cfg.map(|cfg| quote! { #[cfg(#cfg)] });
    let generics = if any_batched {
        quote! { <const SCRATCH: usize> }
    } else {
        quote! {}
    };
    let (out_bound, budget_doc) = if use_async {
        (
            quote! { impl wire_weaver::EventOut },
            quote! {
                /// Send what the reliable streams have, between requests: events by priority, the live run before the
                /// backfill run, round-robin within a priority, up to `budget_bytes` of items (at least one event), and a
                /// `Query` for a stream whose host was silent. `buf` must hold the biggest item of any reliable stream;
                /// `now_ms` is any millisecond clock. A stream whose store fails is skipped this turn.
            },
        )
    } else {
        (
            quote! { impl wire_weaver::BlockingEventOut },
            quote! {
                /// Send the batches of the batched streams, between requests; `now_ms` is any millisecond clock.
                /// `budget_bytes` is for reliable streams, which a blocking server doesn't have.
            },
        )
    };
    quote! {
        #budget_doc
        #scratch_doc
        #cfg
        pub #maybe_async fn poll_streams #generics (
            &mut self,
            out: &mut #out_bound,
            now_ms: u64,
            buf: &mut [u8],
            budget_bytes: usize,
            #scratch_arg
        ) -> Result<(), wire_weaver::SendError> {
            #unused
            #now_us
            #batched_direct
            #batched_store
            #reliable_part
            Ok(())
        }
    }
}

/// Absolute path of every global trait in the API tree, for requests with a global trait path (API-20), by crate and
/// trait name (`GlobalFull`) or by the trait's `ww_global` id (`GlobalCompact`).
///
/// Traits under a resource array are left out (a global path can't select an index), and when a trait is implemented
/// several times, the first one in depth first, id order is used. A trait matches when the requested crate version is
/// protocol compatible with the one this API was built with (same major, and same minor before 1.0).
fn global_trait_path(api_bundle: &ApiBundleOwned) -> TokenStream {
    let mut traits = vec![];
    global_traits_recursive(api_bundle, &api_bundle.root, &mut vec![], &mut traits);
    let version_check = |gid: TokenStream, crate_version: &FullVersionOwned| {
        let (major, minor) = (crate_version.version.major.0, crate_version.version.minor.0);
        if major >= 1 {
            quote! { #gid.major.0 == #major }
        } else {
            quote! { #gid.major.0 == 0 && #gid.minor.0 == #minor }
        }
    };
    let arms = traits.iter().map(|t| {
        let crate_id = t.crate_version.crate_id.as_str();
        let trait_name = t.trait_name.as_str();
        let path = &t.path;
        let version_check = version_check(quote! { gid.version }, &t.crate_version);
        quote! {
            (#crate_id, #trait_name) if #version_check => Some(&[#(UNib32(#path)),*]),
        }
    });
    let compact_arms = traits.iter().filter_map(|t| {
        let id = t.compact_id?;
        let path = &t.path;
        let version_check = version_check(quote! { gid }, &t.crate_version);
        Some(quote! {
            #id if #version_check => Some(&[#(UNib32(#path)),*]),
        })
    });
    quote! {
        /// Absolute path of a global trait in this API, None if it's not implemented in a compatible version.
        fn ww_global_trait_path(gid: &wire_weaver::ww_version::FullVersion<'_>, trait_name: &str) -> Option<&'static [UNib32]> {
            match (gid.crate_id, trait_name) {
                #(#arms)*
                _ => None,
            }
        }

        /// Absolute path of a global trait in this API by its `ww_global` id, None if it's not implemented in a
        /// compatible version or has no id.
        #[allow(unused_variables)]
        fn ww_global_compact_trait_path(gid: &wire_weaver::ww_version::CompactVersion) -> Option<&'static [UNib32]> {
            match gid.gid.id.0 {
                #(#compact_arms)*
                _ => None,
            }
        }
    }
}

/// A trait in the API tree that a global path can reach.
struct GlobalTrait {
    path: Vec<u32>,
    crate_version: FullVersionOwned,
    trait_name: String,
    compact_id: Option<u32>,
}

fn global_traits_recursive(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    path: &mut Vec<u32>,
    traits: &mut Vec<GlobalTrait>,
) {
    let crate_version = api_bundle.crate_version(level.crate_idx.0).unwrap();
    if !traits.iter().any(|t| {
        t.crate_version.crate_id == crate_version.crate_id && t.trait_name == level.trait_name
    }) {
        traits.push(GlobalTrait {
            path: path.clone(),
            crate_version: crate_version.clone(),
            trait_name: level.trait_name.clone(),
            compact_id: level.compact_id.map(|id| id.0),
        });
    }
    for item in &level.items {
        if !matches!(item.kind, ApiItemKindOwned::Trait { .. })
            || !matches!(item.multiplicity, Multiplicity::Flat)
        {
            continue;
        }
        path.push(item.id.0);
        global_traits_recursive(
            api_bundle,
            item.get_as_level(api_bundle).unwrap(),
            path,
            traits,
        );
        path.pop();
    }
}

/// `MultiCall`, `MultiRead` and `MultiWrite`: each index is one more path element after the request's path (plus
/// `in_each_array_id` after it), processed as a `Call`, `Read` or `Write` on its own. Answered with
/// `EventKind::Value` holding an `EitherAnyVec` with `Either<Error, value>` per index, in index order (a write's
/// value is empty). An index that fails answers with an error in its slot, the others go on; `MultiArgs::Different`
/// is a serialized `Vec<Vec<u8>>` with the arguments (or data) of each index. The whole request fails when the path
/// gets too deep, `Different` doesn't deserialize, or the answer doesn't fit into the scratch buffer.
fn multi_req_handlers(maybe_await: &TokenStream) -> TokenStream {
    let (es_depth, es_args) = (
        fixed_err_seq("MULTI_PATH_TOO_DEEP"),
        fixed_err_seq("MULTI_ARGS"),
    );
    let (es_deferred, es_overflow) = (
        fixed_err_seq("MULTI_DEFERRED"),
        fixed_err_seq("MULTI_OVERFLOW"),
    );
    quote! {
        RequestKind::MultiCall {
            multi_idx,
            in_each_array_id,
            ..
        } | RequestKind::MultiRead {
            multi_idx,
            in_each_array_id,
        } | RequestKind::MultiWrite {
            multi_idx,
            in_each_array_id,
            ..
        } => {
            let item_path_len = path_len + 1 + in_each_array_id.is_some() as usize;
            if item_path_len > MAX_DEPTH {
                wr.write(&Error::new(#es_depth, ErrorKind::BadPath))?;
                event_builder.finish(false, &mut wr);
                return wr.finish_and_take();
            }
            if let Some(resource_id) = *in_each_array_id {
                path_arr[path_len + 1] = resource_id;
            }
            let multi_args = match &request.kind {
                RequestKind::MultiCall { multi_args, .. } => Some(multi_args),
                RequestKind::MultiWrite { multi_data, .. } => Some(multi_data),
                _ => None,
            };
            let mut different = match multi_args {
                Some(ww_client_server::MultiArgs::Different(bytes)) => {
                    let mut rd = BufReader::new(bytes.as_slice());
                    match wire_weaver::shrink_wrap::RefVec::<'_, wire_weaver::shrink_wrap::RefVec<'_, u8>>::des_shrink_wrap(&mut rd) {
                        Ok(list) => Some(list.iter()),
                        Err(_) => {
                            wr.write(&Error::new(#es_args, ErrorKind::ArgsDesFailed))?;
                            event_builder.finish(false, &mut wr);
                            return wr.finish_and_take();
                        }
                    }
                }
                _ => None,
            };
            let event_kind_builder = EventKindBuilder::new(&mut wr)?;
            let mut items = wire_weaver::shrink_wrap::either_any_vec::EitherAnyVecBuilder::new(&wr);
            let mut overflow = false;
            for index in multi_idx.iter() {
                path_arr[path_len] = index;
                let path = &path_arr[..item_path_len];
                let mut iter = path.iter();
                let item_args = match (multi_args, different.as_mut()) {
                    (Some(ww_client_server::MultiArgs::Same(bytes)), _) => Some(Some(*bytes)),
                    (_, Some(list)) => match list.next() {
                        Some(Ok(bytes)) => Some(Some(bytes)),
                        // fewer arguments than indices, or a broken entry
                        _ => None,
                    },
                    _ => Some(None),
                };
                let item_kind = match (&request.kind, &item_args) {
                    (RequestKind::MultiRead { .. }, _) => Some(RequestKind::Read),
                    (RequestKind::MultiCall { .. }, Some(Some(args))) => {
                        Some(RequestKind::Call { args: TailBytes(args.as_slice()) })
                    }
                    (RequestKind::MultiWrite { .. }, Some(Some(data))) => {
                        Some(RequestKind::Write { data: TailBytes(data.as_slice()) })
                    }
                    _ => None,
                };
                let Ok(marker) = items.write_item_start(&mut wr) else {
                    overflow = true;
                    break;
                };
                let item_start = wr.save_state();
                let result = match item_kind {
                    Some(kind) => {
                        let item_request = Request {
                            seq: request.seq,
                            path_kind: PathKind::Absolute { path: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] } },
                            kind,
                        };
                        self.process_root(path, &mut iter, &item_request, &mut wr, true, &mut cx)#maybe_await
                    }
                    None => Err(Error::new(#es_args, ErrorKind::ArgsDesFailed)),
                };
                let item_err = match result {
                    Ok(WrAction::WrittenOk(_) | WrAction::Acknowledged) => None,
                    Ok(WrAction::WrittenErr) => {
                        items.write_item_finish(marker, false, &mut wr);
                        continue;
                    }
                    // TODO(CLT-15): a deferred answer inside a multi request, the handler answers later with the
                    // multi request's seq, which the host doesn't expect
                    Ok(WrAction::Deferred) => Some(Error::not_supported(#es_deferred)),
                    Err(e) => Some(e),
                };
                let is_ok = item_err.is_none();
                if let Some(e) = item_err {
                    // the handler could have started to write before it failed
                    wr.restore_state(item_start);
                    if wr.write(&e).is_err() {
                        overflow = true;
                        break;
                    }
                }
                items.write_item_finish(marker, is_ok, &mut wr);
            }
            if overflow {
                wr.restore_state(before_process);
                wr.write(&Error::response_ser_failed(#es_overflow))?;
                event_builder.finish(false, &mut wr);
                return wr.finish_and_take();
            }
            if request.seq.0 == 0 {
                return Ok(&[])
            }
            items.finish(&mut wr);
            event_kind_builder.finish_with_kind(EventKindDiscriminants::Value, &mut wr);
            event_builder.finish(true, &mut wr);
            wr.finish_and_take()
        }
    }
}

#[derive(Clone)]
struct ApiServerCGContext<'i> {
    /// Prefixed names of observable properties (SRV-18), they have a `mark_changed_<name>()`
    observable: BTreeSet<String>,
    ident_prefix: Option<String>,
    /// Names of the trait items on the way to this level, for `handler_model` paths
    level_path: Vec<String>,
    no_alloc: bool,
    use_async: bool,
    handlers: Handlers<'i>,
    method_model: &'i MethodModel,
    property_model: &'i PropertyModel,
    const_model: &'i ConstModel,
    /// `<RootTrait>Consts`, implemented by the user on the server struct, with the values of `const` items
    consts_trait: Ident,
    multi_req: bool,
    /// User medium type or `()`
    medium_ty: TokenStream,
}

impl<'i> ApiServerCGContext<'i> {
    fn push_suffix(&mut self, suffix: &str) {
        if let Some(prefix) = &self.ident_prefix {
            self.ident_prefix = Some(format!("{}_{}", prefix, suffix));
        } else {
            self.ident_prefix = Some(suffix.to_string());
        }
        self.level_path.push(suffix.to_string());
    }

    /// The call of a handler of item `ident` of this level, awaited if it's async (SRV-21).
    fn call(&self, ident: &Ident, call: TokenStream) -> TokenStream {
        self.handlers
            .call(&dotted(&self.level_path, &ident.to_string()), call)
    }
}

/// Dotted paths of every method, property and stream of the API, for checking `handler_model` patterns.
fn collect_item_paths(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    level_path: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    for item in &level.items {
        if let ApiItemKindOwned::Trait { .. } = item.kind {
            level_path.push(item.ident.clone());
            collect_item_paths(
                api_bundle,
                item.get_as_level(api_bundle).unwrap(),
                level_path,
                out,
            );
            level_path.pop();
        } else {
            out.push(dotted(level_path, &item.ident));
        }
    }
}

/// Whether the dispatch of `level` (`process_<level>`) awaits anything, so has to be an `async fn`: an async
/// handler (SRV-21) or a store / event send of the generated code itself, in it or in a level below.
fn level_awaits(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    level_path: &mut Vec<String>,
    cx: &ApiServerCGContext<'_>,
) -> bool {
    level.items.iter().any(|item| match &item.kind {
        ApiItemKindOwned::Trait { .. } => {
            level_path.push(item.ident.clone());
            let awaits = level_awaits(
                api_bundle,
                item.get_as_level(api_bundle).unwrap(),
                level_path,
                cx,
            );
            level_path.pop();
            awaits
        }
        ApiItemKindOwned::Property {
            access: PropertyAccess::Const,
            ..
        } => false,
        // a reliable stream's store is async
        ApiItemKindOwned::StreamFn {
            delivery: Delivery::Reliable,
            ..
        } => cx.use_async,
        // credit sideband events are sent from the dispatch
        ApiItemKindOwned::FlowSink {
            flow: FlowControl::Credit,
            ..
        } => cx.use_async,
        _ => cx.handlers.is_async(&dotted(level_path, &item.ident)),
    })
}

#[allow(clippy::too_many_arguments)]
fn process_request_inner_recursive(
    level_name_chain: String,
    item_path: &[String],
    cfg_attrs: &TokenStream,
    omissions: &Omissions,
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    crate_name: &str,
    cx: &ApiServerCGContext<'_>,
    introspect: Option<TokenStream>,
) -> TokenStream {
    let maybe_async = maybe_quote(
        level_awaits(api_bundle, api_level, &mut item_path.to_vec(), cx)
            || (introspect.is_some() && cx.use_async),
        quote! { async },
    );
    let omitted = omissions.level(item_path);
    let level_matchers = level_matchers(
        api_bundle,
        api_level,
        &omitted,
        &level_name_chain,
        index_chain,
        crate_name,
        cx,
    );
    let maybe_index_chain_def = index_chain.fun_argument_def();

    let process_fn_name = Ident::new(
        format!("process_{}", level_name_chain).as_str(),
        Span::call_site(),
    );
    let es = fixed_err_seq("LEVEL_NOT_SUPPORTED");
    let maybe_in_multi = maybe_quote_cl(cx.multi_req, || quote! { in_multi: bool, });
    let context_ty = cx.context_ty();
    let mut ts = quote! {
        // index chain and in_multi make 8 below an array
        #[allow(clippy::too_many_arguments)]
        #cfg_attrs
        #maybe_async fn #process_fn_name<'a>(
            &mut self,
            #maybe_index_chain_def
            path: &[UNib32],
            path_iter: &mut Iter<'_, UNib32>,
            request: &Request<'_>,
            wr: &mut BufWriter<'_>,
            #maybe_in_multi
            cx: &mut #context_ty,
        ) -> Result<WrAction, Error<'a>> {
            match path_iter.next().copied() {
                #level_matchers
                None => {
                    match request.kind {
                        #introspect
                        _ => { Err(Error::not_supported(#es)) },
                    }
                }
            }
        }
    };

    for item in &api_level.items {
        if !matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
            continue;
        };
        // an omitted `#[optional] ww_impl!` (API-54): no dispatch below it, so no handler of it is called
        let cfg_attrs = match omitted.get(&item.id.0) {
            Some(Omission::Always) => continue,
            Some(Omission::When(cfg)) => quote! { #cfg_attrs #[cfg(not(#cfg))] },
            None => cfg_attrs.clone(),
        };
        let level = item.get_as_level(api_bundle).unwrap();
        let mut cx = cx.clone();
        cx.push_suffix(&item.ident);
        let mut index_chain = index_chain;
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
        let level_name_chain = format!("{}_{}", level_name_chain, item.ident);
        let mut item_path = item_path.to_vec();
        item_path.push(item.ident.clone());
        ts.extend(process_request_inner_recursive(
            level_name_chain,
            &item_path,
            &cfg_attrs,
            omissions,
            api_bundle,
            level,
            index_chain,
            level.crate_name(api_bundle).unwrap(),
            &cx,
            None,
        ));
    }
    ts
}

fn mod_ident(level: &ApiLevelOwned, crate_name: &str) -> Ident {
    Ident::new(
        format!("{}_{}", crate_name, level.trait_name.to_case(Case::Snake)).as_str(),
        Span::call_site(),
    )
}

#[allow(clippy::too_many_arguments)]
fn level_matchers(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    omitted: &BTreeMap<u32, Omission>,
    level_name_chain: &str,
    index_chain: IndexChain,
    crate_name: &str,
    cx: &ApiServerCGContext<'_>,
) -> TokenStream {
    let present = api_level
        .items
        .iter()
        .filter(|item| !matches!(omitted.get(&item.id.0), Some(Omission::Always)));
    let arm_attrs = present.clone().map(|item| match omitted.get(&item.id.0) {
        Some(Omission::When(cfg)) => quote! { #[cfg(not(#cfg))] },
        _ => quote! {},
    });
    let ids = present.clone().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u32", item.id.0).as_str(),
            Span::call_site(),
        ))
    });
    let es = fixed_err_seq("LEVEL_BAD_PATH");
    // An omitted optional item keeps its id: answered as unimplemented with one comparison, no arm per item.
    let omitted_arm = if omitted.is_empty() {
        quote! {}
    } else {
        let item_count = api_level
            .items
            .iter()
            .map(|item| item.id.0 + 1)
            .max()
            .unwrap_or(0);
        let item_count = Lit::Int(LitInt::new(
            format!("{item_count}u32").as_str(),
            Span::call_site(),
        ));
        let es = fixed_err_seq("LEVEL_OMITTED");
        quote! { id if id < #item_count => { Err(Error::unimplemented(#es)) } }
    };
    let handlers = present.map(|item| match &item.multiplicity {
        Multiplicity::Flat => level_matcher(
            api_bundle,
            item,
            level_name_chain,
            index_chain,
            mod_ident(api_level, crate_name),
            cx,
        ),
        Multiplicity::Array { .. } => {
            let mut index_chain_with_this_index = index_chain;
            let maybe_index_chain_push =
                index_chain_with_this_index.push_back(quote! {}, quote! { index });
            let lm = level_matcher(
                api_bundle,
                item,
                level_name_chain,
                index_chain_with_this_index,
                mod_ident(api_level, crate_name),
                cx,
            );

            let valid_indices = Ident::new(
                format!(
                    "valid_indices_{level_name_chain}_{}",
                    item.ident.to_case(Case::Snake)
                )
                .as_str(),
                Span::call_site(),
            );
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            let es_index = fixed_err_seq("LEVEL_BAD_INDEX");
            let validate_index = quote! {
                if !self.#valid_indices(#maybe_index_chain_arg).contains(index.0) {
                    return Err(Error::new(#es_index, ErrorKind::BadIndex));
                }
            };
            let es = fixed_err_seq("LEVEL_INDEX_EXPECTED");
            let ser_indices = ser_value(
                cx.multi_req,
                quote! { indices },
                fixed_err_seq("LEVEL_INDICES_SER"),
            );
            let ev_kind_builder = cx.prepare_event_kind_builder();
            quote! {
                match path_iter.next().copied() {
                    Some(index) => {
                        // let index = index #check_err_on_no_alloc;
                        #validate_index
                        #maybe_index_chain_push
                        #lm
                    }
                    None => {
                        if let RequestKind::Read /* ValidIndices */ = request.kind {
                            let indices = self.#valid_indices(#maybe_index_chain_arg);
                            #ev_kind_builder
                            #ser_indices
                            Ok(WrAction::WrittenOk(event_kind_builder))
                        } else {
                            Err(Error::new(#es, ErrorKind::ExpectedArrayIndexGotNone))
                        }
                    }
                }
            }
        }
    });
    quote! {
        Some(id) => match id.0 {
            #(#arm_attrs #ids => { #handlers } ),*
            #omitted_arm
            _ => { Err(Error::bad_path(#es)) }
        }
    }
}

fn level_matcher(
    api_bundle: &ApiBundleOwned,
    api_item: &ApiItemOwned,
    level_name_chain: &str,
    index_chain: IndexChain,
    mod_ident: Ident,
    cx: &ApiServerCGContext<'_>,
) -> TokenStream {
    let ident = Ident::new(&api_item.ident, Span::call_site());
    match &api_item.kind {
        ApiItemKindOwned::Method {
            args, return_ty, ..
        } => handle_method(
            api_bundle,
            index_chain,
            &mod_ident,
            cx,
            &ident,
            args,
            return_ty,
        ),
        ApiItemKindOwned::Property {
            ty,
            access,
            write_err_ty,
            ..
        } => handle_property(
            api_bundle,
            index_chain,
            cx,
            &ident,
            ty,
            write_err_ty,
            *access,
        ),
        ApiItemKindOwned::Stream { is_up: true, .. } | ApiItemKindOwned::StreamFn { .. } => {
            handle_stream_fn(
                api_bundle,
                index_chain,
                &mod_ident,
                cx,
                &ident,
                api_item.kind.as_stream_fn().expect("a stream"),
            )
        }
        ApiItemKindOwned::Stream { ty, is_up: false } => {
            handle_stream(api_bundle, index_chain, cx, &ident, ty, false, None)
        }
        ApiItemKindOwned::FlowSink { ty, flow } => {
            handle_stream(api_bundle, index_chain, cx, &ident, ty, false, Some(*flow))
        }
        ApiItemKindOwned::Trait { .. } => {
            let process_fn_name = Ident::new(
                format!("process_{level_name_chain}_{}", api_item.ident).as_str(),
                Span::call_site(),
            );
            let mut level_path = cx.level_path.clone();
            level_path.push(api_item.ident.clone());
            let child = api_item.get_as_level(api_bundle).unwrap();
            let maybe_await = maybe_quote(
                level_awaits(api_bundle, child, &mut level_path, cx),
                quote! { .await },
            );
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            let maybe_in_multi = maybe_quote_cl(cx.multi_req, || quote! { in_multi, });
            quote! {
                Ok(self.#process_fn_name(#maybe_index_chain_arg path, path_iter, request, wr, #maybe_in_multi cx)#maybe_await?)
            }
        }
    }
}

impl ApiServerCGContext<'_> {
    /// Type of the `cx` argument passed down to every process_* fn and user handler.
    fn context_ty(&self) -> TokenStream {
        let medium_ty = &self.medium_ty;
        if self.use_async {
            quote! { wire_weaver::Context<'_, impl wire_weaver::EventOut, #medium_ty> }
        } else {
            quote! { wire_weaver::Context<'_, impl wire_weaver::BlockingEventOut, #medium_ty> }
        }
    }

    /// Type of a value a handler takes or gives: borrowed in a `no_alloc` server, owned (`Vec`, `String`,
    /// `..Owned`) otherwise (API-21).
    fn value_ty(&self, api_bundle: &ApiBundleOwned, ty: &TypeOwned, pos: TyPos) -> TokenStream {
        ty_def(api_bundle, ty, !self.no_alloc, pos).unwrap()
    }

    /// `des_shrink_wrap` of a [value_ty](Self::value_ty) (in `TyPos::Expr`), `des_shrink_wrap_owned` with alloc.
    fn des_value(&self, ty: &TokenStream) -> TokenStream {
        if self.no_alloc {
            quote! { #ty::des_shrink_wrap }
        } else {
            quote! { #ty::des_shrink_wrap_owned }
        }
    }

    /// Writes a handler's value into `wr` as the root of what follows (`ser_shrink_wrap`); an owned one through
    /// `BufWriter::ser_owned`, the same bytes. An expression giving `Result<(), ShrinkWrapError>`.
    fn ser_root_value(&self, value: TokenStream) -> TokenStream {
        ser_root_value(self.no_alloc, value)
    }

    /// Writes a handler's value into `wr` as an element (`BufWriter::write`, `write_owned` with alloc). An
    /// expression giving `Result<(), ShrinkWrapError>`.
    fn write_value(&self, value: TokenStream) -> TokenStream {
        if self.no_alloc {
            quote! { wr.write(&#value) }
        } else {
            quote! { wr.write_owned(&#value) }
        }
    }

    /// The answer to a read or call with a handler's value: as an element of a multi request, else as the root.
    fn ser_handler_value(&self, value: TokenStream, es: TokenStream) -> TokenStream {
        let root = self.ser_root_value(value.clone());
        if self.multi_req {
            let element = self.write_value(value);
            quote! {
                if in_multi {
                    #element.map_err(|_| Error::response_ser_failed(#es))?;
                } else {
                    #root.map_err(|_| Error::response_ser_failed(#es))?;
                }
            }
        } else {
            quote! {
                #root.map_err(|_| Error::response_ser_failed(#es))?;
            }
        }
    }

    fn prepare_event_kind_builder(&self) -> TokenStream {
        let es = fixed_err_seq("REPLY_START");
        if self.multi_req {
            quote! {
                let event_kind_builder = if in_multi {
                    None
                } else {
                    Some(EventKindBuilder::new(wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?)
                };
            }
        } else {
            quote! {
                let event_kind_builder = EventKindBuilder::new(wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_method(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    mod_ident: &Ident,
    cx: &ApiServerCGContext,
    ident: &Ident,
    args: &[ArgumentOwned],
    return_type: &Option<TypeOwned>,
) -> TokenStream {
    let maybe_enforce_ty = if let Some(ty) = return_type {
        // a `no_alloc = false` server's handlers return owned types (`Vec`, `String`, `..Owned`, API-21)
        let enforce_ty = cx.value_ty(api_bundle, ty, TyPos::Annotation);
        quote! {
            let output: #enforce_ty = output;
        }
    } else {
        quote! {}
    };
    let maybe_index_chain_arg = index_chain.fun_argument_call();

    let (args_des, args_list) = des_args(
        mod_ident,
        ident,
        args,
        cx.no_alloc,
        item_err_seq("method", "ARGS"),
    );
    let maybe_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args }
    };

    // a deferred method answers later with the request's seq, a multi request can't carry that yet: refused before
    // the handler runs (a handler that defers anyway is answered with the same error in multi_req_handlers)
    let maybe_refuse_in_multi = if cx.multi_req
        && cx
            .method_model
            .pick(&add_prefix(cx.ident_prefix.as_ref(), ident).to_string())
            == Some(MethodModelKind::Deferred)
    {
        let es = item_err_seq("method", "DEFERRED_IN_MULTI");
        quote! {
            // TODO(CLT-15): deferred methods in a multi request
            if in_multi {
                return Err(Error::not_supported(#es));
            }
        }
    } else {
        quote! {}
    };
    let call = {
        let prefixed = add_prefix(cx.ident_prefix.as_ref(), ident);
        cx.call(
            ident,
            quote! { self.#prefixed(cx, #maybe_index_chain_arg #args_list) },
        )
    };
    let (es_kind, es) = (
        item_err_seq("", "KIND_NOT_TAKEN"),
        item_err_seq("method", "HANDLER"),
    );
    let ser_output = cx.ser_handler_value(quote! { output }, item_err_seq("method", "OUTPUT"));
    let ev_kind_builder = cx.prepare_event_kind_builder();
    quote! {
        match &request.kind {
            RequestKind::Call { #maybe_args } => {
                #maybe_refuse_in_multi
                #args_des
                match #call {
                    RpcResult::Ready(output) => {
                        #ev_kind_builder
                        if request.seq.0 != 0 {
                            #maybe_enforce_ty
                            #ser_output
                        }
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    },
                    RpcResult::Deferred => {
                        Ok(WrAction::Deferred)
                    }
                    RpcResult::Unimplemented => {
                        Err(Error::unimplemented(#es))
                    }
                }
            }
            _ => {
                Err(Error::not_supported(#es_kind))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_property(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    cx: &ApiServerCGContext,
    ident: &Ident,
    ty: &TypeOwned,
    user_error_ty: &Option<TypeOwned>,
    access: PropertyAccess,
) -> TokenStream {
    if matches!(access, PropertyAccess::Const) {
        return handle_const(api_bundle, index_chain, cx, ident, ty);
    }
    let maybe_index_chain_arg = index_chain.fun_argument_call();
    let maybe_index_chain_indices = index_chain.array_indices();
    let enforce_ty = cx.value_ty(api_bundle, ty, TyPos::Expr);
    let des_value = cx.des_value(&enforce_ty);
    let es = item_err_seq("property", "USER_ERROR");
    let ser_user_err = user_error_ty
        .as_ref()
        .map(|ty| cx.value_ty(api_bundle, ty, TyPos::Annotation))
        .map(|enforce_user_err_ty| {
            let write_user_err = cx.write_value(quote! { user_err });
            quote! {
                let user_err: #enforce_user_err_ty = user_err;
                let err_builder = ErrorBuilder::new(#es, wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                #write_user_err.map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                err_builder.finish_with_kind(ErrorKindDiscriminants::UserBytes, wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
            }
        })
        .unwrap_or(quote! {
            let _unit: () = user_err;
            wr.write(&Error::new(#es, ErrorKind::UserBytes(RefVec::new_bytes(&[])))).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
        });
    let property_model_pick = cx
        .property_model
        .pick(ident.to_string().as_str())
        .unwrap_or(PropertyModelKind::GetSet);
    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);

    let maybe_set = maybe_quote_cl(
        matches!(
            access,
            PropertyAccess::WriteOnly | PropertyAccess::ReadWrite { .. }
        ),
        || {
            let des_and_set_property = match property_model_pick {
                PropertyModelKind::GetSet => {
                    let set_property = Ident::new(
                        format!("set_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    let set_call = cx.call(
                        ident,
                        quote! { self.#set_property(cx, #maybe_index_chain_arg value) },
                    );
                    let es = item_err_seq("property", "WRITE_DECODE");
                    let es_handler = item_err_seq("property", "HANDLER");
                    let ev_kind_builder = cx.prepare_event_kind_builder();
                    quote! {
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #des_value(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        match #set_call {
                            SetResult::Set => {
                                #ev_kind_builder
                                Ok(WrAction::WrittenOk(event_kind_builder))
                            },
                            SetResult::SetError(user_err) => {
                                // if request.seq.0 != 0 {
                                // always send errors back, even if they won't reach a user call site, they will show up in logs
                                #ser_user_err
                                Ok(WrAction::WrittenErr)
                            }
                            SetResult::Unimplemented => {
                                Err(Error::unimplemented(#es_handler))
                            }
                        }
                    }
                }
                PropertyModelKind::ValueOnChanged => {
                    let mark_changed =
                        maybe_quote_cl(cx.observable.contains(&prefixed_ident.to_string()), || {
                            let mark_changed = mark_changed_ident(&prefixed_ident);
                            quote! { self.#mark_changed(); }
                        });
                    let changed_property = Ident::new(
                        format!("changed_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    let changed_call = cx.call(
                        ident,
                        quote! { self.#changed_property(cx, #maybe_index_chain_arg) },
                    );
                    let es = item_err_seq("property", "WRITE_DECODE");
                    let ev_kind_builder = cx.prepare_event_kind_builder();
                    quote! {
                        #ev_kind_builder
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #des_value(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        if self.#prefixed_ident #maybe_index_chain_indices != value {
                            self.#prefixed_ident #maybe_index_chain_indices = value;
                            #mark_changed
                            #changed_call;
                        }
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                }
            };
            quote! {
                RequestKind::Write { data } => {
                    #des_and_set_property
                }
            }
        },
    );

    let maybe_get = maybe_quote_cl(
        matches!(
            access,
            PropertyAccess::ReadOnly { .. } | PropertyAccess::ReadWrite { .. }
        ),
        || {
            let es = item_err_seq("property", "HANDLER");
            let ser_value =
                cx.ser_handler_value(quote! { value }, item_err_seq("property", "VALUE"));
            let ev_kind_builder = cx.prepare_event_kind_builder();
            let get_and_ser_property = match property_model_pick {
                PropertyModelKind::GetSet => {
                    let get_property = Ident::new(
                        format!("get_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    let get_call = cx.call(
                        ident,
                        quote! { self.#get_property(cx, #maybe_index_chain_arg) },
                    );
                    quote! {
                        match #get_call {
                            GetResult::Value(value) => {
                                #ev_kind_builder
                                let value: #enforce_ty = value;
                                #ser_value
                                Ok(WrAction::WrittenOk(event_kind_builder))
                            }
                            GetResult::GetError(user_err) => {
                                #ser_user_err
                                Ok(WrAction::WrittenErr)
                            }
                            GetResult::Unimplemented => {
                                wr.write(&Error::unimplemented(#es)).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                                Ok(WrAction::WrittenErr)
                            }
                        }
                    }
                }
                PropertyModelKind::ValueOnChanged => {
                    quote! {
                        #ev_kind_builder
                        let value: &#enforce_ty = &self.#prefixed_ident #maybe_index_chain_indices;
                        #ser_value
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                }
            };
            quote! {
                RequestKind::Read => {
                    #get_and_ser_property
                }
            }
        },
    );

    let es = item_err_seq("", "KIND_NOT_TAKEN");
    quote! {
        match &request.kind {
            #maybe_set
            #maybe_get
            _ => { Err(Error::not_supported(#es)) }
        }
    }
}

/// A `const` item (API-36): read only, its value comes from the generated `<Trait>Consts` trait the server struct
/// implements, an associated const or a `fn`, see [consts_trait].
fn handle_const(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    cx: &ApiServerCGContext,
    ident: &Ident,
    ty: &TypeOwned,
) -> TokenStream {
    let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Expr).unwrap();
    let ser_value = ser_value(
        cx.multi_req,
        quote! { value },
        item_err_seq("constant", "VALUE"),
    );
    let ev_kind_builder = cx.prepare_event_kind_builder();
    let consts_trait = &cx.consts_trait;
    let (const_ident, fn_ident) = const_idents(cx.ident_prefix.as_ref(), ident);
    let value = match cx.const_model.pick(&ident.to_string()) {
        ConstModelKind::Const => quote! { <Self as #consts_trait>::#const_ident },
        ConstModelKind::Fn => {
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            quote! { <Self as #consts_trait>::#fn_ident(self, #maybe_index_chain_arg) }
        }
    };
    let es = item_err_seq("", "KIND_NOT_TAKEN");
    quote! {
        match &request.kind {
            RequestKind::Read => {
                #ev_kind_builder
                let value: #enforce_ty = #value;
                #ser_value
                Ok(WrAction::WrittenOk(event_kind_builder))
            }
            _ => { Err(Error::not_supported(#es)) }
        }
    }
}

/// Name of the trait with the values of `const` items: `<RootTrait>Consts`.
fn consts_trait_ident(root: &ApiLevelOwned) -> Ident {
    Ident::new(&format!("{}Consts", root.trait_name), Span::call_site())
}

/// Names of a `const` item in the consts trait, as an associated const (`UPPER_SNAKE`) and as a `fn` (`snake`),
/// both with the trait item chain prefix: `BOARD` of `board: BoardInfo` is `BOARD_BOARD` / `board_board`.
fn const_idents(prefix: Option<&String>, ident: &Ident) -> (Ident, Ident) {
    let name = add_prefix(prefix, ident).to_string();
    (
        Ident::new(&name.to_case(Case::UpperSnake), ident.span()),
        Ident::new(&name.to_case(Case::Snake), ident.span()),
    )
}

/// `pub trait <RootTrait>Consts`: one associated const or `fn` (per `const_model`) for every `const` item of the
/// API, the server struct implements it. Not generated when the API has no `const` items.
fn consts_trait(
    api_bundle: &ApiBundleOwned,
    omissions: &Omissions,
    cx: &ApiServerCGContext,
) -> TokenStream {
    let mut items = TokenStream::new();
    consts_trait_recursive(
        api_bundle,
        &api_bundle.root,
        omissions,
        &[],
        &quote! {},
        None,
        IndexChain::new(),
        cx,
        &mut items,
    );
    if items.is_empty() {
        return quote! {};
    }
    let consts_trait = &cx.consts_trait;
    let trait_name = &api_bundle.root.trait_name;
    let doc = format!(
        " Values of the `const` items of `{trait_name}`, served without a handler: implement it on the server struct."
    );
    quote! {
        #[doc = #doc]
        #[allow(non_camel_case_types)]
        pub trait #consts_trait {
            #items
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn consts_trait_recursive(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    omissions: &Omissions,
    item_path: &[String],
    cfg_attrs: &TokenStream,
    prefix: Option<&String>,
    index_chain: IndexChain,
    cx: &ApiServerCGContext,
    items: &mut TokenStream,
) {
    let omitted = omissions.level(item_path);
    for item in &level.items {
        // a const left out with omit_if (or inside a trait left out so) is only in the trait when its arm is
        let cfg_attrs = match omitted.get(&item.id.0) {
            Some(Omission::Always) => continue,
            Some(Omission::When(cfg)) => quote! { #cfg_attrs #[cfg(not(#cfg))] },
            None => cfg_attrs.clone(),
        };
        let mut index_chain = index_chain;
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
        match &item.kind {
            ApiItemKindOwned::Property {
                ty,
                access: PropertyAccess::Const,
                ..
            } => {
                let ident = Ident::new(&item.ident, Span::call_site());
                let (const_ident, fn_ident) = const_idents(prefix, &ident);
                let docs = item.docs.iter().map(|line| quote! { #[doc = #line] });
                let item_def = match cx.const_model.pick(&item.ident) {
                    ConstModelKind::Const => {
                        let ty = ty_def(api_bundle, ty, false, TyPos::Static).unwrap();
                        quote! { const #const_ident: #ty; }
                    }
                    ConstModelKind::Fn => {
                        let ty = ty_def(api_bundle, ty, false, TyPos::Annotation).unwrap();
                        let index_chain_arg = index_chain.fun_argument_def();
                        quote! { fn #fn_ident(&self, #index_chain_arg) -> #ty; }
                    }
                };
                items.extend(quote! {
                    #(#docs)*
                    #cfg_attrs
                    #item_def
                });
            }
            ApiItemKindOwned::Trait { .. } => {
                let child = item.get_as_level(api_bundle).unwrap();
                let prefix = match prefix {
                    Some(prefix) => format!("{prefix}_{}", item.ident),
                    None => item.ident.clone(),
                };
                let mut item_path = item_path.to_vec();
                item_path.push(item.ident.clone());
                consts_trait_recursive(
                    api_bundle,
                    child,
                    omissions,
                    &item_path,
                    &cfg_attrs,
                    Some(&prefix),
                    index_chain,
                    cx,
                    items,
                );
            }
            _ => {}
        }
    }
}

/// A stream (device to host) as a stream function (API-31): opened with `RequestKind::StreamOpen` (the user's
/// handler named like the stream gets the arguments, `Ok` starts a new session), resumed with a cursor (counted and
/// reliable), closed with sideband `Close` (`close_<name>`). Counted and reliable streams keep their state in
/// what `stream_<name>()` returns (`ww_stream::tx::Counted` / `Reliable`), a read on them returns its
/// `StreamStatus`, and a reliable stream takes the host's `Ack` / `Resend`. Plain streams pass other sideband
/// commands to `sideband_<name>` as before.
#[allow(clippy::too_many_arguments)]
fn handle_stream_fn(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    mod_ident: &Ident,
    cx: &ApiServerCGContext<'_>,
    ident: &Ident,
    stream: StreamFnRef<'_>,
) -> TokenStream {
    let idx = index_chain.fun_argument_call();
    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let close_fn = Ident::new(&format!("close_{prefixed_ident}"), ident.span());
    let state_fn = Ident::new(&format!("stream_{prefixed_ident}"), ident.span());
    // the stream's state, from `stream_<name>()`
    let state = cx.call(ident, quote! { self.#state_fn(#idx) });
    let delivery = stream.delivery;
    if delivery == Delivery::Reliable && !cx.use_async {
        return quote! {
            compile_error!("reliable streams need an async server (use_async = true): their store is async");
        };
    }
    // reliable streams' methods that touch the store are async
    let store_await = maybe_quote(delivery == Delivery::Reliable, quote! { .await });
    // a batched stream's state is a `ww_stream::batch::Batched`: the delivery state is in its `stream` (API-41)
    let batched = stream.batched.is_some();
    let inner = maybe_quote(batched, quote! { .stream });
    let grant = maybe_quote(
        batched,
        quote! {
            let granted = #state.grant_max_age(max_age_us.as_ref().map(|a| a.0));
        },
    );
    let with_granted = maybe_quote(batched, quote! { .with_max_age_us(Some(granted)) });
    let max_age_arg = maybe_quote(batched, quote! { max_age_us, });
    // a counted stream's state is anything `ww_stream::codec::CountedState`: `Counted`, or `Compressed` that takes
    // a codec out of the host's offer (API-55)
    let codec_counted = delivery == Delivery::Counted && !batched;
    let codecs_arg = maybe_quote(codec_counted, quote! { codecs, });

    let args: Vec<ArgumentOwned> = stream.args.to_vec();
    let (args_des, args_list) = des_args(
        mod_ident,
        ident,
        &args,
        cx.no_alloc,
        item_err_seq("stream", "ARGS"),
    );
    let maybe_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args, .. }
    };
    let enforce_ty = match stream.return_ty {
        Some(ty) => cx.value_ty(api_bundle, ty, TyPos::Annotation),
        None => quote! { () },
    };
    let ser_output = cx.ser_root_value(quote! { output });
    let es_ser = item_err_seq("stream", "OPENED");
    let es_store = item_err_seq("stream", "STORE");
    let closed_opened = quote! {
        ww_client_server::stream::StreamOpened {
            session: wire_weaver::shrink_wrap::UVlq32(0),
            oldest: wire_weaver::shrink_wrap::UVlq64(0),
            live_from: wire_weaver::shrink_wrap::UVlq64(0),
            resume_from: None,
            capacity: None,
            max_age_us: None,
            codec: None,
            result: TailBytes(&[]),
        }
    };
    let new_session = match delivery {
        Delivery::Plain if batched => quote! {
            #grant
            #state.stream.open();
            #closed_opened #with_granted
        },
        Delivery::Plain => quote! {},
        Delivery::Counted if codec_counted => quote! {
            let session = self.new_stream_session();
            let (opened, codec) = ww_stream::codec::CountedState::open_offered(#state, session, codecs.as_ref());
            opened.to_wire(&[]).with_codec(codec)
        },
        Delivery::Counted => quote! {
            #grant
            let session = self.new_stream_session();
            #state #inner .open(session).to_wire(&[]) #with_granted
        },
        Delivery::Reliable => quote! {
            #grant
            let session = self.new_stream_session();
            match #state #inner .open(session).await {
                Ok(opened) => opened.to_wire(&[]) #with_granted,
                Err(_) => return Err(Error::new(#es_store, ErrorKind::StreamStoreFailed)),
            }
        },
    };
    let open_state = match (delivery, stream.return_ty.is_some()) {
        (Delivery::Plain, _) if !batched => quote! { let opened = #closed_opened; },
        (_, false) => quote! { let opened = { #new_session }; },
        // The handler's result may borrow from `self` (an error type with a lifetime, API-35): take the `Ok` value
        // out of it before the stream's state is touched, so the borrow ends on that path.
        (_, true) => quote! {
            let (opened, output): (_, #enforce_ty) = match output {
                Ok(value) => ({ #new_session }, Ok(value)),
                // the stream stays as it was
                Err(e) => (#closed_opened, Err(e)),
            };
        },
    };
    let ev_kind_builder = cx.prepare_event_kind_builder();
    let es_open = item_err_seq("stream", "OPEN");
    let open_call = cx.call(ident, quote! { self.#prefixed_ident(cx, #idx #args_list) });
    let open = quote! {
        RequestKind::StreamOpen { resume: None, #max_age_arg #codecs_arg #maybe_args } => {
            #args_des
            match #open_call {
                RpcResult::Ready(output) => {
                    let output: #enforce_ty = output;
                    #open_state
                    #ev_kind_builder
                    opened.ser_head(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                    #ser_output.map_err(|_| Error::response_ser_failed(#es_ser))?;
                    Ok(WrAction::WrittenOk(event_kind_builder))
                }
                // a stream function answers right away: the session starts with its result
                RpcResult::Deferred => Err(Error::not_supported(#es_open)),
                RpcResult::Unimplemented => Err(Error::unimplemented(#es_open)),
            }
        }
    };
    let es_resume = item_err_seq("stream", "RESUME");
    let resume = if delivery == Delivery::Plain {
        quote! {}
    } else {
        let ev_kind_builder = cx.prepare_event_kind_builder();
        let resume_result = if delivery == Delivery::Counted {
            quote! { r.map_err(|_| ErrorKind::StreamNoSession) }
        } else {
            quote! {
                r.map_err(|e| match e {
                    ww_stream::ResumeError::NoSession => ErrorKind::StreamNoSession,
                    ww_stream::ResumeError::Store(_) => ErrorKind::StreamStoreFailed,
                })
            }
        };
        if codec_counted {
            quote! {
                RequestKind::StreamOpen { resume: Some(cursor), codecs, .. } => {
                    let r = ww_stream::codec::CountedState::resume_offered(#state, *cursor, codecs.as_ref());
                    let (opened, codec) = #resume_result.map_err(|kind| Error::new(#es_resume, kind))?;
                    #ev_kind_builder
                    opened.to_wire(&[]).with_codec(codec).ser_head(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                    Ok(WrAction::WrittenOk(event_kind_builder))
                }
            }
        } else {
            quote! {
                RequestKind::StreamOpen { resume: Some(cursor), #max_age_arg .. } => {
                    #grant
                    let r = #state #inner .resume(*cursor)#store_await;
                    let opened = #resume_result.map_err(|kind| Error::new(#es_resume, kind))?;
                    #ev_kind_builder
                    opened.to_wire(&[]) #with_granted .ser_head(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                    Ok(WrAction::WrittenOk(event_kind_builder))
                }
            }
        }
    };
    let detach = match delivery {
        Delivery::Plain if batched => quote! { #state.stream.close(); },
        Delivery::Plain => quote! {},
        Delivery::Counted if codec_counted => {
            quote! { ww_stream::codec::CountedState::detach(#state); }
        }
        _ => quote! { #state #inner .detach(); },
    };
    let close_call = cx.call(ident, quote! { self.#close_fn(cx, #idx) });
    let close = quote! {
        RequestKind::StreamSideband { sideband: StreamSideband::Close } => {
            #detach
            #close_call;
            Ok(WrAction::Acknowledged)
        }
    };
    let es_sb = item_err_seq("stream", "SIDEBAND");
    let sideband = match delivery {
        Delivery::Plain => {
            let sideband_fn = Ident::new(&format!("sideband_{prefixed_ident}"), ident.span());
            let sideband_call = cx.call(ident, quote! { self.#sideband_fn(cx, #idx *sideband) });
            let ev_kind_builder = cx.prepare_event_kind_builder();
            quote! {
                RequestKind::StreamSideband { sideband } => {
                    // user fn returns Option<StreamSideband>
                    match #sideband_call {
                        Some(sideband) => {
                            #ev_kind_builder
                            wr.write(&RefVec::Slice { slice: &path }).map_err(|_| Error::new(#es_sb, ErrorKind::ResponseSerFailed))?;
                            wr.write(&sideband).map_err(|_| Error::new(#es_sb, ErrorKind::ResponseSerFailed))?;
                            Ok(WrAction::WrittenOk(event_kind_builder))
                        }
                        // nothing to send back, but the host may wait with the request's seq
                        None => Ok(WrAction::Acknowledged),
                    }
                }
            }
        }
        // acknowledgements of a counted stream mean nothing: nothing is kept
        Delivery::Counted => quote! {
            RequestKind::StreamSideband { sideband: StreamSideband::Ack { .. } } => {
                Ok(WrAction::Acknowledged)
            }
        },
        Delivery::Reliable => quote! {
            RequestKind::StreamSideband {
                sideband: sideband @ (StreamSideband::Ack { .. } | StreamSideband::Resend { .. })
            } => {
                #state #inner .on_host_sideband(sideband).await
                    .map_err(|_| Error::new(#es_store, ErrorKind::StreamStoreFailed))?;
                Ok(WrAction::Acknowledged)
            }
        },
    };
    let status_call = if codec_counted {
        quote! { ww_stream::codec::CountedState::status(#state) }
    } else {
        quote! { #state #inner .status() }
    };
    let status = if delivery == Delivery::Plain {
        quote! {}
    } else {
        let ev_kind_builder = cx.prepare_event_kind_builder();
        let ser_status = ser_value(
            cx.multi_req,
            quote! { status },
            item_err_seq("stream", "STATUS"),
        );
        quote! {
            RequestKind::Read => {
                let status: ww_client_server::stream::StreamStatus = #status_call;
                #ev_kind_builder
                #ser_status
                Ok(WrAction::WrittenOk(event_kind_builder))
            }
        }
    };
    let es = item_err_seq("", "KIND_NOT_TAKEN");
    quote! {
        match &request.kind {
            #open
            #resume
            #close
            #sideband
            #status
            _ => { Err(Error::not_supported(#es)) }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_stream(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    cx: &ApiServerCGContext<'_>,
    ident: &Ident,
    ty: &TypeOwned,
    is_up: bool,
    flow: Option<FlowControl>,
) -> TokenStream {
    let maybe_index_chain_call = index_chain.fun_argument_call();

    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let sideband_fn = Ident::new(
        format!("sideband_{}", prefixed_ident).as_str(),
        ident.span(),
    );
    let es = item_err_seq("sink", "SIDEBAND");
    let ev_kind_builder = cx.prepare_event_kind_builder();
    let reply_sideband = quote! {
        #ev_kind_builder
        wr.write(&RefVec::Slice { slice: &path }).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
        wr.write(&sideband).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
        Ok(WrAction::WrittenOk(event_kind_builder))
    };
    let sideband_call = cx.call(
        ident,
        quote! { self.#sideband_fn(cx, #maybe_index_chain_call *sideband) },
    );
    let call_user_sideband = quote! {
        // user fn returns Option<StreamSideband>
        let r = #sideband_call;
    };
    let match_user_sideband = quote! {
        match r {
            Some(sideband) => {
                #reply_sideband
            }
            None => {
                // nothing to send back, but the host is waiting with the request's seq
                Ok(WrAction::Acknowledged)
            }
        }
    };
    let handle_sideband = quote! {
        #call_user_sideband
        #match_user_sideband
    };
    // streams (device out) are stream functions, see handle_stream_fn
    debug_assert!(!is_up);
    let specific_ops = {
        // sink (device in)
        // bytes with a max length: sent as is like without one, checked here
        let is_bounded_bytes =
            matches!(ty, TypeOwned::MaxLen { .. }) && ty.is_byte_slice(api_bundle).unwrap();
        let max_len_check = match ty.max_len(api_bundle).unwrap() {
            Some(max_len) if ty.is_byte_slice(api_bundle).unwrap() => {
                let es = item_err_seq("sink", "DATA_TOO_LONG");
                let max_len = max_len as usize;
                quote! {
                    if data.0.len() > #max_len {
                        return Err(Error::new(#es, ErrorKind::ArgsDesFailed));
                    }
                }
            }
            _ => quote! {},
        };
        let other_des = || {
            let es = item_err_seq("sink", "DATA");
            let enforce_ty = cx.value_ty(api_bundle, ty, TyPos::Expr);
            let des_value = cx.des_value(&enforce_ty);
            let ts = quote! {
                let mut rd = BufReader::new(data);
                let value = #des_value(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
            };
            (ts, quote! { value })
        };
        let write = Ident::new(format!("write_{}", prefixed_ident).as_str(), ident.span());
        let (des_data, arg) = match ty {
            TypeOwned::Tuple(elements) => {
                if elements.is_empty() {
                    (quote! {}, quote! { () })
                } else {
                    other_des()
                }
            }
            TypeOwned::Vec(inner) => {
                if matches!(
                    inner.as_ref(),
                    TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U8))
                ) {
                    (quote! {}, quote! { data })
                } else {
                    other_des()
                }
            }
            TypeOwned::MaxLen { .. } if is_bounded_bytes => {
                (max_len_check.clone(), quote! { data })
            }
            _ => other_des(),
        };
        let write_call = cx.call(
            ident,
            quote! { self.#write(cx, #maybe_index_chain_call #arg) },
        );
        let ev_kind_builder = cx.prepare_event_kind_builder();
        match flow {
            None => quote! {
                RequestKind::Write { data } => {
                    #des_data
                    #write_call;
                    #ev_kind_builder
                    Ok(WrAction::WrittenOk(event_kind_builder)) // TODO: ?? do not send acknowledgements on stream writes
                }
                RequestKind::StreamSideband { sideband } => {
                    #handle_sideband
                }
            },
            Some(FlowControl::Credit) => {
                let credit_fn =
                    Ident::new(format!("credit_{}", prefixed_ident).as_str(), ident.span());
                let credit = quote! { self.#credit_fn(#maybe_index_chain_call) };
                let send_sideband = if cx.use_async {
                    quote! {
                        // a lost Ack is recovered by the host's next Query
                        let _ = wire_weaver::EventOut::send_with(&mut *cx, |scratch| {
                            ser_ok_event(scratch, 0, EventKind::StreamSideband { path: RefVec::Slice { slice: path }, sideband })
                                .map(|bytes| bytes.len())
                        }).await;
                    }
                } else {
                    quote! {
                        // a lost Ack is recovered by the host's next Query
                        let _ = wire_weaver::BlockingEventOut::send_with_blocking(&mut *cx, |scratch| {
                            ser_ok_event(scratch, 0, EventKind::StreamSideband { path: RefVec::Slice { slice: path }, sideband })
                                .map(|bytes| bytes.len())
                        });
                    }
                };
                // a too long write is refused before it takes a position, so it is never counted as accepted
                let (credit_max_len_check, credit_des_data) = if is_bounded_bytes {
                    (max_len_check, quote! {})
                } else {
                    (quote! {}, des_data)
                };
                // the host names a credit sink's session: an open without a cursor isn't one
                let es_open = item_err_seq("sink", "KIND_NOT_TAKEN");
                let es_opened = item_err_seq("sink", "OPENED");
                let ser_status = ser_value(
                    cx.multi_req,
                    quote! { status },
                    item_err_seq("sink", "STATUS"),
                );
                let ev_kind_builder_open = cx.prepare_event_kind_builder();
                let ev_kind_builder_status = cx.prepare_event_kind_builder();
                quote! {
                    // the host names the session (API-34): a new one at 0, or its own again after a reconnect
                    RequestKind::StreamOpen { resume: Some(cursor), .. } => {
                        let new_session = #credit.session() != Some(cursor.session.0);
                        let opened = #credit.open(*cursor);
                        if new_session {
                            // user code sees where a session (one logical write) starts, as `Open`; not on a resume
                            let sideband = StreamSideband::Open;
                            let sideband = &sideband;
                            #call_user_sideband
                            if let Some(sideband) = r {
                                #send_sideband
                            }
                        }
                        // the grant: the host applies it once it has the answer
                        if let Some(sideband) = #credit.take_ack() {
                            #send_sideband
                        }
                        #ev_kind_builder_open
                        // the sink's buffer: a resumed host gets no grant of all of it while something is buffered
                        let capacity = #credit.capacity();
                        opened.to_wire(&[]).with_capacity(Some(capacity)).ser_head(wr).map_err(|_| Error::response_ser_failed(#es_opened))?;
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                    RequestKind::StreamOpen { resume: None, .. } => {
                        Err(Error::not_supported(#es_open))
                    }
                    kind @ (RequestKind::SinkWrite { .. } | RequestKind::SinkWriteUnits { .. }) => {
                        let Some((units, pos, data)) = kind.as_sink_write() else {
                            return Err(Error::not_supported(#es_open));
                        };
                        let data = &TailBytes(data);
                        #credit_max_len_check
                        let verdict = #credit.on_write(pos, units, data.0.len() as u32);
                        match verdict {
                            ww_stream::sink::SinkVerdict::Accept => {
                                #credit_des_data
                                #write_call;
                                if let Some(sideband) = #credit.take_ack() {
                                    #send_sideband
                                }
                            }
                            ww_stream::sink::SinkVerdict::Gap(Some(sideband))
                            | ww_stream::sink::SinkVerdict::NoSession(Some(sideband)) => {
                                #send_sideband
                            }
                            ww_stream::sink::SinkVerdict::Duplicate
                            | ww_stream::sink::SinkVerdict::Gap(None)
                            | ww_stream::sink::SinkVerdict::NoSession(None)
                            | ww_stream::sink::SinkVerdict::Invalid => {}
                        }
                        #ev_kind_builder
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                    RequestKind::StreamSideband { sideband: StreamSideband::Query { sent, .. } } => {
                        // answered here, the user's sideband handler doesn't see it
                        match #credit.on_query(sent.0) {
                            Some(sideband) => {
                                #reply_sideband
                            }
                            None => Ok(WrAction::Acknowledged),
                        }
                    }
                    RequestKind::StreamSideband { sideband } => {
                        if matches!(sideband, StreamSideband::Close) {
                            #credit.close();
                        }
                        #call_user_sideband
                        // a re-grant if the handler freed something
                        if let Some(sideband) = #credit.take_ack() {
                            #send_sideband
                        }
                        #match_user_sideband
                    }
                    RequestKind::Read => {
                        let status: ww_client_server::stream::StreamStatus = #credit.status();
                        #ev_kind_builder_status
                        #ser_status
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                }
            }
        }
    };
    let es = item_err_seq("", "KIND_NOT_TAKEN");
    quote! {
        match &request.kind {
            #specific_ops
            _ => { Err(Error::new(#es, ErrorKind::Unimplemented)) }
        }
    }
}

fn args_structs_recursive(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    no_alloc: bool,
    seen: &mut Vec<Ident>,
    ts: &mut TokenStream,
) {
    let mod_name = util::mod_name(api_level, api_bundle);
    if !seen.contains(&mod_name) {
        let args_structs = api_common::args_structs(api_bundle, api_level, no_alloc);
        ts.extend(quote! {
            mod #mod_name {
                use wire_weaver::shrink_wrap::prelude::*;
                #args_structs
            }
        });
        seen.push(mod_name);
    }
    for item in &api_level.items {
        let ApiItemKindOwned::Trait { .. } = &item.kind else {
            continue;
        };
        let level = item.get_as_level(api_bundle).unwrap();
        args_structs_recursive(api_bundle, level, no_alloc, seen, ts);
    }
}

/// `<METHOD>_REQUEST_MAX_LEN` and `<METHOD>_RESPONSE_MAX_LEN` of every method (SW-19, first step of API-18), named
/// like its handler: worst-case lengths of a call request with an absolute path and of the answer with the value,
/// `None` when an argument or the return type has no bound.
fn max_len_consts_recursive(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    ident_prefix: Option<&String>,
    path: &[u32],
    no_alloc: bool,
    ts: &mut TokenStream,
) {
    let mod_name = util::mod_name(api_level, api_bundle);
    for item in &api_level.items {
        let mut path = path.to_vec();
        path.push(item.id.0);
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            // any index
            path.push(u32::MAX);
        }
        match &item.kind {
            ApiItemKindOwned::Method {
                args, return_ty, ..
            } => {
                let ident = Ident::new(&item.ident, Span::call_site());
                let name = add_prefix(ident_prefix, &ident)
                    .to_string()
                    .to_case(Case::UpperSnake);
                let args_max_size = if args.is_empty() {
                    quote! { Some(wire_weaver::shrink_wrap::MaxSize::ZERO) }
                } else {
                    let args_struct = api_common::args_struct_ident(&item.ident, no_alloc);
                    let is_lifetime = args
                        .iter()
                        .any(|arg| arg.ty.is_lifetime(api_bundle).unwrap());
                    let lifetime = maybe_quote(no_alloc && is_lifetime, quote! { <'static> });
                    quote! { <#mod_name::#args_struct #lifetime as wire_weaver::shrink_wrap::SerializeShrinkWrap>::MAX_SIZE }
                };
                let output_max_size = match return_ty {
                    Some(ty) => {
                        let ty = ty_def(api_bundle, ty, !no_alloc, TyPos::Arg).unwrap();
                        quote! { <#ty as wire_weaver::shrink_wrap::SerializeShrinkWrap>::MAX_SIZE }
                    }
                    None => quote! { Some(wire_weaver::shrink_wrap::MaxSize::ZERO) },
                };
                let request = Ident::new(&format!("{name}_REQUEST_MAX_LEN"), Span::call_site());
                let response = Ident::new(&format!("{name}_RESPONSE_MAX_LEN"), Span::call_site());
                let request_doc = format!(
                    " Worst-case length of a `{}` call request with an absolute path, `None` if an argument \
                    has no bound (see `shrink_wrap::MaxLen`).",
                    item.ident
                );
                let response_doc = format!(
                    " Worst-case length of the answer to a `{}` call with its value, `None` if the return \
                    type has no bound.",
                    item.ident
                );
                ts.extend(quote! {
                    #[doc = #request_doc]
                    pub const #request: Option<usize> =
                        ww_client_server::max_len::call_request(&[#(#path),*], #args_max_size);
                    #[doc = #response_doc]
                    pub const #response: Option<usize> =
                        ww_client_server::max_len::value_event(#output_max_size);
                });
            }
            ApiItemKindOwned::Trait { .. } => {
                let level = item.get_as_level(api_bundle).unwrap();
                let prefix = match ident_prefix {
                    Some(prefix) => format!("{prefix}_{}", item.ident),
                    None => item.ident.clone(),
                };
                max_len_consts_recursive(api_bundle, level, Some(&prefix), &path, no_alloc, ts);
            }
            _ => {}
        }
    }
}

/// A deferred method's return helper: its error sites are the same for every method (SRV-22).
fn ser_method_output(
    return_type: &Option<TypeOwned>,
    seq_path: TokenStream,
    no_alloc: bool,
) -> TokenStream {
    if let Some(_ty) = return_type {
        let es = fixed_err_seq("DEFERRED_OUTPUT_SER");
        let ser = ser_root_value(no_alloc, quote! { output });
        let ser_output = quote! {
            {
                let wr = &mut wr;
                #ser.map_err(|_| Error::response_ser_failed(#es))?;
            }
        };

        let es0 = fixed_err_seq("DEFERRED_OUTPUT_FINISH");
        let es1 = fixed_err_seq("DEFERRED_EVENT_SER");
        let es2 = fixed_err_seq("DEFERRED_EVENT_FINISH");
        quote! {
            let mut wr = BufWriter::new(scratch_args);
            #ser_output
            let output_bytes = wr.finish_and_take().map_err(|_| Error::response_ser_failed(#es0))?;

            let mut event_wr = BufWriter::new(scratch_event);
            let event = Event {
                seq: wire_weaver::shrink_wrap::UVlq32(#seq_path),
                result: Ok(EventKind::Value {
                    data: wire_weaver::shrink_wrap::tail_bytes::TailBytes(output_bytes)
                })
            };
            event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::response_ser_failed(#es1))?;
            event_wr.finish_and_take().map_err(|_| Error::response_ser_failed(#es2))
        }
    } else {
        let es = fixed_err_seq("DEFERRED_EVENT_SER");
        quote! {
            let _ = scratch_args;
            ww_client_server::util::ser_unit_return_event(scratch_event, #seq_path).map_err(|_| Error::response_ser_failed(#es))
        }
    }
}

fn des_args(
    mod_ident: &Ident,
    method_ident: &Ident,
    args: &[ArgumentOwned],
    no_alloc: bool,
    es: TokenStream,
) -> (TokenStream, TokenStream) {
    let args_struct_ident = api_common::args_struct_ident(&method_ident.to_string(), no_alloc);
    if args.is_empty() {
        (quote! {}, quote! {})
    } else {
        let des = if no_alloc {
            quote! { des_shrink_wrap }
        } else {
            quote! { des_shrink_wrap_owned }
        };
        let args_des = quote! {
            let args = args.as_slice();
            let mut rd = BufReader::new(args);
            // TODO: Log _e ?
            let args = #mod_ident::#args_struct_ident::#des(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
        };
        let idents = args
            .iter()
            .map(|arg| Ident::new(&arg.ident, Span::call_site()));
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}

fn deferred_method_return_ser_methods(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    ident_prefix: Option<&String>,
    no_alloc: bool,
    method_model: &MethodModel,
) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &api_level.items {
        // nested levels use the handler's name: prefix chain + method, also for `method_model` patterns
        let prefixed = match ident_prefix {
            Some(prefix) => format!("{prefix}_{}", item.ident),
            None => item.ident.clone(),
        };
        if matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
            let level = item.get_as_level(api_bundle).unwrap();
            ts.extend(deferred_method_return_ser_methods(
                api_bundle,
                level,
                Some(&prefixed),
                no_alloc,
                method_model,
            ));
            continue;
        }
        let ApiItemKindOwned::Method { return_ty, .. } = &item.kind else {
            continue;
        };
        if method_model.pick(&prefixed) != Some(MethodModelKind::Deferred) {
            continue;
        }
        let fn_name = Ident::new(
            format!("{prefixed}_ser_return_event").as_str(),
            Span::call_site(),
        );
        let ser_output_or_unit = ser_method_output(return_ty, quote! { seq }, no_alloc);
        let maybe_output = match return_ty {
            Some(ty) => {
                let ty = ty_def(api_bundle, ty, !no_alloc, TyPos::Annotation).unwrap();
                quote! { , output: #ty }
            }
            None => quote! {},
        };
        let ser = ser_root_value(no_alloc, quote! { output });
        let ser_output = maybe_quote(
            return_ty.is_some(),
            quote! {
                {
                    let wr = &mut wr;
                    #ser?;
                }
            },
        );
        let ser_event = quote! {
            |scratch: &mut [u8]| -> Result<usize, ShrinkWrapError> {
                let mut wr = BufWriter::new(scratch);
                let event_builder = EventBuilder::new(seq, &mut wr)?;
                let event_kind_builder = EventKindBuilder::new(&mut wr)?;
                #ser_output
                event_kind_builder.finish_with_kind(EventKindDiscriminants::Value, &mut wr);
                event_builder.finish(true, &mut wr);
                Ok(wr.finish()?.len())
            }
        };
        let send_fn_name = Ident::new(
            format!("{prefixed}_send_return").as_str(),
            Span::call_site(),
        );
        let send_blocking_fn_name = Ident::new(
            format!("{prefixed}_send_return_blocking").as_str(),
            Span::call_site(),
        );
        ts.extend(quote! {
            pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u32 #maybe_output) -> Result<&'i [u8], Error<'static>> {
                #ser_output_or_unit
            }

            /// Send the return value of a deferred call, `seq` is from `cx.reply_to()` of the original call.
            pub async fn #send_fn_name<'i>(out: &mut impl wire_weaver::EventOut, seq: u32 #maybe_output) -> Result<(), wire_weaver::SendError> {
                wire_weaver::EventOut::send_with(out, #ser_event).await
            }

            /// Send the return value of a deferred call, `seq` is from `cx.reply_to()` of the original call.
            pub fn #send_blocking_fn_name<'i>(out: &mut impl wire_weaver::BlockingEventOut, seq: u32 #maybe_output) -> Result<(), wire_weaver::SendError> {
                wire_weaver::BlockingEventOut::send_with_blocking(out, #ser_event)
            }
        });
    }
    ts
}

/// See [ApiServerCGContext::ser_root_value].
fn ser_root_value(no_alloc: bool, value: TokenStream) -> TokenStream {
    if no_alloc {
        quote! { #value.ser_shrink_wrap(wr) }
    } else {
        quote! { wr.ser_owned(&#value) }
    }
}

/// A value of the framework (valid indices, a sink's status), borrowed in every server.
fn ser_value(multi_req: bool, ident: TokenStream, es: TokenStream) -> TokenStream {
    if multi_req {
        quote! {
            if in_multi {
                wr.write(& #ident).map_err(|_| Error::response_ser_failed(#es))?;
            } else {
                #ident.ser_shrink_wrap(wr).map_err(|_| Error::response_ser_failed(#es))?;
            }
        }
    } else {
        quote! {
            #ident.ser_shrink_wrap(wr).map_err(|_| Error::response_ser_failed(#es))?;
        }
    }
}
