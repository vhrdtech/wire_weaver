//! # Implementation details:
//! * Server's index chain contains only array indices on the way to a resource
use crate::codegen::index_chain::IndexChain;
use crate::codegen::omit::{Omission, Omissions, Omit};
use crate::codegen::server::observe::{Observe, mark_changed_ident};
use crate::codegen::server::stream::stream_ser_methods_recursive;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{ErrorSeq, add_prefix, maybe_quote, maybe_quote_cl};
use crate::codegen::{api_common, util};
use crate::const_model::{ConstModel, ConstModelKind};
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
    /// Set to true to expect all user handlers be async fn's.
    pub use_async: bool,
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
    /// Set to true to expect all user handlers be async fn's.
    pub use_async: bool,
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
}

impl From<GenServerConfig> for GenServerConfigRaw {
    fn from(config: GenServerConfig) -> Self {
        Self {
            no_alloc: config.no_alloc,
            use_async: config.use_async,
            method_model: config.method_model,
            property_model: config.property_model,
            const_model: config.const_model,
            server_struct_path: super::util::str_to_path(&config.server_struct_path),
            introspect_mode: config.introspect_mode,
            multi_req: config.multi_req,
            medium: config.medium.as_deref().map(super::util::str_to_path),
            omit: config.omit,
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
    let additional_use = maybe_quote(
        config.no_alloc,
        quote! { #[allow(unused_imports)] use wire_weaver::shrink_wrap::{RefVec, RefVecIter}; },
    );
    let maybe_async = maybe_quote(config.use_async, quote! { async });
    let maybe_await = maybe_quote(config.use_async, quote! { .await });
    let mut error_seq = ErrorSeq::default();
    let api_level = &api_bundle.root;
    let deferred_return_methods = deferred_method_return_ser_methods(
        api_bundle,
        api_level,
        config.no_alloc,
        &config.method_model,
        &mut error_seq,
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
    let observe = Observe::collect(api_bundle, &omissions, &config.property_model);
    let cx = ApiServerCGContext {
        observable: observe.observable(),
        ident_prefix: None,
        no_alloc: config.no_alloc,
        use_async: config.use_async,
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
    let (handle_introspect, api_signature) = super::server::introspect::introspect(
        api_bundle,
        &omissions,
        introspect_enabled,
        include_docs,
        cx.use_async,
        &mut error_seq,
    );
    let process_request_inner = process_request_inner_recursive(
        "root".into(),
        &[],
        &omissions,
        api_bundle,
        api_level,
        IndexChain::new(),
        crate_name,
        &cx,
        &mut error_seq,
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
    let poll_streams = poll_streams(api_bundle, cx.use_async);
    let observe_items = observe.items(api_bundle, config.no_alloc);
    let observe_methods = observe.methods(api_bundle, cx.use_async, &medium_ty, &mut error_seq);
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
    let (es1, es2) = (error_seq.next(), error_seq.next());
    let server_struct_path = config.server_struct_path;
    let maybe_in_multi_false = maybe_quote_cl(cx.multi_req, || quote! { false, });
    let maybe_multi_req = maybe_quote_cl(cx.multi_req, || {
        multi_req_handlers(&maybe_await, &mut error_seq)
    });
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
            Error as ShrinkWrapError, nib32::UNib32, ElementSize,
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
                    // TODO(API-20): compact ids of traits are not in the API model yet, no trait has one assigned
                    PathKind::GlobalCompact { .. } => {
                        wr.write(&Error::new(#es1, ErrorKind::TraitNotFound))?;
                        event_builder.finish(false, &mut wr);
                        return wr.finish_and_take();
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
                        match self.process_root(path, &mut iter, &request, &mut wr, #maybe_in_multi_false &mut cx)#maybe_await {
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

        #stream_send_methods

        #global_trait_path

        #consts_trait
    }
}

/// Reliable streams of the API: name of their handlers (with the trait chain prefix) and absolute path, or an
/// error message for one that can't be polled.
fn reliable_streams_recursive(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    prefix: Option<&String>,
    path: &mut Vec<u32>,
    under_array: bool,
    out: &mut Vec<Result<(Ident, Vec<u32>), String>>,
) {
    for item in &level.items {
        let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
        path.push(item.id.0);
        match &item.kind {
            ApiItemKindOwned::StreamFn {
                delivery: Delivery::Reliable,
                ..
            } => {
                if under_array || is_array {
                    out.push(Err(format!(
                        "reliable stream {} in a resource array: poll_streams can't send it yet (API-31)",
                        item.ident
                    )));
                } else {
                    let ident = Ident::new(&item.ident, Span::call_site());
                    out.push(Ok((add_prefix(prefix, &ident), path.clone())));
                }
            }
            ApiItemKindOwned::Trait { .. } => {
                let child = item.get_as_level(api_bundle).unwrap();
                let prefix = match prefix {
                    Some(prefix) => format!("{prefix}_{}", item.ident),
                    None => item.ident.clone(),
                };
                reliable_streams_recursive(
                    api_bundle,
                    child,
                    Some(&prefix),
                    path,
                    under_array || is_array,
                    out,
                );
            }
            _ => {}
        }
        path.pop();
    }
}

/// `poll_streams` on the server: sends what reliable streams have in their stores, as the send scheduler of
/// `ww_stream` picks (priority, live before backfill, round-robin), and their queries. Not generated without
/// reliable streams.
fn poll_streams(api_bundle: &ApiBundleOwned, use_async: bool) -> TokenStream {
    let mut streams = vec![];
    reliable_streams_recursive(
        api_bundle,
        &api_bundle.root,
        None,
        &mut vec![],
        false,
        &mut streams,
    );
    if streams.is_empty() || !use_async {
        // a blocking server with reliable streams already got a compile error from its dispatcher
        return quote! {};
    }
    let mut errors = TokenStream::new();
    let mut ok = vec![];
    for s in streams {
        match s {
            Ok(s) => ok.push(s),
            Err(e) => errors.extend(quote! { compile_error!(#e); }),
        }
    }
    let n = ok.len();
    let state_fns: Vec<Ident> = ok
        .iter()
        .map(|(ident, _)| Ident::new(&format!("stream_{ident}"), ident.span()))
        .collect();
    let paths: Vec<TokenStream> = ok
        .iter()
        .map(|(_, path)| quote! { [#(UNib32(#path)),*] })
        .collect();
    let idx = 0..n;
    quote! {
        #errors

        /// Send what the reliable streams have, between requests: events by priority, the live run before the
        /// backfill run, round-robin within a priority, up to `budget_bytes` of items (at least one event), and a
        /// `Query` for a stream whose host was silent. `buf` must hold the biggest item of any reliable stream;
        /// `now_ms` is any millisecond clock. A stream whose store fails is skipped this turn.
        pub async fn poll_streams(
            &mut self,
            out: &mut impl wire_weaver::EventOut,
            now_ms: u64,
            buf: &mut [u8],
            budget_bytes: usize,
        ) -> Result<(), wire_weaver::SendError> {
            #(
                if let Some(sideband) = self.#state_fns().await.poll_query(now_ms) {
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
                let ready = [#(self.#state_fns().await.ready()),*];
                let Some((i, run)) = sched.pick(&ready) else {
                    break;
                };
                let sent = match i {
                    #(
                        #idx => {
                            let ev = match self.#state_fns().await.next_event(run, buf).await {
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
            Ok(())
        }
    }
}

/// Absolute path of every global trait in the API tree, for requests with a global trait path (API-20).
///
/// Traits under a resource array are left out (a global path can't select an index), and when a trait is implemented
/// several times, the first one in depth first, id order is used. A trait matches when the requested crate version is
/// protocol compatible with the one this API was built with (same major, and same minor before 1.0).
fn global_trait_path(api_bundle: &ApiBundleOwned) -> TokenStream {
    let mut traits = vec![];
    global_traits_recursive(api_bundle, &api_bundle.root, &mut vec![], &mut traits);
    let arms = traits.iter().map(|(path, crate_version, trait_name)| {
        let crate_id = crate_version.crate_id.as_str();
        let (major, minor) = (crate_version.version.major.0, crate_version.version.minor.0);
        let version_check = if major >= 1 {
            quote! { gid.version.major.0 == #major }
        } else {
            quote! { gid.version.major.0 == 0 && gid.version.minor.0 == #minor }
        };
        quote! {
            (#crate_id, #trait_name) if #version_check => Some(&[#(UNib32(#path)),*]),
        }
    });
    quote! {
        /// Absolute path of a global trait in this API, None if it's not implemented in a compatible version.
        fn ww_global_trait_path(gid: &wire_weaver::ww_version::FullVersion<'_>, trait_name: &str) -> Option<&'static [UNib32]> {
            match (gid.crate_id, trait_name) {
                #(#arms)*
                _ => None,
            }
        }
    }
}

fn global_traits_recursive(
    api_bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    path: &mut Vec<u32>,
    traits: &mut Vec<(Vec<u32>, FullVersionOwned, String)>,
) {
    let crate_version = api_bundle.crate_version(level.crate_idx.0).unwrap();
    if !traits
        .iter()
        .any(|(_, v, name)| v.crate_id == crate_version.crate_id && *name == level.trait_name)
    {
        traits.push((
            path.clone(),
            crate_version.clone(),
            level.trait_name.clone(),
        ));
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
fn multi_req_handlers(maybe_await: &TokenStream, error_seq: &mut ErrorSeq) -> TokenStream {
    let (es_depth, es_args, es_deferred) = (error_seq.next(), error_seq.next(), error_seq.next());
    let es_overflow = error_seq.next();
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
    no_alloc: bool,
    use_async: bool,
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
    }
}

#[allow(clippy::too_many_arguments)]
fn process_request_inner_recursive(
    level_name_chain: String,
    item_path: &[String],
    omissions: &Omissions,
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    crate_name: &str,
    cx: &ApiServerCGContext<'_>,
    error_seq: &mut ErrorSeq,
    introspect: Option<TokenStream>,
) -> TokenStream {
    let maybe_async = maybe_quote(cx.use_async, quote! { async });
    let level_matchers = level_matchers(
        api_bundle,
        api_level,
        &omissions.level(item_path),
        &level_name_chain,
        index_chain,
        crate_name,
        cx,
        error_seq,
    );
    let maybe_index_chain_def = index_chain.fun_argument_def();

    let process_fn_name = Ident::new(
        format!("process_{}", level_name_chain).as_str(),
        Span::call_site(),
    );
    let es = error_seq.next();
    let maybe_in_multi = maybe_quote_cl(cx.multi_req, || quote! { in_multi: bool, });
    let context_ty = cx.context_ty();
    let mut ts = quote! {
        // index chain and in_multi make 8 below an array
        #[allow(clippy::too_many_arguments)]
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
            omissions,
            api_bundle,
            level,
            index_chain,
            level.crate_name(api_bundle).unwrap(),
            &cx,
            error_seq,
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
    error_seq: &mut ErrorSeq,
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
    let es = error_seq.next();
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
        let es = error_seq.next();
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
            error_seq,
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
                error_seq,
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
            let validate_index = quote! {
                if !self.#valid_indices(#maybe_index_chain_arg).contains(index.0) {
                    return Err(Error::new(#es, ErrorKind::BadIndex));
                }
            };
            let es = error_seq.next();
            let ser_indices = ser_value(cx.multi_req, quote! { indices }, error_seq);
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
    error_seq: &mut ErrorSeq,
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
            error_seq,
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
            error_seq,
        ),
        ApiItemKindOwned::Stream { is_up: true, .. } | ApiItemKindOwned::StreamFn { .. } => {
            handle_stream_fn(
                api_bundle,
                index_chain,
                &mod_ident,
                cx,
                &ident,
                api_item.kind.as_stream_fn().expect("a stream"),
                error_seq,
            )
        }
        ApiItemKindOwned::Stream { ty, is_up: false } => handle_stream(
            api_bundle,
            index_chain,
            cx,
            &ident,
            ty,
            false,
            None,
            error_seq,
        ),
        ApiItemKindOwned::FlowSink { ty, flow } => handle_stream(
            api_bundle,
            index_chain,
            cx,
            &ident,
            ty,
            false,
            Some(*flow),
            error_seq,
        ),
        ApiItemKindOwned::Trait { .. } => {
            let process_fn_name = Ident::new(
                format!("process_{level_name_chain}_{}", api_item.ident).as_str(),
                Span::call_site(),
            );
            let maybe_await = maybe_quote(cx.use_async, quote! { .await });
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

    fn prepare_event_kind_builder(&self) -> TokenStream {
        if self.multi_req {
            quote! {
                let event_kind_builder = if in_multi {
                    None
                } else {
                    Some(EventKindBuilder::new(wr).map_err(|_| Error::new(0, ErrorKind::ResponseSerFailed))?)
                };
            }
        } else {
            quote! {
                let event_kind_builder = EventKindBuilder::new(wr).map_err(|_| Error::new(0, ErrorKind::ResponseSerFailed))?;
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
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_enforce_ty = if let Some(ty) = return_type {
        let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Annotation).unwrap();
        quote! {
            let output: #enforce_ty = output;
        }
    } else {
        quote! {}
    };
    let maybe_index_chain_arg = index_chain.fun_argument_call();

    let (args_des, args_list) = des_args(mod_ident, ident, args, cx.no_alloc, error_seq);
    let maybe_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args }
    };

    // a deferred method answers later with the request's seq, a multi request can't carry that yet: refused before
    // the handler runs (a handler that defers anyway is answered with the same error in multi_req_handlers)
    let maybe_refuse_in_multi = if cx.multi_req
        && cx.method_model.pick(&ident.to_string()) == Some(MethodModelKind::Deferred)
    {
        let es = error_seq.next();
        quote! {
            // TODO(CLT-15): deferred methods in a multi request
            if in_multi {
                return Err(Error::not_supported(#es));
            }
        }
    } else {
        quote! {}
    };
    let ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let es = error_seq.next();
    let ser_output = ser_value(cx.multi_req, quote! { output }, error_seq);
    let ev_kind_builder = cx.prepare_event_kind_builder();
    quote! {
        match &request.kind {
            RequestKind::Call { #maybe_args } => {
                #maybe_refuse_in_multi
                #args_des
                match self.#ident(cx, #maybe_index_chain_arg #args_list)#maybe_await {
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
                Err(Error::not_supported(#es))
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
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    if matches!(access, PropertyAccess::Const) {
        return handle_const(api_bundle, index_chain, cx, ident, ty, error_seq);
    }
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_index_chain_arg = index_chain.fun_argument_call();
    let maybe_index_chain_indices = index_chain.array_indices();
    let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Expr).unwrap();
    let es = error_seq.next();
    let ser_user_err = user_error_ty
        .as_ref()
        .map(|ty| ty_def(api_bundle, ty, false, TyPos::Annotation).unwrap())
        .map(|enforce_user_err_ty| {
            quote! {
                let user_err: #enforce_user_err_ty = user_err;
                let err_builder = ErrorBuilder::new(#es, wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                wr.write(&user_err).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
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
                    let es = error_seq.next();
                    let ev_kind_builder = cx.prepare_event_kind_builder();
                    quote! {
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #enforce_ty::des_shrink_wrap(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        match self.#set_property(cx, #maybe_index_chain_arg value)#maybe_await {
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
                                Err(Error::unimplemented(#es))
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
                    let es = error_seq.next();
                    let ev_kind_builder = cx.prepare_event_kind_builder();
                    quote! {
                        #ev_kind_builder
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #enforce_ty::des_shrink_wrap(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        if self.#prefixed_ident #maybe_index_chain_indices != value {
                            self.#prefixed_ident #maybe_index_chain_indices = value;
                            #mark_changed
                            self.#changed_property(cx, #maybe_index_chain_arg)#maybe_await;
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
            let es = error_seq.next();
            let ser_value = ser_value(cx.multi_req, quote! { value }, error_seq);
            let ev_kind_builder = cx.prepare_event_kind_builder();
            let get_and_ser_property = match property_model_pick {
                PropertyModelKind::GetSet => {
                    let get_property = Ident::new(
                        format!("get_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    quote! {
                        match self.#get_property(cx, #maybe_index_chain_arg)#maybe_await {
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

    let es = error_seq.next();
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
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Expr).unwrap();
    let ser_value = ser_value(cx.multi_req, quote! { value }, error_seq);
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
    let es = error_seq.next();
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
    prefix: Option<&String>,
    index_chain: IndexChain,
    cx: &ApiServerCGContext,
    items: &mut TokenStream,
) {
    let omitted = omissions.level(item_path);
    for item in &level.items {
        if omitted.contains_key(&item.id.0) {
            continue;
        }
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
/// `StreamStatus`, and a reliable stream takes the host's `Ack` / `ResendFrom`. Plain streams pass other sideband
/// commands to `sideband_<name>` as before.
#[allow(clippy::too_many_arguments)]
fn handle_stream_fn(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    mod_ident: &Ident,
    cx: &ApiServerCGContext<'_>,
    ident: &Ident,
    stream: StreamFnRef<'_>,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let idx = index_chain.fun_argument_call();
    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let close_fn = Ident::new(&format!("close_{prefixed_ident}"), ident.span());
    let state_fn = Ident::new(&format!("stream_{prefixed_ident}"), ident.span());
    let delivery = stream.delivery;
    if delivery == Delivery::Reliable && !cx.use_async {
        return quote! {
            compile_error!("reliable streams need an async server (use_async = true): their store is async");
        };
    }
    // reliable streams' methods that touch the store are async
    let store_await = maybe_quote(delivery == Delivery::Reliable, quote! { .await });

    let args: Vec<ArgumentOwned> = stream.args.to_vec();
    let (args_des, args_list) = des_args(mod_ident, ident, &args, cx.no_alloc, error_seq);
    let maybe_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args, .. }
    };
    let enforce_ty = match stream.return_ty {
        Some(ty) => ty_def(api_bundle, ty, false, TyPos::Annotation).unwrap(),
        None => quote! { () },
    };
    let es_ser = error_seq.next();
    let es_store = error_seq.next();
    let closed_opened = quote! {
        ww_client_server::stream::StreamOpened {
            session: wire_weaver::shrink_wrap::UVlq32(0),
            oldest: wire_weaver::shrink_wrap::UVlq64(0),
            live_from: wire_weaver::shrink_wrap::UVlq64(0),
            resume_from: None,
            result: TailBytes(&[]),
        }
    };
    let new_session = match delivery {
        Delivery::Plain => quote! {},
        Delivery::Counted => quote! {
            let session = self.new_stream_session();
            self.#state_fn(#idx)#maybe_await.open(session).to_wire(&[])
        },
        Delivery::Reliable => quote! {
            let session = self.new_stream_session();
            match self.#state_fn(#idx).await.open(session).await {
                Ok(opened) => opened.to_wire(&[]),
                Err(_) => return Err(Error::new(#es_store, ErrorKind::StreamStoreFailed)),
            }
        },
    };
    let open_state = match (delivery, stream.return_ty.is_some()) {
        (Delivery::Plain, _) => quote! { let opened = #closed_opened; },
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
    let es_open = error_seq.next();
    let open = quote! {
        RequestKind::StreamOpen { resume: None, #maybe_args } => {
            #args_des
            match self.#prefixed_ident(cx, #idx #args_list)#maybe_await {
                RpcResult::Ready(output) => {
                    let output: #enforce_ty = output;
                    #open_state
                    #ev_kind_builder
                    opened.ser_head(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                    output.ser_shrink_wrap(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                    Ok(WrAction::WrittenOk(event_kind_builder))
                }
                // a stream function answers right away: the session starts with its result
                RpcResult::Deferred => Err(Error::not_supported(#es_open)),
                RpcResult::Unimplemented => Err(Error::unimplemented(#es_open)),
            }
        }
    };
    let es_resume = error_seq.next();
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
        quote! {
            RequestKind::StreamOpen { resume: Some(cursor), .. } => {
                let r = self.#state_fn(#idx)#maybe_await.resume(*cursor)#store_await;
                let opened = #resume_result.map_err(|kind| Error::new(#es_resume, kind))?;
                #ev_kind_builder
                opened.to_wire(&[]).ser_head(wr).map_err(|_| Error::response_ser_failed(#es_ser))?;
                Ok(WrAction::WrittenOk(event_kind_builder))
            }
        }
    };
    let detach = maybe_quote(
        delivery != Delivery::Plain,
        quote! { self.#state_fn(#idx)#maybe_await.detach(); },
    );
    let close = quote! {
        RequestKind::StreamSideband { sideband: StreamSideband::Close } => {
            #detach
            self.#close_fn(cx, #idx)#maybe_await;
            Ok(WrAction::Acknowledged)
        }
    };
    let es_sb = error_seq.next();
    let sideband = match delivery {
        Delivery::Plain => {
            let sideband_fn = Ident::new(&format!("sideband_{prefixed_ident}"), ident.span());
            let ev_kind_builder = cx.prepare_event_kind_builder();
            quote! {
                RequestKind::StreamSideband { sideband } => {
                    // user fn returns Option<StreamSideband>
                    match self.#sideband_fn(cx, #idx *sideband)#maybe_await {
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
                sideband: sideband @ (StreamSideband::Ack { .. } | StreamSideband::ResendFrom { .. })
            } => {
                self.#state_fn(#idx).await.on_host_sideband(sideband).await
                    .map_err(|_| Error::new(#es_store, ErrorKind::StreamStoreFailed))?;
                Ok(WrAction::Acknowledged)
            }
        },
    };
    let status = if delivery == Delivery::Plain {
        quote! {}
    } else {
        let ev_kind_builder = cx.prepare_event_kind_builder();
        let ser_status = ser_value(cx.multi_req, quote! { status }, error_seq);
        quote! {
            RequestKind::Read => {
                let status: ww_client_server::stream::StreamStatus = self.#state_fn(#idx)#maybe_await.status();
                #ev_kind_builder
                #ser_status
                Ok(WrAction::WrittenOk(event_kind_builder))
            }
        }
    };
    let es = error_seq.next();
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
    err_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_index_chain_call = index_chain.fun_argument_call();
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });

    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let sideband_fn = Ident::new(
        format!("sideband_{}", prefixed_ident).as_str(),
        ident.span(),
    );
    let es = err_seq.next();
    let ev_kind_builder = cx.prepare_event_kind_builder();
    let reply_sideband = quote! {
        #ev_kind_builder
        wr.write(&RefVec::Slice { slice: &path }).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
        wr.write(&sideband).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
        Ok(WrAction::WrittenOk(event_kind_builder))
    };
    let call_user_sideband = quote! {
        // user fn returns Option<StreamSideband>
        let r = self.#sideband_fn(cx, #maybe_index_chain_call *sideband)#maybe_await;
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
                let es = err_seq.next();
                let max_len = max_len as usize;
                quote! {
                    if data.0.len() > #max_len {
                        return Err(Error::new(#es, ErrorKind::ArgsDesFailed));
                    }
                }
            }
            _ => quote! {},
        };
        let mut other_des = || {
            let es = err_seq.next();
            let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Expr).unwrap();
            let ts = quote! {
                let mut rd = BufReader::new(data);
                let value = #enforce_ty::des_shrink_wrap(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
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
        let ev_kind_builder = cx.prepare_event_kind_builder();
        match flow {
            None => quote! {
                RequestKind::Write { data } => {
                    #des_data
                    self.#write(cx, #maybe_index_chain_call #arg)#maybe_await;
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
                let es_pos = err_seq.next();
                let send_sideband = if cx.use_async {
                    quote! {
                        // a lost credit event is recovered by the host's next CreditQuery
                        let _ = wire_weaver::EventOut::send_with(&mut *cx, |scratch| {
                            ser_ok_event(scratch, 0, EventKind::StreamSideband { path: RefVec::Slice { slice: path }, sideband })
                                .map(|bytes| bytes.len())
                        }).await;
                    }
                } else {
                    quote! {
                        // a lost credit event is recovered by the host's next CreditQuery
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
                quote! {
                    RequestKind::Write { data } => {
                        let credit_write = {
                            let mut rd = BufReader::new(data);
                            ww_client_server::CreditWrite::des_shrink_wrap(&mut rd)
                                .map_err(|_e| Error::new(#es_pos, ErrorKind::ArgsDesFailed))?
                        };
                        let data = &credit_write.data;
                        #credit_max_len_check
                        let verdict = self.#credit_fn(#maybe_index_chain_call).on_write(credit_write.pos.0, credit_write.data.0.len() as u32);
                        match verdict {
                            ww_client_server::credit::CreditVerdict::Accept => {
                                #credit_des_data
                                self.#write(cx, #maybe_index_chain_call #arg)#maybe_await;
                                if let Some(sideband) = self.#credit_fn(#maybe_index_chain_call).take_credit() {
                                    #send_sideband
                                }
                            }
                            ww_client_server::credit::CreditVerdict::Gap(Some(sideband)) => {
                                #send_sideband
                            }
                            ww_client_server::credit::CreditVerdict::Duplicate
                            | ww_client_server::credit::CreditVerdict::Gap(None) => {}
                        }
                        #ev_kind_builder
                        Ok(WrAction::WrittenOk(event_kind_builder))
                    }
                    RequestKind::StreamSideband { sideband: StreamSideband::CreditQuery { sent } } => {
                        // answered here, the user's sideband handler doesn't see it
                        let sideband = self.#credit_fn(#maybe_index_chain_call).query(sent.0);
                        #reply_sideband
                    }
                    RequestKind::StreamSideband { sideband } => {
                        if matches!(sideband, StreamSideband::Open) {
                            self.#credit_fn(#maybe_index_chain_call).open();
                        }
                        #call_user_sideband
                        // initial credit after Open, or a re-grant if the handler freed something
                        if let Some(sideband) = self.#credit_fn(#maybe_index_chain_call).take_credit() {
                            #send_sideband
                        }
                        #match_user_sideband
                    }
                }
            }
        }
    };
    let es = err_seq.next();
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

fn ser_method_output(
    return_type: &Option<TypeOwned>,
    seq_path: TokenStream,
    errors_seq: &mut ErrorSeq,
) -> TokenStream {
    if let Some(_ty) = return_type {
        let es = errors_seq.next();
        let ser_output = quote! { output.ser_shrink_wrap(&mut wr).map_err(|_| Error::response_ser_failed(#es))?; };

        let es0 = errors_seq.next();
        let es1 = errors_seq.next();
        let es2 = errors_seq.next();
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
        let es = errors_seq.next();
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
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    let args_struct_ident = api_common::args_struct_ident(&method_ident.to_string(), no_alloc);
    if args.is_empty() {
        (quote! {}, quote! {})
    } else {
        let es = error_seq.next();
        let args_des = quote! {
            let args = args.as_slice();
            let mut rd = BufReader::new(args);
            // TODO: Log _e ?
            let args = #mod_ident::#args_struct_ident::des_shrink_wrap(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
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
    no_alloc: bool,
    method_model: &MethodModel,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let byte_return_ty = if no_alloc {
        quote! { &'i [u8] }
    } else {
        quote! { Vec<u8> }
    };
    for item in &api_level.items {
        let ApiItemKindOwned::Method { return_ty, .. } = &item.kind else {
            continue;
        };
        if method_model.pick(&item.ident) != Some(MethodModelKind::Deferred) {
            continue;
        }
        let fn_name = Ident::new(
            format!("{}_ser_return_event", item.ident).as_str(),
            Span::call_site(),
        );
        let ser_output_or_unit = ser_method_output(return_ty, quote! { seq }, error_seq);
        let maybe_output = match return_ty {
            Some(ty) => {
                let ty = ty_def(api_bundle, ty, !no_alloc, TyPos::Annotation).unwrap();
                quote! { , output: #ty }
            }
            None => quote! {},
        };
        let ser_output = maybe_quote(
            return_ty.is_some(),
            quote! { output.ser_shrink_wrap(&mut wr)?; },
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
            format!("{}_send_return", item.ident).as_str(),
            Span::call_site(),
        );
        let send_blocking_fn_name = Ident::new(
            format!("{}_send_return_blocking", item.ident).as_str(),
            Span::call_site(),
        );
        ts.extend(quote! {
            pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u32 #maybe_output) -> Result<#byte_return_ty, Error<'static>> {
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

fn ser_value(multi_req: bool, ident: TokenStream, error_seq: &mut ErrorSeq) -> TokenStream {
    let es = error_seq.next();
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
