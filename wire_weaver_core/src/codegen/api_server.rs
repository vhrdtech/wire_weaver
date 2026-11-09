//! # Implementation details:
//! * Server's index chain contains only array indices on the way to a resource
use crate::codegen::CodegenBundle;
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
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Delivery, FlowControl,
    Multiplicity, PropertyAccess, StreamFnRef, TypeOwned,
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
    /// How the introspection bytes are stored in flash and sent (INT-16), `introspect_compression = ".."`.
    pub introspect_compression: IntrospectCompression,
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
    /// Whether the server takes samples of `+batched` properties (`observe_samples = false`, SRV-56: no batcher,
    /// outbox or `Series` encoder, the slots stay `Batched`, `Samples` mode refused with `ModeNotSupported`).
    pub observe_samples: bool,
    /// How the generated code names crates it doesn't depend on (API-60), from [crate::load_dep_with_paths];
    /// default: every crate by its own name.
    pub crate_paths: crate::transform::CratePaths,
    /// Which global trait paths the server resolves itself (SRV-53), `global_paths = ".."`.
    pub global_paths: GlobalPaths,
}

pub enum IntrospectMode {
    Disabled,
    NoDocs,
    WithDocs,
}

/// How a device keeps its introspection bytes (INT-16): `ww_codegen!(.., introspect_compression = "none" | "lz4")`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IntrospectCompression {
    /// The serialized bundle as is.
    #[default]
    None,
    /// LZ4 blocks compressed at build time (`ww_self::compressed`, their own magic in front), sent like a plain
    /// bundle. The device carries no decoder, the client decompresses; API hashes stay those of the uncompressed
    /// bundle.
    Lz4,
}

impl IntrospectCompression {
    /// From the `introspect_compression` argument of `ww_codegen!`, `""` is [IntrospectCompression::None].
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "" | "none" => Ok(IntrospectCompression::None),
            "lz4" => Ok(IntrospectCompression::Lz4),
            u => Err(format!(
                "Unknown introspect_compression '{u}', supported: \"none\" or \"\" and \"lz4\""
            )),
        }
    }
}

/// Which global trait paths (`PathKind::GlobalFull`, `PathKind::GlobalCompact`, API-20) a server resolves itself
/// (SRV-53): `ww_codegen!(.., global_paths = "full" | "compact" | "none")`. A path it doesn't resolve is answered
/// with `TraitNotFound`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GlobalPaths {
    /// [GlobalPaths::Full] without introspection, [GlobalPaths::None] with it: a client resolves a global path to
    /// an absolute one from the introspection data, so the device carries no lookup.
    #[default]
    Auto,
    /// `GlobalFull` paths by a `const` table of the traits' [ww_global::trait_hash] (no crate or trait names in
    /// flash), `GlobalCompact` paths by the traits' `ww_global` ids.
    Full,
    /// Only `GlobalCompact` paths, by the traits' `ww_global` ids.
    Compact,
    /// No global paths.
    None,
}

impl GlobalPaths {
    /// From the `global_paths` argument of `ww_codegen!`, `""` is [GlobalPaths::Auto].
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "" | "auto" => Ok(GlobalPaths::Auto),
            "full" => Ok(GlobalPaths::Full),
            "compact" => Ok(GlobalPaths::Compact),
            "none" => Ok(GlobalPaths::None),
            u => Err(format!(
                "Unknown global_paths '{u}', supported: \"auto\" or \"\", \"full\", \"compact\" and \"none\""
            )),
        }
    }

    /// What `Auto` is for a server with or without introspection.
    fn resolve(self, introspect: bool) -> Self {
        match self {
            GlobalPaths::Auto if introspect => GlobalPaths::None,
            GlobalPaths::Auto => GlobalPaths::Full,
            other => other,
        }
    }
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
    /// How the introspection bytes are stored in flash and sent (INT-16), `introspect_compression = ".."`.
    pub introspect_compression: IntrospectCompression,
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
    /// Whether the server takes samples of `+batched` properties (`observe_samples = false`, SRV-56: no batcher,
    /// outbox or `Series` encoder, the slots stay `Batched`, `Samples` mode refused with `ModeNotSupported`).
    pub observe_samples: bool,
    /// How the generated code names crates it doesn't depend on (API-60), from [crate::load_dep_with_paths];
    /// default: every crate by its own name.
    pub crate_paths: crate::transform::CratePaths,
    /// Which global trait paths the server resolves itself (SRV-53), `global_paths = ".."`.
    pub global_paths: GlobalPaths,
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
            introspect_compression: config.introspect_compression,
            multi_req: config.multi_req,
            medium: config.medium.as_deref().map(super::util::str_to_path),
            omit: config.omit,
            observe_batch: config.observe_batch,
            observe_outbox: config.observe_outbox,
            observe_cells: config.observe_cells,
            observe_samples: config.observe_samples,
            crate_paths: config.crate_paths,
            global_paths: config.global_paths,
        }
    }
}

/// Generates API server code for the given API bundle and configuration.
/// ApiBundleOwned can be loaded using [crate::load] or [crate::load_dep].
/// Pass a [GenServerConfig] or [GenServerConfigRaw] to configure code generation.
///
/// Alternatively, use `wire_weaver_derive::ww_codegen!` proc-macro if you do not want to use build.rs.
pub fn gen_server(
    api_bundle: &ww_self::ApiBundleOwned,
    config: impl Into<GenServerConfigRaw>,
) -> TokenStream {
    let config: GenServerConfigRaw = config.into();
    let api_bundle = &CodegenBundle::new(api_bundle, &config.crate_paths);
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
    let medium_ty = match &config.medium {
        Some(path) => quote! { #path },
        None => quote! { () },
    };
    let mut observe = Observe::collect(
        api_bundle,
        &omissions,
        &config.property_model,
        config.observe_samples,
    );
    let cells_errors = if config.observe_samples {
        observe.use_cells(&config.observe_cells)
    } else {
        no_samples_errors(
            config.observe_batch,
            config.observe_outbox,
            &config.observe_cells,
        )
    };
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
    // an async server's root level is always `async fn`: it sends the introspection events, and with introspection
    // off its `Introspect` arm still answers with a `Close` event (see `process_request_inner_recursive`)
    let root_await = maybe_quote(cx.use_async, quote! { .await });
    let (handle_introspect, api_signature) = super::server::introspect::introspect(
        api_bundle,
        &omissions,
        introspect_enabled,
        include_docs,
        config.introspect_compression,
        cx.use_async,
    );
    let no_observe_samples = !config.observe_samples;
    let api_signature = quote! {
        #api_signature
        /// What this image chose at build time and the API doesn't show, for `LinkConfig::new`: not in the API hash.
        pub const fn image_facts() -> wire_weaver::ww_version::ImageFacts {
            wire_weaver::ww_version::ImageFacts {
                no_observe_samples: #no_observe_samples,
            }
        }
    };
    let process_request_inner = process_request_inner_recursive(
        "root".into(),
        &[],
        &quote! {},
        &omissions,
        api_bundle,
        api_level,
        IndexChain::new(),
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
    let observe_arm = if observe.is_enabled() {
        let es = fixed_err_seq("OBSERVE_MALFORMED");
        quote! {
            RequestKind::Observe { cfg } => {
                // decoded here, not with the request: only a server that observes links the decoder (SRV-54)
                let Ok(cfg) = cfg.read() else {
                    wr.write(&Error::new(#es, ErrorKind::ArgsDesFailed))?;
                    event_builder.finish(false, &mut wr);
                    return wr.finish_and_take();
                };
                match self.observe_request(path, &cfg, &mut wr) {
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
    } else {
        // no observable property anywhere: the same answer as a path without an observable level, whatever the
        // path, so a host gets one defined error and the request is never walked as a call (SRV-54)
        let es = fixed_err_seq("OBSERVE_PATH");
        quote! {
            RequestKind::Observe { .. } => {
                wr.write(&Error::bad_path(#es))?;
                event_builder.finish(false, &mut wr);
                wr.finish_and_take()
            }
        }
    };
    let consts_trait = consts_trait(api_bundle, &omissions, &cx);
    let es2 = fixed_err_seq("REQUEST_PATH_TOO_DEEP");
    let (global_path_arms, global_trait_path) =
        match global_trait_path(api_bundle, config.global_paths.resolve(introspect_enabled)) {
            Ok(global) => global,
            Err(e) => return quote! { compile_error!(#e); },
        };
    let server_struct_path = config.server_struct_path;
    let multi = cx.multi_req;
    let maybe_multi_req = maybe_quote_cl(cx.multi_req, || multi_req_handlers(&root_await));
    let growable_reply_helpers = maybe_quote(
        !config.no_alloc,
        quote! {
            #[allow(dead_code)]
            fn ww_owned_bytes<T: wire_weaver::shrink_wrap::SerializeShrinkWrapOwned>(value: &T) -> Result<Vec<u8>, ShrinkWrapError> {
                let mut wr = wire_weaver::shrink_wrap::BufWriterOwned::new();
                value.ser_shrink_wrap_owned(&mut wr)?;
                wr.finish_and_take()
            }

            #[allow(dead_code)]
            fn ww_value_event(seq: u32, value: &[u8]) -> Result<Vec<u8>, ShrinkWrapError> {
                const HEAD_MAX_LEN: usize = 8;
                let mut event = Vec::new();
                event.resize(value.len() + HEAD_MAX_LEN, 0u8);
                let len = ser_value_event(&mut event, seq, value)?.len();
                event.truncate(len);
                Ok(event)
            }
        },
    );
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
            util::{ser_err_event, ser_unit_return_event, ser_value_event, ser_stream_sideband_event},
            builder::{EventBuilder, EventKindBuilder, ErrorBuilder},
            dispatch::{err as ww_err, WrAction},
        };
        use core::slice::Iter;
        #additional_use
        #api_signature

        /// `expr`'s `Ok` value, or the error `(err_seq, kind)` written in place as the answer of the level (SRV-52).
        #[allow(unused_macros)]
        macro_rules! __ww_try {
            ($wr:ident, $d:ident, $e:expr, $es:expr, $kind:ident) => {
                match $e {
                    Ok(v) => v,
                    Err(_) => return ww_err($wr, $d, $es, ErrorKindDiscriminants::$kind),
                }
            };
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
                let mut cx = wire_weaver::Context::new(request.seq.0, medium, out).with_priority(request.priority.into());

                let mut wr = BufWriter::new(scratch);
                let event_builder = EventBuilder::new(request.seq.0, &mut wr)?;

                // a global trait path continues from wherever the trait sits in this API (API-20)
                let (base, path): (&[UNib32], _) = match &request.path_kind {
                    PathKind::Absolute { path } => (&[], path),
                    #global_path_arms
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
                        let d = ww_client_server::dispatch::Dispatch::<'_, #multi> {
                            request: &request,
                            path,
                            before: before_process,
                            in_multi: false,
                        };
                        match self.process_root(&mut iter, &mut wr, &d, &mut cx)#root_await {
                            WrAction::WrittenOk => {
                                if request.seq.0 == 0 {
                                    return Ok(&[])
                                }
                                // the kind's nibble is where the reply starts (SRV-52)
                                EventKindBuilder::at(d.before).finish_with_kind(request.kind.discriminants(), &mut wr);
                                event_builder.finish(true, &mut wr);
                                wr.finish_and_take()
                            }
                            WrAction::WrittenErr => {
                                event_builder.finish(false, &mut wr);
                                wr.finish_and_take()
                            }
                            WrAction::Acknowledged => {
                                if request.seq.0 == 0 {
                                    return Ok(&[])
                                }
                                let event_kind_builder = EventKindBuilder::new(&mut wr)?;
                                event_kind_builder.finish_with_kind(EventKindDiscriminants::Written, &mut wr);
                                event_builder.finish(true, &mut wr);
                                wr.finish_and_take()
                            }
                            WrAction::Deferred => {
                                Ok(&[])
                            }
                            WrAction::Failed(e) => Err(e),
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
        #growable_reply_helpers

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

/// `observe_samples = false` (SRV-56) generates no batches: an argument that sizes or places them is a contradiction.
fn no_samples_errors(
    observe_batch: Option<usize>,
    observe_outbox: Option<usize>,
    observe_cells: &[String],
) -> TokenStream {
    let args = [
        ("observe_batch", observe_batch.is_some()),
        ("observe_outbox", observe_outbox.is_some()),
        ("observe_cells", !observe_cells.is_empty()),
    ];
    let errors = args.iter().filter(|(_, given)| *given).map(|(arg, _)| {
        let msg = format!(
            "{arg} has nothing to do with observe_samples = false: no batches are generated"
        );
        quote! { compile_error!(#msg); }
    });
    quote! { #(#errors)* }
}

/// Streams of the API `poll_streams` sends for, or an error message for one it can't send.
/// Items left out in this combination of `omit_if` conditions (`active`, see [Omissions::cfg_combinations]) and their
/// streams are skipped.
#[allow(clippy::too_many_arguments)]
fn polled_streams_recursive(
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
                        ser_stream_sideband_event(scratch, 0, &path, sideband)
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

/// How the server resolves global trait paths (API-20, SRV-53): the arms of `process_request_bytes` for
/// `PathKind::GlobalFull` and `PathKind::GlobalCompact`, and the items they use. A path the server doesn't resolve
/// is answered with `TraitNotFound`.
///
/// `GlobalFull` paths are looked up by [ww_global::trait_hash] of the request's crate id and trait name in a
/// `const` table (no names in flash), `GlobalCompact` paths by the traits' `ww_global` ids. Traits under a resource
/// array are left out (a global path can't select an index), and when a trait is implemented several times, the
/// first one in depth first, id order is used. A trait matches when the requested crate version is protocol
/// compatible with the one this API was built with (same major, and same minor before 1.0). Two traits of the API
/// with the same hash are an error.
fn global_trait_path(
    api_bundle: &CodegenBundle,
    global_paths: GlobalPaths,
) -> Result<(TokenStream, TokenStream), String> {
    let es = fixed_err_seq("REQUEST_TRAIT_NOT_FOUND");
    let not_found = quote! {{
        wr.write(&Error::new(#es, ErrorKind::TraitNotFound))?;
        event_builder.finish(false, &mut wr);
        return wr.finish_and_take();
    }};
    let compact_arm = quote! {
        PathKind::GlobalCompact { gid, path_from_trait } => {
            match ww_global_compact_trait_path(gid) {
                Some(base) => (base, path_from_trait),
                None => #not_found
            }
        }
    };
    let mut traits = vec![];
    global_traits_recursive(api_bundle, &api_bundle.root, &mut vec![], &mut traits);
    let (arms, items) = match global_paths {
        GlobalPaths::Auto => unreachable!("resolved by the caller"),
        GlobalPaths::None => (
            quote! { PathKind::GlobalFull { .. } | PathKind::GlobalCompact { .. } => #not_found },
            quote! {},
        ),
        GlobalPaths::Compact => (
            quote! {
                PathKind::GlobalFull { .. } => #not_found
                #compact_arm
            },
            global_compact_lookup(&traits),
        ),
        GlobalPaths::Full => {
            let table = global_full_table(&traits)?;
            let compact = global_compact_lookup(&traits);
            (
                quote! {
                    PathKind::GlobalFull { gid, trait_name, path_from_trait } => {
                        match ww_client_server::dispatch::global_full_path(WW_GLOBAL_TRAITS, gid, trait_name) {
                            Some(base) => (base, path_from_trait),
                            None => #not_found
                        }
                    }
                    #compact_arm
                },
                quote! { #table #compact },
            )
        }
    };
    Ok((arms, items))
}

/// The `const` table `GlobalFull` paths are looked up in (SRV-53), or an error naming two traits whose hashes
/// collide, or a crate version that doesn't fit.
fn global_full_table(traits: &[GlobalTrait]) -> Result<TokenStream, String> {
    let mut entries = vec![];
    for (i, t) in traits.iter().enumerate() {
        let crate_id = t.crate_version.crate_id.as_str();
        let hash = ww_global::trait_hash(crate_id, &t.trait_name);
        if let Some(other) = traits[..i].iter().find(|o| {
            ww_global::trait_hash(o.crate_version.crate_id.as_str(), &o.trait_name) == hash
        }) {
            return Err(format!(
                "global trait paths (SRV-53): {crate_id}::{} and {}::{} have the same hash {hash:#010x}, \
                 rename one of the traits or use global_paths = \"compact\"",
                t.trait_name, other.crate_version.crate_id, other.trait_name
            ));
        }
        let v = &t.crate_version.version;
        let (Ok(major), Ok(minor)) = (u16::try_from(v.major.0), u16::try_from(v.minor.0)) else {
            return Err(format!(
                "global trait paths (SRV-53): {crate_id} {}.{} has a major or minor version above {}",
                v.major.0,
                v.minor.0,
                u16::MAX
            ));
        };
        let path = &t.path;
        entries.push(quote! {
            ww_client_server::dispatch::GlobalTrait {
                hash: #hash, major: #major, minor: #minor, path: &[#(UNib32(#path)),*]
            }
        });
    }
    Ok(quote! {
        /// Traits a `GlobalFull` path reaches, by the hash of their crate and name (SRV-53).
        const WW_GLOBAL_TRAITS: &[ww_client_server::dispatch::GlobalTrait] = &[#(#entries),*];
    })
}

/// `ww_global_compact_trait_path`: absolute path of a trait by its `ww_global` id.
fn global_compact_lookup(traits: &[GlobalTrait]) -> TokenStream {
    let version_check = |crate_version: &FullVersionOwned| {
        let (major, minor) = (crate_version.version.major.0, crate_version.version.minor.0);
        if major >= 1 {
            quote! { gid.major.0 == #major }
        } else {
            quote! { gid.major.0 == 0 && gid.minor.0 == #minor }
        }
    };
    let compact_arms = traits.iter().filter_map(|t| {
        let id = t.compact_id?;
        let path = &t.path;
        let version_check = version_check(&t.crate_version);
        Some(quote! {
            #id if #version_check => Some(&[#(UNib32(#path)),*]),
        })
    });
    quote! {
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
    api_bundle: &CodegenBundle,
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
                // an element's error is written where the element starts (SRV-52)
                let item_written = |wr: &mut BufWriter<'_>, es: u32, kind: ErrorKind<'_>| {
                    ww_client_server::dispatch::write_error_at(wr, &item_start, &Error::new(es, kind))
                };
                let action = match item_kind {
                    Some(kind) => {
                        let item_request = Request {
                            seq: request.seq,
                            priority: request.priority,
                            path_kind: PathKind::Absolute { path: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] } },
                            kind,
                        };
                        let d = ww_client_server::dispatch::Dispatch::<'_, true> {
                            request: &item_request,
                            path,
                            before: item_start,
                            in_multi: true,
                        };
                        match self.process_root(&mut iter, &mut wr, &d, &mut cx)#maybe_await {
                            // TODO(CLT-15): a deferred answer inside a multi request, the handler answers later with
                            // the multi request's seq, which the host doesn't expect
                            WrAction::Deferred => item_written(&mut wr, #es_deferred, ErrorKind::OperationNotSupported),
                            action => action,
                        }
                    }
                    None => item_written(&mut wr, #es_args, ErrorKind::ArgsDesFailed),
                };
                let is_ok = match action {
                    WrAction::WrittenOk | WrAction::Acknowledged => true,
                    WrAction::WrittenErr | WrAction::Deferred => false,
                    WrAction::Failed(_) => {
                        overflow = true;
                        break;
                    }
                };
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
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
        cx,
    );
    let maybe_index_chain_def = index_chain.fun_argument_def();

    let process_fn_name = Ident::new(
        format!("process_{}", level_name_chain).as_str(),
        Span::call_site(),
    );
    let es = fixed_err_seq("LEVEL_NOT_SUPPORTED");
    let multi = cx.multi_req;
    let context_ty = cx.context_ty();
    let mut ts = quote! {
        // SRV-52: errors are written in place, a level returns only what it did
        #[allow(clippy::too_many_arguments, unused_variables)]
        #cfg_attrs
        #maybe_async fn #process_fn_name(
            &mut self,
            #maybe_index_chain_def
            path_iter: &mut Iter<'_, UNib32>,
            wr: &mut BufWriter<'_>,
            d: &ww_client_server::dispatch::Dispatch<'_, #multi>,
            cx: &mut #context_ty,
        ) -> WrAction {
            let (request, path) = (d.request, d.path);
            match path_iter.next().copied() {
                #level_matchers
                None => {
                    match request.kind {
                        #introspect
                        _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported) },
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
            &cx,
            None,
        ));
    }
    ts
}

#[allow(clippy::too_many_arguments)]
fn level_matchers(
    api_bundle: &CodegenBundle,
    api_level: &ApiLevelOwned,
    omitted: &BTreeMap<u32, Omission>,
    level_name_chain: &str,
    index_chain: IndexChain,
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
        quote! { id if id < #item_count => { ww_err(wr, d, #es, ErrorKindDiscriminants::Unimplemented) } }
    };
    let handlers = present.map(|item| match &item.multiplicity {
        Multiplicity::Flat => level_matcher(
            api_bundle,
            item,
            level_name_chain,
            index_chain,
            util::mod_name(api_level, api_bundle),
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
                util::mod_name(api_level, api_bundle),
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
                    return ww_err(wr, d, #es_index, ErrorKindDiscriminants::BadIndex);
                }
            };
            let es = fixed_err_seq("LEVEL_INDEX_EXPECTED");
            let ser_indices = ser_value(
                cx.multi_req,
                quote! { indices },
                fixed_err_seq("LEVEL_INDICES_SER"),
            );
            let ev_kind_builder = cx.prepare_event_kind_builder();
            let read_indices = if cx.no_alloc {
                let es_ser = fixed_err_seq("LEVEL_INDICES_SER");
                quote! {
                    let indices = self.#valid_indices(#maybe_index_chain_arg);
                    return ww_client_server::dispatch::reply_value(wr, d, &indices, #es_ser);
                }
            } else {
                quote! {
                    let indices = self.#valid_indices(#maybe_index_chain_arg);
                    #ev_kind_builder
                    #ser_indices
                    return WrAction::WrittenOk;
                }
            };
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
                            #read_indices
                        } else {
                            ww_err(wr, d, #es, ErrorKindDiscriminants::ExpectedArrayIndexGotNone)
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
            _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::BadPath) }
        }
    }
}

fn level_matcher(
    api_bundle: &CodegenBundle,
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
            quote! {
                self.#process_fn_name(#maybe_index_chain_arg path_iter, wr, d, cx)#maybe_await
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
    fn value_ty(&self, api_bundle: &CodegenBundle, ty: &TypeOwned, pos: TyPos) -> TokenStream {
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
        let root = if self.no_alloc {
            let root = self.ser_root_value(value.clone());
            quote! { __ww_try!(wr, d, #root, #es, ResponseSerFailed); }
        } else {
            self.ser_growable_reply(value.clone(), es.clone())
        };
        if self.multi_req {
            let element = self.write_value(value);
            quote! {
                if d.in_multi {
                    __ww_try!(wr, d, #element, #es, ResponseSerFailed);
                } else {
                    #root
                }
            }
        } else {
            root
        }
    }

    /// The answer of a `no_alloc = false` server (CLT-55): the value is serialized into a growable buffer and goes
    /// into `wr` when it fits; a longer one is sent from there as a whole event, up to the link's message limit.
    fn ser_growable_reply(&self, value: TokenStream, es: TokenStream) -> TokenStream {
        let es_over_limit = fixed_err_seq("REPLY_OVER_LIMIT");
        let send = if self.use_async {
            quote! { wire_weaver::EventOut::send_reply_bytes(&mut *cx, priority, &event).await }
        } else {
            quote! { wire_weaver::BlockingEventOut::send_reply_bytes_blocking(&mut *cx, priority, &event) }
        };
        quote! {
            let bytes = __ww_try!(wr, d, ww_owned_bytes(&#value), #es, ResponseSerFailed);
            if wr.write_raw_slice(&bytes).is_err() {
                if request.seq.0 != 0 {
                    let event = __ww_try!(wr, d, ww_value_event(request.seq.0, &bytes), #es, ResponseSerFailed);
                    let priority = cx.priority();
                    match #send {
                        Ok(()) => {}
                        Err(wire_weaver::SendError::Sink) => {
                            return ww_err(wr, d, #es_over_limit, ErrorKindDiscriminants::ResponseSerFailed);
                        }
                        Err(wire_weaver::SendError::Serialize(_)) => {
                            return ww_err(wr, d, #es, ErrorKindDiscriminants::ResponseSerFailed);
                        }
                    }
                }
                return WrAction::Deferred;
            }
        }
    }

    /// Starts an `Ok` answer's event kind (nothing in a multi request's element), filled in at the root (SRV-52).
    fn prepare_event_kind_builder(&self) -> TokenStream {
        let es = fixed_err_seq("REPLY_START");
        quote! {
            __ww_try!(wr, d, ww_client_server::dispatch::start(wr, d), #es, ResponseSerFailed);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_method(
    api_bundle: &CodegenBundle,
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
            if d.in_multi {
                return ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported);
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
    if cx.no_alloc {
        // SRV-51: the arm decodes, calls and hands the result to the shared reply path
        let output_ty = match return_type {
            Some(ty) => cx.value_ty(api_bundle, ty, TyPos::Annotation),
            None => quote! { () },
        };
        let args_des = if args.is_empty() {
            quote! {}
        } else {
            let args_struct_ident = api_common::args_struct_ident(&ident.to_string(), true);
            let es_args = item_err_seq("method", "ARGS");
            quote! {
                let Some(args) = ww_client_server::dispatch::des::<#mod_ident::#args_struct_ident>(args.as_slice()) else {
                    return ww_err(wr, d, #es_args, ErrorKindDiscriminants::ArgsDesFailed);
                };
            }
        };
        return quote! {
            match &request.kind {
                RequestKind::Call { #maybe_args } => {
                    #maybe_refuse_in_multi
                    #args_des
                    let result: RpcResult<#output_ty> = #call;
                    ww_client_server::dispatch::reply_call(wr, d, result)
                }
                _ => ww_err(wr, d, #es_kind, ErrorKindDiscriminants::OperationNotSupported),
            }
        };
    }
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
                        WrAction::WrittenOk
                    },
                    RpcResult::Deferred => {
                        WrAction::Deferred
                    }
                    RpcResult::Unimplemented => {
                        ww_err(wr, d, #es, ErrorKindDiscriminants::Unimplemented)
                    }
                }
            }
            _ => {
                ww_err(wr, d, #es_kind, ErrorKindDiscriminants::OperationNotSupported)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_property(
    api_bundle: &CodegenBundle,
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
                let err_builder = __ww_try!(wr, d, ErrorBuilder::new(#es, wr), #es, ResponseSerFailed);
                __ww_try!(wr, d, #write_user_err, #es, ResponseSerFailed);
                __ww_try!(wr, d, err_builder.finish_with_kind(ErrorKindDiscriminants::UserBytes, wr), #es, ResponseSerFailed);
            }
        })
        .unwrap_or(quote! {
            let _unit: () = user_err;
            __ww_try!(wr, d, wr.write(&Error::new(#es, ErrorKind::UserBytes(RefVec::new_bytes(&[])))), #es, ResponseSerFailed);
        });
    let property_model_pick = cx
        .property_model
        .pick(ident.to_string().as_str())
        .unwrap_or(PropertyModelKind::GetSet);
    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    // SRV-51: the shared reply paths of a `no_alloc` server, by whether the property has a user error type
    let (reply_get, reply_set, get_result_ty, set_result_ty) = match user_error_ty {
        Some(ty) => {
            let user_err_ty = cx.value_ty(api_bundle, ty, TyPos::Annotation);
            (
                quote! { ww_client_server::dispatch::reply_get },
                quote! { ww_client_server::dispatch::reply_set },
                quote! { GetResult<#enforce_ty, #user_err_ty> },
                quote! { SetResult<#user_err_ty> },
            )
        }
        None => (
            quote! { ww_client_server::dispatch::reply_get_unit_err },
            quote! { ww_client_server::dispatch::reply_set_unit_err },
            quote! { GetResult<#enforce_ty, ()> },
            quote! { SetResult<()> },
        ),
    };
    let des_compact = |es: &TokenStream| {
        quote! {
            let Some(value) = ww_client_server::dispatch::des::<#enforce_ty>(data.as_slice()) else {
                return ww_err(wr, d, #es, ErrorKindDiscriminants::PropertyDesFailed);
            };
        }
    };

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
                    if cx.no_alloc {
                        let des = des_compact(&es);
                        quote! {
                            #des
                            let result: #set_result_ty = #set_call;
                            #reply_set(wr, d, result)
                        }
                    } else {
                        quote! {
                            let mut rd = BufReader::new(data.as_slice());
                            let value = __ww_try!(wr, d, #des_value(&mut rd), #es, PropertyDesFailed);
                            match #set_call {
                                SetResult::Set => {
                                    #ev_kind_builder
                                    WrAction::WrittenOk
                                },
                                SetResult::SetError(user_err) => {
                                    // if request.seq.0 != 0 {
                                    // always send errors back, even if they won't reach a user call site, they will show up in logs
                                    #ser_user_err
                                    WrAction::WrittenErr
                                }
                                SetResult::Unimplemented => {
                                    ww_err(wr, d, #es_handler, ErrorKindDiscriminants::Unimplemented)
                                }
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
                    if cx.no_alloc {
                        let des = des_compact(&es);
                        quote! {
                            #des
                            if self.#prefixed_ident #maybe_index_chain_indices != value {
                                self.#prefixed_ident #maybe_index_chain_indices = value;
                                #mark_changed
                                #changed_call;
                            }
                            ww_client_server::dispatch::reply_ok(wr, d)
                        }
                    } else {
                        quote! {
                            #ev_kind_builder
                            let mut rd = BufReader::new(data.as_slice());
                            let value = __ww_try!(wr, d, #des_value(&mut rd), #es, PropertyDesFailed);
                            if self.#prefixed_ident #maybe_index_chain_indices != value {
                                self.#prefixed_ident #maybe_index_chain_indices = value;
                                #mark_changed
                                #changed_call;
                            }
                            WrAction::WrittenOk
                        }
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
                    if cx.no_alloc {
                        quote! {
                            let result: #get_result_ty = #get_call;
                            #reply_get(wr, d, result)
                        }
                    } else {
                        quote! {
                            match #get_call {
                                GetResult::Value(value) => {
                                    #ev_kind_builder
                                    let value: #enforce_ty = value;
                                    #ser_value
                                    WrAction::WrittenOk
                                }
                                GetResult::GetError(user_err) => {
                                    #ser_user_err
                                    WrAction::WrittenErr
                                }
                                GetResult::Unimplemented => {
                                    __ww_try!(wr, d, wr.write(&Error::unimplemented(#es)), #es, ResponseSerFailed);
                                    WrAction::WrittenErr
                                }
                            }
                        }
                    }
                }
                PropertyModelKind::ValueOnChanged => {
                    if cx.no_alloc {
                        let es_value = item_err_seq("property", "VALUE");
                        quote! {
                            let value: &#enforce_ty = &self.#prefixed_ident #maybe_index_chain_indices;
                            ww_client_server::dispatch::reply_value(wr, d, value, #es_value)
                        }
                    } else {
                        quote! {
                            #ev_kind_builder
                            let value: &#enforce_ty = &self.#prefixed_ident #maybe_index_chain_indices;
                            #ser_value
                            WrAction::WrittenOk
                        }
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
            _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported) }
        }
    }
}

/// A `const` item (API-36): read only, its value comes from the generated `<Trait>Consts` trait the server struct
/// implements, an associated const or a `fn`, see [consts_trait].
fn handle_const(
    api_bundle: &CodegenBundle,
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
    if cx.no_alloc {
        let es_value = item_err_seq("constant", "VALUE");
        return quote! {
            match &request.kind {
                RequestKind::Read => {
                    let value: #enforce_ty = #value;
                    ww_client_server::dispatch::reply_value(wr, d, &value, #es_value)
                }
                _ => ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported),
            }
        };
    }
    quote! {
        match &request.kind {
            RequestKind::Read => {
                #ev_kind_builder
                let value: #enforce_ty = #value;
                #ser_value
                WrAction::WrittenOk
            }
            _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported) }
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    let (args_des, args_list) = if cx.no_alloc {
        des_args_compact(mod_ident, ident, &args, item_err_seq("stream", "ARGS"))
    } else {
        des_args(
            mod_ident,
            ident,
            &args,
            cx.no_alloc,
            item_err_seq("stream", "ARGS"),
        )
    };
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
                Err(_) => return ww_err(wr, d, #es_store, ErrorKindDiscriminants::StreamStoreFailed),
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
    let open = if cx.no_alloc && delivery == Delivery::Plain && !batched {
        // SRV-51: no session, so the whole answer is the shared path's
        quote! {
            RequestKind::StreamOpen { resume: None, #max_age_arg #codecs_arg #maybe_args } => {
                #args_des
                let result: RpcResult<#enforce_ty> = #open_call;
                ww_client_server::dispatch::reply_open_plain(wr, d, result)
            }
        }
    } else if cx.no_alloc {
        quote! {
            RequestKind::StreamOpen { resume: None, #max_age_arg #codecs_arg #maybe_args } => {
                #args_des
                match #open_call {
                    RpcResult::Ready(output) => {
                        let output: #enforce_ty = output;
                        #open_state
                        ww_client_server::dispatch::reply_opened(wr, d, &opened, &output)
                    }
                    // a stream function answers right away: the session starts with its result
                    RpcResult::Deferred => ww_err(wr, d, #es_open, ErrorKindDiscriminants::OperationNotSupported),
                    RpcResult::Unimplemented => ww_err(wr, d, #es_open, ErrorKindDiscriminants::Unimplemented),
                }
            }
        }
    } else {
        quote! {
            RequestKind::StreamOpen { resume: None, #max_age_arg #codecs_arg #maybe_args } => {
                #args_des
                match #open_call {
                    RpcResult::Ready(output) => {
                        let output: #enforce_ty = output;
                        #open_state
                        #ev_kind_builder
                        __ww_try!(wr, d, opened.ser_head(wr), #es_ser, ResponseSerFailed);
                        __ww_try!(wr, d, #ser_output, #es_ser, ResponseSerFailed);
                        WrAction::WrittenOk
                    }
                    // a stream function answers right away: the session starts with its result
                    RpcResult::Deferred => ww_err(wr, d, #es_open, ErrorKindDiscriminants::OperationNotSupported),
                    RpcResult::Unimplemented => ww_err(wr, d, #es_open, ErrorKindDiscriminants::Unimplemented),
                }
            }
        }
    };
    let es_resume = item_err_seq("stream", "RESUME");
    let resume = if delivery == Delivery::Plain {
        quote! {}
    } else {
        let ev_kind_builder = cx.prepare_event_kind_builder();
        // the answer of a resumed session: its head, without an output
        let reply_resumed = |opened: TokenStream| {
            if cx.no_alloc {
                quote! { ww_client_server::dispatch::reply_opened(wr, d, &#opened, &()) }
            } else {
                quote! {{
                    #ev_kind_builder
                    __ww_try!(wr, d, #opened.ser_head(wr), #es_ser, ResponseSerFailed);
                    WrAction::WrittenOk
                }}
            }
        };
        let resumed_codec = reply_resumed(quote! { opened.to_wire(&[]).with_codec(codec) });
        let resumed = reply_resumed(quote! { opened.to_wire(&[]) #with_granted });
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
                    let (opened, codec) = match #resume_result {
                        Ok(v) => v,
                        Err(kind) => return ww_client_server::dispatch::err_kind(wr, d, #es_resume, kind),
                    };
                    #resumed_codec
                }
            }
        } else {
            quote! {
                RequestKind::StreamOpen { resume: Some(cursor), #max_age_arg .. } => {
                    #grant
                    let r = #state #inner .resume(*cursor)#store_await;
                    let opened = match #resume_result {
                        Ok(v) => v,
                        Err(kind) => return ww_client_server::dispatch::err_kind(wr, d, #es_resume, kind),
                    };
                    #resumed
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
            WrAction::Acknowledged
        }
    };
    let es_sb = item_err_seq("stream", "SIDEBAND");
    let sideband = match delivery {
        Delivery::Plain => {
            let sideband_fn = Ident::new(&format!("sideband_{prefixed_ident}"), ident.span());
            let sideband_call = cx.call(ident, quote! { self.#sideband_fn(cx, #idx *sideband) });
            let ev_kind_builder = cx.prepare_event_kind_builder();
            if cx.no_alloc {
                quote! {
                    RequestKind::StreamSideband { sideband } => {
                        let answer: Option<StreamSideband> = #sideband_call;
                        ww_client_server::dispatch::reply_sideband(wr, d, answer)
                    }
                }
            } else {
                quote! {
                    RequestKind::StreamSideband { sideband } => {
                        // user fn returns Option<StreamSideband>
                        match #sideband_call {
                            Some(sideband) => {
                                #ev_kind_builder
                                __ww_try!(wr, d, wr.write(&RefVec::Slice { slice: &path }), #es_sb, ResponseSerFailed);
                                __ww_try!(wr, d, wr.write(&sideband), #es_sb, ResponseSerFailed);
                                WrAction::WrittenOk
                            }
                            // nothing to send back, but the host may wait with the request's seq
                            None => WrAction::Acknowledged,
                        }
                    }
                }
            }
        }
        // acknowledgements of a counted stream mean nothing: nothing is kept
        Delivery::Counted => quote! {
            RequestKind::StreamSideband { sideband: StreamSideband::Ack { .. } } => {
                WrAction::Acknowledged
            }
        },
        Delivery::Reliable => quote! {
            RequestKind::StreamSideband {
                sideband: sideband @ (StreamSideband::Ack { .. } | StreamSideband::Resend { .. })
            } => {
                __ww_try!(wr, d, #state #inner .on_host_sideband(sideband).await, #es_store, StreamStoreFailed);
                WrAction::Acknowledged
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
        if cx.no_alloc {
            let es_status = item_err_seq("stream", "STATUS");
            quote! {
                RequestKind::Read => {
                    let status: ww_client_server::stream::StreamStatus = #status_call;
                    ww_client_server::dispatch::reply_value(wr, d, &status, #es_status)
                }
            }
        } else {
            quote! {
                RequestKind::Read => {
                    let status: ww_client_server::stream::StreamStatus = #status_call;
                    #ev_kind_builder
                    #ser_status
                    WrAction::WrittenOk
                }
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
            _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::OperationNotSupported) }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_stream(
    api_bundle: &CodegenBundle,
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
    // SRV-51: a `no_alloc` server answers through the shared paths
    let reply_sideband = if cx.no_alloc {
        quote! { ww_client_server::dispatch::reply_sideband(wr, d, Some(sideband)) }
    } else {
        quote! {
            #ev_kind_builder
            __ww_try!(wr, d, wr.write(&RefVec::Slice { slice: &path }), #es, ResponseSerFailed);
            __ww_try!(wr, d, wr.write(&sideband), #es, ResponseSerFailed);
            WrAction::WrittenOk
        }
    };
    let reply_ack = if cx.no_alloc {
        quote! { ww_client_server::dispatch::reply_ok(wr, d) }
    } else {
        quote! {
            #ev_kind_builder
            WrAction::WrittenOk
        }
    };
    let sideband_call = cx.call(
        ident,
        quote! { self.#sideband_fn(cx, #maybe_index_chain_call *sideband) },
    );
    let call_user_sideband = quote! {
        // user fn returns Option<StreamSideband>
        let r = #sideband_call;
    };
    let match_user_sideband = if cx.no_alloc {
        quote! { ww_client_server::dispatch::reply_sideband(wr, d, r) }
    } else {
        quote! {
            match r {
                Some(sideband) => {
                    #reply_sideband
                }
                None => {
                    // nothing to send back, but the host is waiting with the request's seq
                    WrAction::Acknowledged
                }
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
                        return ww_err(wr, d, #es, ErrorKindDiscriminants::ArgsDesFailed);
                    }
                }
            }
            _ => quote! {},
        };
        let other_des = || {
            let es = item_err_seq("sink", "DATA");
            let enforce_ty = cx.value_ty(api_bundle, ty, TyPos::Expr);
            let des_value = cx.des_value(&enforce_ty);
            let ts = if cx.no_alloc {
                quote! {
                    let Some(value) = ww_client_server::dispatch::des::<#enforce_ty>(data) else {
                        return ww_err(wr, d, #es, ErrorKindDiscriminants::ArgsDesFailed);
                    };
                }
            } else {
                quote! {
                    let mut rd = BufReader::new(data);
                    let value = __ww_try!(wr, d, #des_value(&mut rd), #es, ArgsDesFailed);
                }
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
        match flow {
            None => quote! {
                RequestKind::Write { data } => {
                    #des_data
                    #write_call;
                    // TODO: ?? do not send acknowledgements on stream writes
                    #reply_ack
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
                            ser_stream_sideband_event(scratch, 0, path, sideband)
                                .map(|bytes| bytes.len())
                        }).await;
                    }
                } else {
                    quote! {
                        // a lost Ack is recovered by the host's next Query
                        let _ = wire_weaver::BlockingEventOut::send_with_blocking(&mut *cx, |scratch| {
                            ser_stream_sideband_event(scratch, 0, path, sideband)
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
                let (reply_opened, reply_status) = if cx.no_alloc {
                    let es_status = item_err_seq("sink", "STATUS");
                    (
                        quote! {
                            let opened = opened.to_wire(&[]).with_capacity(Some(capacity));
                            ww_client_server::dispatch::reply_opened(wr, d, &opened, &())
                        },
                        quote! { ww_client_server::dispatch::reply_value(wr, d, &status, #es_status) },
                    )
                } else {
                    (
                        quote! {
                            #ev_kind_builder_open
                            __ww_try!(wr, d, opened.to_wire(&[]).with_capacity(Some(capacity)).ser_head(wr), #es_opened, ResponseSerFailed);
                            WrAction::WrittenOk
                        },
                        quote! {
                            #ev_kind_builder_status
                            #ser_status
                            WrAction::WrittenOk
                        },
                    )
                };
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
                        // the sink's buffer: a resumed host gets no grant of all of it while something is buffered
                        let capacity = #credit.capacity();
                        #reply_opened
                    }
                    RequestKind::StreamOpen { resume: None, .. } => {
                        ww_err(wr, d, #es_open, ErrorKindDiscriminants::OperationNotSupported)
                    }
                    kind @ (RequestKind::SinkWrite { .. } | RequestKind::SinkWriteUnits { .. }) => {
                        let Some((units, pos, data)) = kind.as_sink_write() else {
                            return ww_err(wr, d, #es_open, ErrorKindDiscriminants::OperationNotSupported);
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
                        #reply_ack
                    }
                    RequestKind::StreamSideband { sideband: StreamSideband::Query { sent, .. } } => {
                        // answered here, the user's sideband handler doesn't see it
                        match #credit.on_query(sent.0) {
                            Some(sideband) => {
                                #reply_sideband
                            }
                            None => WrAction::Acknowledged,
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
                        #reply_status
                    }
                }
            }
        }
    };
    let es = item_err_seq("", "KIND_NOT_TAKEN");
    quote! {
        match &request.kind {
            #specific_ops
            _ => { ww_err(wr, d, #es, ErrorKindDiscriminants::Unimplemented) }
        }
    }
}

fn args_structs_recursive(
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
                #ser.map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
            }
        };

        let es0 = fixed_err_seq("DEFERRED_OUTPUT_FINISH");
        let es1 = fixed_err_seq("DEFERRED_EVENT_SER");
        quote! {
            let mut wr = BufWriter::new(scratch_args);
            #ser_output
            let output_bytes = wr.finish_and_take().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;

            ww_client_server::util::ser_value_event(scratch_event, #seq_path, output_bytes)
                .map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))
        }
    } else {
        let es = fixed_err_seq("DEFERRED_EVENT_SER");
        quote! {
            let _ = scratch_args;
            ww_client_server::util::ser_unit_return_event(scratch_event, #seq_path).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))
        }
    }
}

/// [des_args] of a `no_alloc` server (SRV-51): the arguments struct decoded by the shared
/// `ww_client_server::dispatch::des`, one copy per type.
fn des_args_compact(
    mod_ident: &Ident,
    method_ident: &Ident,
    args: &[ArgumentOwned],
    es: TokenStream,
) -> (TokenStream, TokenStream) {
    if args.is_empty() {
        return (quote! {}, quote! {});
    }
    let args_struct_ident = api_common::args_struct_ident(&method_ident.to_string(), true);
    let args_des = quote! {
        let Some(args) = ww_client_server::dispatch::des::<#mod_ident::#args_struct_ident>(args.as_slice()) else {
            return ww_err(wr, d, #es, ErrorKindDiscriminants::ArgsDesFailed);
        };
    };
    (args_des, args_list(args))
}

/// The handler's arguments from the decoded args struct `args`: its fields, or the struct itself when it is a
/// `#[flatten]` argument's (EVO-22).
fn args_list(args: &[ArgumentOwned]) -> TokenStream {
    if let [arg] = args
        && arg.is_flatten()
    {
        return quote! { args };
    }
    let idents = args
        .iter()
        .map(|arg| Ident::new(&arg.ident, Span::call_site()));
    quote! { #(args.#idents),* }
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
            let args = __ww_try!(wr, d, #mod_ident::#args_struct_ident::#des(&mut rd), #es, ArgsDesFailed);
        };
        (args_des, args_list(args))
    }
}

fn deferred_method_return_ser_methods(
    api_bundle: &CodegenBundle,
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
        let es_over_limit = fixed_err_seq("REPLY_OVER_LIMIT");
        // an unanswered call would wait for its timeout: a reply the sink refused is answered with the error instead
        let send_owned = |send: TokenStream, send_error: TokenStream| {
            quote! {
                let bytes = ww_owned_bytes(&output)?;
                let event = ww_value_event(seq, &bytes)?;
                let sent = #send;
                if let Err(wire_weaver::SendError::Sink) = sent {
                    _ = #send_error;
                }
                sent
            }
        };
        let ser_error = quote! {
            |scratch: &mut [u8]| -> Result<usize, ShrinkWrapError> {
                Ok(ser_err_event(scratch, seq, Error::response_ser_failed(#es_over_limit))?.len())
            }
        };
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
        let send_async = if !no_alloc && return_ty.is_some() {
            send_owned(
                quote! { wire_weaver::EventOut::send_reply_bytes(out, reply_to.priority, &event).await },
                quote! { wire_weaver::EventOut::send_reply_with(out, reply_to.priority, #ser_error).await },
            )
        } else {
            quote! { wire_weaver::EventOut::send_reply_with(out, reply_to.priority, #ser_event).await }
        };
        let send_blocking = if !no_alloc && return_ty.is_some() {
            send_owned(
                quote! { wire_weaver::BlockingEventOut::send_reply_bytes_blocking(out, reply_to.priority, &event) },
                quote! { wire_weaver::BlockingEventOut::send_reply_with_blocking(out, reply_to.priority, #ser_error) },
            )
        } else {
            quote! { wire_weaver::BlockingEventOut::send_reply_with_blocking(out, reply_to.priority, #ser_event) }
        };
        ts.extend(quote! {
            pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u32 #maybe_output) -> Result<&'i [u8], Error<'static>> {
                #ser_output_or_unit
            }

            /// Send the return value of a deferred call, `reply_to` is `cx.reply_to()` of the original call: the reply
            /// inherits its priority, an urgent one is flushed at once (SRV-42).
            pub async fn #send_fn_name<'i, M>(out: &mut impl wire_weaver::EventOut, reply_to: wire_weaver::ReplyTo<M> #maybe_output) -> Result<(), wire_weaver::SendError> {
                let seq = reply_to.seq;
                #send_async
            }

            /// Send the return value of a deferred call, `reply_to` is `cx.reply_to()` of the original call: the reply
            /// inherits its priority, an urgent one is flushed at once (SRV-42).
            pub fn #send_blocking_fn_name<'i, M>(out: &mut impl wire_weaver::BlockingEventOut, reply_to: wire_weaver::ReplyTo<M> #maybe_output) -> Result<(), wire_weaver::SendError> {
                let seq = reply_to.seq;
                #send_blocking
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
            if d.in_multi {
                __ww_try!(wr, d, wr.write(& #ident), #es, ResponseSerFailed);
            } else {
                __ww_try!(wr, d, #ident.ser_shrink_wrap(wr), #es, ResponseSerFailed);
            }
        }
    } else {
        quote! {
            __ww_try!(wr, d, #ident.ser_shrink_wrap(wr), #es, ResponseSerFailed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_version::VersionOwned;

    fn global_trait(crate_id: &str, trait_name: &str, path: &[u32]) -> GlobalTrait {
        GlobalTrait {
            path: path.to_vec(),
            crate_version: FullVersionOwned::new(crate_id.into(), VersionOwned::new(0, 3, 1)),
            trait_name: trait_name.into(),
            compact_id: None,
        }
    }

    #[test]
    fn global_table_has_hashes_not_names() {
        let table = global_full_table(&[
            global_trait("my_api", "Trait1", &[0]),
            global_trait("my_api", "Trait2", &[1, 2]),
        ])
        .unwrap()
        .to_string();
        let hash = ww_global::trait_hash("my_api", "Trait2");
        assert!(table.contains(&format!("{hash}u32")), "{table}");
        assert!(
            !table.contains("Trait2") && !table.contains("my_api"),
            "{table}"
        );
    }

    #[test]
    fn global_hash_collision_is_an_error() {
        // found by brute force: FNV-1a of "my_api", 0xff, then the trait name is 0x9d738fa3 for both
        assert_eq!(
            ww_global::trait_hash("my_api", "Trait268817"),
            ww_global::trait_hash("my_api", "Trait1018190")
        );
        let e = global_full_table(&[
            global_trait("my_api", "Trait268817", &[0]),
            global_trait("my_api", "Trait1018190", &[1]),
        ])
        .unwrap_err();
        assert!(
            e.contains("my_api::Trait1018190 and my_api::Trait268817"),
            "{e}"
        );
        assert!(e.contains("0x9d738fa3"), "{e}");
    }

    #[test]
    fn global_paths_auto_follows_introspection() {
        assert_eq!(GlobalPaths::Auto.resolve(true), GlobalPaths::None);
        assert_eq!(GlobalPaths::Auto.resolve(false), GlobalPaths::Full);
        assert_eq!(GlobalPaths::Compact.resolve(true), GlobalPaths::Compact);
        assert_eq!(GlobalPaths::parse("").unwrap(), GlobalPaths::Auto);
        assert!(GlobalPaths::parse("names").is_err());
    }
}
