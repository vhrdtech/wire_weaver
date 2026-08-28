//! # Implementation details:
//! * Server's index chain contains only array indices on the way to a resource
use crate::codegen::index_chain::IndexChain;
use crate::codegen::server::stream::stream_ser_methods_recursive;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::{ErrorSeq, add_prefix, maybe_quote, maybe_quote_cl};
use crate::codegen::{api_common, util};
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt, Path};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Multiplicity,
    PropertyAccess, TypeOwned,
};

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
    /// Path to the user struct on which server dispatcher method will be implemented (process_request_bytes).
    /// Dispatcher expects all the user handlers also to be implemented on this struct by user code.
    pub server_struct_path: String,
    /// Generate ww_self introspect bytes, fully describing all API methods and data types used.
    pub generate_introspect: bool,
    /// Generate multi read, multi write and multi call support code.
    /// Takes a bit more FLASH, but allows for more efficient requests in some cases.
    pub multi_req: bool,
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
    /// Path to the user struct on which server dispatcher method will be implemented (process_request_bytes).
    /// Dispatcher expects all the user handlers also to be implemented on this struct by user code.
    pub server_struct_path: Path,
    /// Generate ww_self introspect bytes, fully describing all API methods and data types used.
    pub generate_introspect: bool,
    /// Generate multi read, multi write and multi call support code.
    /// Takes a bit more FLASH, but allows for more efficient requests in some cases.
    pub multi_req: bool,
}

impl From<GenServerConfig> for GenServerConfigRaw {
    fn from(config: GenServerConfig) -> Self {
        Self {
            no_alloc: config.no_alloc,
            use_async: config.use_async,
            method_model: config.method_model,
            property_model: config.property_model,
            server_struct_path: super::util::str_to_path(&config.server_struct_path),
            generate_introspect: config.generate_introspect,
            multi_req: config.multi_req,
        }
    }
}

/// Generates API server code for the given API bundle and configuration.
/// ApiBundleOwned can be loaded using [crate::load] or [crate::load_dep].
/// Pass a [GenServerConfig] or [GenServerConfigRaw] to configure code generation.
///
/// Alternatively, use [wire_weaver_derive::ww_codegen] proc-macro if you do not want to use build.rs.
pub fn gen_server(
    api_bundle: &ApiBundleOwned,
    config: impl Into<GenServerConfigRaw>,
) -> TokenStream {
    let config: GenServerConfigRaw = config.into();
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
    let crate_name = api_level.crate_name(api_bundle).unwrap();
    let cx = ApiServerCGContext {
        ident_prefix: None,
        no_alloc: config.no_alloc,
        use_async: config.use_async,
        property_model: &config.property_model,
        multi_req: config.multi_req,
    };
    let (handle_introspect, api_signature) = super::server::introspect::introspect(
        api_bundle,
        config.generate_introspect,
        cx.use_async,
        &mut error_seq,
    );
    let process_request_inner = process_request_inner_recursive(
        "root".into(),
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
        crate_name,
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
    let (es1, es2) = (error_seq.next(), error_seq.next());
    let server_struct_path = config.server_struct_path;
    let maybe_use_ser_shrink_wrap = maybe_quote_cl(cx.multi_req, || quote! { false, });
    // let maybe_use_write = maybe_quote_cl(cx.multi_req, || quote! { true, });
    quote! {
        #args_structs

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
            WrittenOk,
            WrittenErr,
            Deferred
        }

        impl #server_struct_path {
            /// Returns an Error only if request deserialization or error serialization failed.
            /// If there are any other errors, they are sent to the remote.
            pub #maybe_async fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch_args: &'a mut [u8],
                scratch_event: &'a mut [u8],
                scratch_err: &'a mut [u8],
                msg_tx: &mut impl wire_weaver::MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd)?;

                let mut wr = BufWriter::new(scratch_event);
                let event_builder = EventBuilder::new(request.seq, &mut wr)?;

                // TODO: handle trait paths on server side
                let PathKind::Absolute { path } = &request.path_kind else {
                    wr.write(&Error::new(#es1, ErrorKind::PathKindNotSupported))?;
                    event_builder.finish(false, &mut wr);
                    return wr.finish_and_take();
                };

                const MAX_DEPTH: usize = 16; // TODO: calculate max depth
                let mut path_arr = [UNib32(0); MAX_DEPTH];
                let mut path_len: usize = 0;
                for (idx, resource_id) in path.iter().enumerate() {
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
                let before_event_kind = wr.save_state();
                let event_kind_builder = EventKindBuilder::new(&mut wr)?;
                match &request.kind {
                    _ => {
                        match self.process_root(path, &mut iter, &request, &mut wr, #maybe_use_ser_shrink_wrap msg_tx)#maybe_await {
                            Ok(WrAction::WrittenOk) => {
                                if request.seq == 0 {
                                    return Ok(&[])
                                }
                                event_kind_builder.finish_with_kind(request.kind.discriminants(), &mut wr);
                                event_builder.finish(true, &mut wr);
                                wr.finish_and_take()
                            }
                            Ok(WrAction::WrittenErr) => {
                                event_builder.finish(false, &mut wr);
                                wr.finish_and_take()
                            }
                            Ok(WrAction::Deferred) => {
                                Ok(&[])
                            }
                            Err(e) => {
                                wr.restore_state(before_event_kind); // handler could have started to write to wr but failed
                                wr.write(&e)?;
                                event_builder.finish(false, &mut wr);
                                wr.finish_and_take()
                            }
                        }
                    }
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
                        todo!()
                        // let (mut builder, mut wr) =
                        //     shrink_wrap::either_any_vec::EitherAnyVecBuilder::new(scratch_event);
                        // for index_or_glob in multi_idx.iter() {
                        //     path_arr[path_len] = index_or_glob;
                        //     let path_len = if let Some(resource_id) = in_each_array_id {
                        //         path_arr[path_len + 1] = ResourceIndexKind::Index(resource_id.0);
                        //         path_len + 2
                        //     } else {
                        //         path_len + 1
                        //     };
                        //     let path = &path_arr[..path_len];
                        //     let mut iter = path.iter();
                        //     let marker = builder.write_item_start(&mut wr)?;
                        //     match self
                        //         .process_root(path, &mut iter, &request, scratch_args, &mut wr, msg_tx)
                        //         .await
                        //     {
                        //         Ok(r) => {
                        //             builder.write_item_finish(marker, true, &mut wr);
                        //         }
                        //         Err(e) => {
                        //             // nothing was written to wr
                        //             wr.write(&e)?;
                        //             builder.write_item_finish(marker, false, &mut wr);
                        //         }
                        //     }
                        // }
                        // let data = builder.finish_and_take(wr)?;
                    }

                }
            }

            #process_request_inner

            #deferred_return_methods
        }

        #stream_send_methods
    }
}

#[derive(Clone)]
struct ApiServerCGContext<'i> {
    ident_prefix: Option<String>,
    no_alloc: bool,
    use_async: bool,
    property_model: &'i PropertyModel,
    multi_req: bool,
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

fn process_request_inner_recursive(
    level_name_chain: String,
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
    let maybe_use_read = maybe_quote_cl(cx.multi_req, || quote! { use_write: bool, });
    let mut ts = quote! {
        #maybe_async fn #process_fn_name<'a>(
            &mut self,
            #maybe_index_chain_def
            path: &[UNib32],
            path_iter: &mut Iter<'_, UNib32>,
            request: &Request<'_>,
            wr: &mut BufWriter<'_>,
            #maybe_use_read
            msg_tx: &mut impl wire_weaver::MessageSink,
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
        ts.extend(process_request_inner_recursive(
            level_name_chain,
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

fn level_matchers(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    level_name_chain: &str,
    index_chain: IndexChain,
    crate_name: &str,
    cx: &ApiServerCGContext<'_>,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let ids = api_level.items.iter().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u32", item.id.0).as_str(),
            Span::call_site(),
        ))
    });
    let es = error_seq.next();
    let handlers = api_level.items.iter().map(|item| match &item.multiplicity {
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
                            #ser_indices
                            Ok(WrAction::WrittenOk)
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
            #(#ids => { #handlers } ),*
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
        ApiItemKindOwned::Method { args, return_ty } => handle_method(
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
        ApiItemKindOwned::Stream { ty, is_up } => {
            handle_stream(api_bundle, index_chain, cx, &ident, ty, *is_up, error_seq)
        }
        ApiItemKindOwned::Trait { .. } => {
            let process_fn_name = Ident::new(
                format!("process_{level_name_chain}_{}", api_item.ident).as_str(),
                Span::call_site(),
            );
            let maybe_await = maybe_quote(cx.use_async, quote! { .await });
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            let maybe_use_write = maybe_quote_cl(cx.multi_req, || quote! { use_write, });
            quote! {
                Ok(self.#process_fn_name(#maybe_index_chain_arg path, path_iter, request, wr, #maybe_use_write msg_tx)#maybe_await?)
            }
        }
    }
}

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

    let ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let es = error_seq.next();
    let ser_output = ser_value(cx.multi_req, quote! { output }, error_seq);
    quote! {
        match &request.kind {
            RequestKind::Call { #maybe_args } => {
                #args_des
                match self.#ident(msg_tx, #maybe_index_chain_arg #args_list)#maybe_await {
                    RpcResult::Ready(output) => {
                        if request.seq != 0 {
                            #maybe_enforce_ty
                            #ser_output
                        }
                        Ok(WrAction::WrittenOk)
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
                    quote! {
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #enforce_ty::des_shrink_wrap(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        match self.#set_property(#maybe_index_chain_arg value)#maybe_await {
                            SetResult::Set => {
                                Ok(WrAction::WrittenOk)
                            },
                            SetResult::SetError(user_err) => {
                                // if request.seq != 0 {
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
                    let changed_property = Ident::new(
                        format!("changed_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    let es = error_seq.next();
                    quote! {
                        let mut rd = BufReader::new(data.as_slice());
                        let value = #enforce_ty::des_shrink_wrap(&mut rd).map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))?;
                        if self.#prefixed_ident #maybe_index_chain_indices != value {
                            self.#prefixed_ident #maybe_index_chain_indices = value;
                            self.#changed_property(#maybe_index_chain_arg)#maybe_await;
                        }
                        Ok(WrAction::WrittenOk)
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
            PropertyAccess::Const
                | PropertyAccess::ReadOnly { .. }
                | PropertyAccess::ReadWrite { .. }
        ),
        || {
            let es = error_seq.next();
            let ser_value = ser_value(cx.multi_req, quote! { value }, error_seq);
            let get_and_ser_property = match property_model_pick {
                PropertyModelKind::GetSet => {
                    let get_property = Ident::new(
                        format!("get_{}", prefixed_ident).as_str(),
                        Span::call_site(),
                    );
                    quote! {
                        match self.#get_property(#maybe_index_chain_arg)#maybe_await {
                            GetResult::Value(value) => {
                                let value: #enforce_ty = value;
                                #ser_value
                                Ok(WrAction::WrittenOk)
                            }
                            GetResult::GetError(user_err) => {
                                #ser_user_err
                                Ok(WrAction::WrittenErr)
                            }
                            GetResult::Unimplemented => {
                                Err(Error::unimplemented(#es))
                            }
                        }
                    }
                }
                PropertyModelKind::ValueOnChanged => {
                    quote! {
                        let value: &#enforce_ty = &self.#prefixed_ident #maybe_index_chain_indices;
                        #ser_value
                        Ok(WrAction::WrittenOk)
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

fn handle_stream(
    api_bundle: &ApiBundleOwned,
    index_chain: IndexChain,
    cx: &ApiServerCGContext<'_>,
    ident: &Ident,
    ty: &TypeOwned,
    is_up: bool,
    err_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_index_chain_call = index_chain.fun_argument_call();
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });

    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), &ident);
    let sideband_fn = Ident::new(
        format!("sideband_{}", prefixed_ident).as_str(),
        ident.span(),
    );
    let es = err_seq.next();
    let handle_sideband = quote! {
        // user fn returns Option<StreamSideband>
        let r = self.#sideband_fn(msg_tx, #maybe_index_chain_call *sideband)#maybe_await;
        match r {
            Some(sideband) => {
                wr.write(&RefVec::Slice { slice: &path }).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                wr.write(&sideband).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
                Ok(WrAction::WrittenOk)
            }
            None => {
                Ok(WrAction::Deferred)
            }
        }
    };
    let specific_ops = if is_up {
        // stream (device out)
        quote! {
            RequestKind::StreamSideband { sideband } => {
                #handle_sideband
            }
        }
    } else {
        // sink (device in)
        let mut other_des = || {
            let es = err_seq.next();
            let enforce_ty = ty_def(api_bundle, ty, false, TyPos::Annotation).unwrap();
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
            _ => other_des(),
        };
        quote! {
            RequestKind::Write { data } => {
                #des_data
                self.#write(#maybe_index_chain_call #arg)#maybe_await;
                Ok(WrAction::WrittenOk) // TODO: ?? do not send acknowledgements on stream writes
            }
            RequestKind::StreamSideband { sideband } => {
                #handle_sideband
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
                seq: #seq_path,
                result: Ok(EventKind::Value {
                    data: RefVec::Slice { slice: output_bytes }
                })
            };
            event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::response_ser_failed(#es1))?;
            Ok(event_wr.finish_and_take().map_err(|_| Error::response_ser_failed(#es2))?)
        }
    } else {
        let es = errors_seq.next();
        quote! {
            Ok(ser_unit_return_event(scratch_event, request.seq).map_err(|_| Error::response_ser_failed(#es))?)
        }
    }
}

fn des_args(
    mod_ident: &Ident,
    method_ident: &Ident,
    args: &[ArgumentOwned],
    _no_alloc: bool,
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    let args_struct_ident = Ident::new(
        format!("{}_args", method_ident)
            .to_case(Case::Pascal)
            .as_str(),
        Span::call_site(),
    );
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
        ts.extend(quote! {
            pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u16 #maybe_output) -> Result<#byte_return_ty, Error> {
                #ser_output_or_unit
            }
        });
    }
    ts
}

fn ser_value(multi_req: bool, ident: TokenStream, error_seq: &mut ErrorSeq) -> TokenStream {
    let es = error_seq.next();
    if multi_req {
        quote! {
            if use_write {
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
