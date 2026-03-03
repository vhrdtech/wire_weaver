//! # Implementation details:
//! * Server's index chain contains only array indices on the way to a resource
use crate::codegen::api_common;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::server::stream::stream_ser_methods_recursive;
use crate::codegen::ty_def::ty_def;
use crate::codegen::util::{add_prefix, maybe_quote, ErrorSeq};
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, ArgumentOwned, Multiplicity, PropertyAccess,
    TypeOwned,
};

pub fn impl_server_dispatcher(
    api_bundle: &ApiBundleOwned,
    no_alloc: bool,
    use_async: bool,
    method_model: &MethodModel,
    property_model: &PropertyModel,
    context_ident: &Ident,
    handler_ident: &Ident,
    generate_introspect: bool,
) -> TokenStream {
    let additional_use = maybe_quote(
        no_alloc,
        quote! { use wire_weaver::shrink_wrap::{RefVec, RefVecIter}; },
    );
    let maybe_async = maybe_quote(use_async, quote! { async });
    let maybe_await = maybe_quote(use_async, quote! { .await });
    let mut error_seq = ErrorSeq::default();
    let api_level = &api_bundle.root;
    // let deferred_return_methods =
    //     deferred_method_return_ser_methods(api_level, no_alloc, method_model, &mut error_seq);
    let crate_name = api_level.crate_name(api_bundle).unwrap();
    let cx = ApiServerCGContext {
        ident_prefix: None,
        no_alloc,
        use_async,
        method_model,
        property_model,
    };
    // let (handle_introspect, api_signature) = super::server::introspect::introspect(
    //     api_level,
    //     generate_introspect,
    //     cx.use_async,
    //     &mut error_seq,
    // );
    let process_request_inner = process_request_inner_recursive(
        Ident::new("process_request_inner", Span::call_site()),
        api_bundle,
        api_level,
        IndexChain::new(),
        crate_name,
        &cx,
        &mut error_seq,
        // Some(handle_introspect),
        None,
    );
    let stream_send_methods =
        stream_ser_methods_recursive(api_level, IndexChain::new(), crate_name, no_alloc, true);
    let args_structs = args_structs_recursive(api_bundle, api_level, crate_name, no_alloc);
    // let use_external = api_level.use_external_types(Path::new_ident(crate_name.clone()), no_alloc);
    let es = error_seq.next_err();
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter,
            Error as ShrinkWrapError, nib32::UNib32, ElementSize
        };
        use ww_client_server::{Request, RequestKind, Event, EventKind, PathKind, Error, ErrorKind, StreamSidebandCommand, util::{ser_ok_event, ser_err_event, ser_unit_return_event}};
        #additional_use
        // #use_external
        // #api_signature

        impl super::#context_ident {
            /// Returns an Error only if request deserialization or error serialization failed.
            /// If there are any other errors, they are returned to the remote caller.
            pub #maybe_async fn #handler_ident<'a>(
                &mut self,
                bytes: &[u8],
                scratch_args: &'a mut [u8],
                scratch_event: &'a mut [u8],
                scratch_err: &'a mut [u8],
                msg_tx: &mut impl wire_weaver::MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd)?;
                // if matches!(request.kind, RequestKind::Read) && request.seq == 0 { // TODO: Move to property read
                //     return Ok(ser_err_event(scratch_err, request.seq, Error::ReadPropertyWithSeqZero).map_err(|_| Error::ResponseSerFailed)?)
                // }
                // TODO: handle trait paths on server side
                let PathKind::Absolute { path } = &request.path_kind else {
                    let mut wr = BufWriter::new(scratch_err);
                    let event = Event { seq: request.seq, result: Err(Error::new(#es, ErrorKind::PathKindNotSupported)) };
                    event.ser_shrink_wrap(&mut wr)?;
                    return wr.finish_and_take();
                };
                let mut path_iter = path.iter();
                match self.process_request_inner(path.clone(), &mut path_iter, &request, scratch_args, scratch_event, msg_tx)#maybe_await {
                    Ok(response_bytes) => Ok(response_bytes),
                    Err(e) => {
                        let mut wr = BufWriter::new(scratch_err);
                        let event = Event {
                            seq: request.seq,
                            result: Err(e)
                        };
                        event.ser_shrink_wrap(&mut wr)?;
                        wr.finish_and_take()
                    }
                }
            }

            #process_request_inner

            // #deferred_return_methods
        }

        #stream_send_methods
    }
}

#[derive(Clone)]
struct ApiServerCGContext<'i> {
    ident_prefix: Option<String>,
    no_alloc: bool,
    use_async: bool,
    method_model: &'i MethodModel,
    property_model: &'i PropertyModel,
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
    ident: Ident,
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
        index_chain,
        crate_name,
        cx,
        error_seq,
    );
    let maybe_index_chain_def = index_chain.fun_argument_def();

    let es = error_seq.next_err();
    let mut ts = quote! {
        #maybe_async fn #ident<'a>(
            &mut self,
            #maybe_index_chain_def
            path: RefVec<'_, UNib32>,
            path_iter: &mut RefVecIter<'_, UNib32>,
            request: &Request<'_>,
            scratch_args: &'a mut [u8],
            scratch_event: &'a mut [u8],
            msg_tx: &mut impl wire_weaver::MessageSink,
        ) -> Result<&'a [u8], Error<'_>> {
            match path_iter.next() {
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
        let process_fn_name = Ident::new(
            format!("process_{}", level.trait_name.to_case(Case::Snake)).as_str(),
            Span::call_site(),
        );
        let mut cx = cx.clone();
        cx.push_suffix(&item.ident);
        let mut index_chain = index_chain;
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
        ts.extend(process_request_inner_recursive(
            process_fn_name,
            api_bundle,
            level,
            index_chain,
            api_level.crate_name(api_bundle).unwrap(),
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
    let es0 = error_seq.next_err();
    let es1 = error_seq.next_err();
    let handlers = api_level.items.iter().map(|item| match &item.multiplicity {
        Multiplicity::Flat => level_matcher(
            api_bundle,
            &item.ident,
            &item.kind,
            index_chain,
            mod_ident(api_level, crate_name),
            cx,
            crate_name,
            error_seq,
        ),
        Multiplicity::Array { index_type_idx } => {
            let check_err_on_no_alloc = if cx.no_alloc {
                let es = error_seq.next_err();
                quote! { .map_err(|_| Error::new(#es, ErrorKind::ArrayIndexDesFailed))? }
            } else {
                quote! { }
            };
            let es = error_seq.next_err();
            let mut index_chain = index_chain;
            let maybe_index_chain_push =
                index_chain.push_back(
                    quote! { },
                    quote! { path_iter.next().ok_or(Error::new(#es, ErrorKind::ExpectedArrayIndexGotNone))?#check_err_on_no_alloc }
                );
            let lm = level_matcher(
                api_bundle,
                &item.ident,
                &item.kind,
                index_chain,
                mod_ident(api_level, crate_name),
                cx,
                crate_name,
                error_seq,
            );
            let maybe_validate_index = if index_type_idx.is_some() {
                quote! { }
            } else {
                let validate_index_fn = Ident::new(
                    format!(
                        "validate_index_{}",
                        item.ident.to_case(Case::Snake)
                    )
                        .as_str(),
                    Span::call_site(),
                );
                quote! {
                    if let Err(_) = self.#validate_index_fn(index_chain) {
                        return Err(Error::new(#es, ErrorKind::BadIndex));
                    }
                }
            };
            quote! {
                #maybe_index_chain_push
                #maybe_validate_index
                #lm
            }
        }
    });
    let check_err_on_no_alloc = if cx.no_alloc {
        quote! { .map_err(|_| Error::new(#es0, ErrorKind::PathDesFailed))?.0 }
    } else {
        quote! { .0 }
    };
    quote! {
        Some(id) => match id #check_err_on_no_alloc {
            #(#ids => { #handlers } ),*
            _ => { Err(Error::bad_path(#es1)) }
        }
    }
}

fn level_matcher(
    api_bundle: &ApiBundleOwned,
    ident: &str,
    kind: &ApiItemKindOwned,
    index_chain: IndexChain,
    mod_ident: Ident,
    cx: &ApiServerCGContext<'_>,
    crate_name: &str,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let ident = Ident::new(ident, Span::call_site());
    match kind {
        ApiItemKindOwned::Method { args, return_ty } => handle_method(
            api_bundle,
            index_chain,
            &mod_ident,
            cx,
            &ident,
            args,
            return_ty,
            crate_name,
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
            crate_name,
            error_seq,
        ),
        ApiItemKindOwned::Stream { ty, is_up } => handle_stream(
            api_bundle,
            index_chain,
            cx,
            &ident,
            ty,
            *is_up,
            crate_name,
            error_seq,
        ),
        ApiItemKindOwned::Trait { .. } => {
            let process_fn_name = Ident::new(
                format!("process_{}", ident.to_string()).as_str(),
                Span::call_site(),
            );
            let maybe_await = maybe_quote(cx.use_async, quote! { .await });
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            quote! {
                Ok(self.#process_fn_name(#maybe_index_chain_arg path, path_iter, request, scratch_event, scratch_args, msg_tx)#maybe_await?)
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
    crate_name: &str,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_let_output = if let Some(ty) = return_type {
        let enforce_ty = ty_def(api_bundle, ty, false, true).unwrap();
        quote! {
            let output: #enforce_ty =
        }
    } else {
        quote! {}
    };
    // let maybe_let_output = maybe_quote(return_type.is_some(), quote! { let output = });
    let maybe_index_chain_arg = index_chain.fun_argument_call();

    let (args_des, args_list) = des_args(mod_ident, ident, args, cx.no_alloc, error_seq);
    let is_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args }
    };

    let ser_output_or_unit = ser_method_output(
        mod_ident,
        ident,
        return_type,
        quote! { request.seq },
        error_seq,
    );
    let ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let call_and_handle_deferred = match cx.method_model.pick(ident.to_string().as_str()).unwrap() {
        MethodModelKind::Immediate => quote! {
            #maybe_let_output self.#ident(msg_tx, #maybe_index_chain_arg #args_list)#maybe_await;
            if request.seq != 0 {
                #ser_output_or_unit
            } else {
                Ok(&[])
            }
        },
        MethodModelKind::Deferred => quote! {
            let output = match self.#ident(msg_tx, #maybe_index_chain_arg request.seq, #args_list)#maybe_await {
                Some(o) => o,
                None => {
                    return Ok(&[])
                }
            };
            #ser_output_or_unit
        },
    };

    let es = error_seq.next_err();
    quote! {
        match &request.kind {
            RequestKind::Call { #is_args } => {
                #args_des
                #call_and_handle_deferred
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
    user_result_ty: &Option<TypeOwned>,
    access: PropertyAccess,
    crate_name: &str,
    error_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_index_chain_arg = index_chain.fun_argument_call();
    let maybe_index_chain_indices = index_chain.array_indices();
    // let mut des = TokenStream::new();
    // let es = error_seq.next_err();
    let enforce_ty = ty_def(api_bundle, ty, false, true).unwrap();
    // ty.buf_read(
    //     &Ident::new("value", Span::call_site()),
    //     cx.no_alloc,
    //     false,
    //     quote! { .map_err(|_| Error::new(#es, ErrorKind::PropertyDesFailed))? },
    //     &enforce_ty,
    //     &mut des,
    // );
    let property_model_pick = cx.property_model.pick(ident.to_string().as_str()).unwrap();
    let prefixed_ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let maybe_let_user_result = maybe_quote(user_result_ty.is_some(), quote! { let user_result = });
    let (es0, es1, es2, es3) = (
        error_seq.next_err(),
        error_seq.next_err(),
        error_seq.next_err(),
        error_seq.next_err(),
    );
    let maybe_ret_user_result = maybe_quote(
        user_result_ty.is_some(),
        quote! {
            if user_result.is_err() && request.seq != 0 {
                let mut wr = BufWriter::new(scratch_args);
                user_result.ser_shrink_wrap(&mut wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                let user_err_bytes = wr.finish_and_take().map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                return Ok(
                    ser_err_event(
                        scratch_event,
                        request.seq,
                        Error::new(#es2, ErrorKind::UserBytes(RefVec::new_bytes(user_err_bytes)))
                    ).map_err(|_| Error::new(#es3, ErrorKind::ResponseSerFailed))?
                );
            }
        },
    );
    let set_property = match property_model_pick {
        PropertyModelKind::GetSet => {
            let set_property = Ident::new(
                format!("set_{}", prefixed_ident).as_str(),
                Span::call_site(),
            );
            quote! {
                #maybe_let_user_result self.#set_property(#maybe_index_chain_arg value)#maybe_await;
                #maybe_ret_user_result
            }
        }
        PropertyModelKind::ValueOnChanged => {
            let on_property_changed = Ident::new(
                format!("on_{}_changed", prefixed_ident).as_str(),
                Span::call_site(),
            );
            quote! {
                if self.#prefixed_ident != value {
                    self.#prefixed_ident = value;
                    #maybe_let_user_result self.#on_property_changed(#maybe_index_chain_arg)#maybe_await;
                    #maybe_ret_user_result
                }
            }
        }
    };
    let get_and_ser_property = match property_model_pick {
        PropertyModelKind::GetSet => {
            let get_property = Ident::new(
                format!("get_{}", prefixed_ident).as_str(),
                Span::call_site(),
            );
            let es = error_seq.next_err();
            quote! {
                let value: #enforce_ty = self.#get_property(#maybe_index_chain_arg)#maybe_await;
                let mut wr = BufWriter::new(scratch_args);
                value.ser_shrink_wrap(&mut wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
            }
        }
        PropertyModelKind::ValueOnChanged => {
            let mut ser = TokenStream::new();
            let es = error_seq.next_err();
            quote! {
                let mut wr = BufWriter::new(scratch_args);
                #ser
                self.#prefixed_ident #maybe_index_chain_indices
                    .ser_shrink_wrap(&mut wr).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?;
            }
        }
    };
    let es0 = error_seq.next_err();
    let es1 = error_seq.next_err();
    let write = quote! {
        RequestKind::Write { data } => {
            let data = data.as_slice();
            let mut rd = BufReader::new(data);
            let value: #enforce_ty = rd.des_shrink_wrap(&mut rd).map_err(|_| Error::new(#es0, ErrorKind::PropertyDesFailed))?;
            #set_property
            if request.seq == 0 {
                Ok(&[])
            } else {
                Ok(ser_ok_event(scratch_event, request.seq, EventKind::Written).map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?)
            }
        }
    };
    let maybe_write = maybe_quote(
        matches!(
            access,
            PropertyAccess::WriteOnly | PropertyAccess::ReadWrite { .. }
        ),
        write,
    );
    let es0 = error_seq.next_err();
    let es1 = error_seq.next_err();
    let read = quote! {
        RequestKind::Read => {
            #get_and_ser_property
            let output_bytes = wr.finish_and_take().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
            let kind = EventKind::ReadValue {
                    data: RefVec::Slice { slice: output_bytes }
                };
            Ok(ser_ok_event(scratch_event, request.seq, kind).map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?)
        }
    };
    let maybe_read = maybe_quote(
        matches!(
            access,
            PropertyAccess::Const
                | PropertyAccess::ReadOnly { .. }
                | PropertyAccess::ReadWrite { .. }
        ),
        read,
    );
    let es = error_seq.next_err();
    quote! {
        match &request.kind {
            #maybe_write
            #maybe_read
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
    crate_name: &str,
    err_seq: &mut ErrorSeq,
) -> TokenStream {
    let maybe_index_chain_call = index_chain.fun_argument_call();
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });

    let sideband_fn = Ident::new(format!("{}_sideband", ident).as_str(), ident.span());
    let es = err_seq.next_err();
    let handle_sideband_cmd = quote! {
        // user fn returns Option<StreamSidebandEvent>
        let r = self.#sideband_fn(msg_tx, #maybe_index_chain_call sideband_cmd)#maybe_await;
        match r {
            Some(sideband_event) => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path, sideband_event })
                };
                Ok(event.to_ww_bytes(scratch_event).map_err(|_| Error::new(#es, ErrorKind::ResponseSerFailed))?)
            }
            None => {
                Ok(&[])
            }
        }
    };

    let specific_ops = if is_up {
        // stream (device out)
        quote! {
            RequestKind::ChangeRate { .. } | RequestKind::StreamSideband { .. } => {
                let sideband_cmd = match &request.kind {
                    RequestKind::ChangeRate { shaper_config } => StreamSidebandCommand::ChangeRate(*shaper_config),
                    RequestKind::StreamSideband { sideband_cmd } => *sideband_cmd,
                    _ => unreachable!()
                };
                #handle_sideband_cmd
            }
        }
    } else {
        // sink (device in)
        let mut other_des = || {
            let es = err_seq.next_err();
            let enforce_ty = ty_def(api_bundle, ty, false, true).unwrap();
            let mut ts = quote! {
                let mut rd = BufReader::new(data);
                let value: #enforce_ty = rd.des_shrink_wrap(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
            };
            (ts, quote! { value })
        };
        let write = Ident::new(format!("{}_write", ident).as_str(), ident.span());
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
        let maybe_comma = maybe_quote(!index_chain.is_empty(), quote! { , });
        quote! {
            RequestKind::Write { data } => {
                #des_data
                self.#write(#maybe_index_chain_call #maybe_comma #arg)#maybe_await;
                Ok(&[]) // do not send acknowledgements on stream writes
            }
            RequestKind::StreamSideband { sideband_cmd } => {
                let sideband_cmd = *sideband_cmd;
                #handle_sideband_cmd
            }
        }
    };
    let es = err_seq.next_err();
    quote! {
        match &request.kind {
            #specific_ops
            _ => { Err(Error::new(#es, ErrorKind::OperationNotImplemented)) }
        }
    }
}

fn args_structs_recursive(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    crate_name: &str,
    no_alloc: bool,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let args_structs = api_common::args_structs(api_bundle, api_level, no_alloc);

    // let mod_name = api_level.mod_ident(crate_name);
    // let use_external = api_level.use_external_types(Path::new_ident(crate_name.clone()), no_alloc);
    ts.extend(quote! {
        mod todo {
            use super::*;
            // #use_external
            #args_structs
        }
    });
    for item in &api_level.items {
        let ApiItemKindOwned::Trait { .. } = &item.kind else {
            continue;
        };
        let level = item.get_as_level(api_bundle).unwrap();
        let crate_name = level.crate_name(api_bundle).unwrap();
        ts.extend(args_structs_recursive(
            api_bundle, level, crate_name, no_alloc,
        ));
    }
    ts
}

fn ser_method_output(
    _mod_ident: &Ident,
    _ident: &Ident,
    return_type: &Option<TypeOwned>,
    seq_path: TokenStream,
    errors_seq: &mut ErrorSeq,
) -> TokenStream {
    if let Some(_ty) = return_type {
        let es = errors_seq.next_err();
        let ser_output = quote! { output.ser_shrink_wrap(&mut wr).map_err(|_| Error::response_ser_failed(#es))?; };
        // let ser_output = if matches!(ty, /*Type::Sized(_, _) |*/ Type::External(_, _)) {
        //     quote! { output.ser_shrink_wrap(&mut wr).map_err(|_| Error::response_ser_failed(#es))?; }
        // } else {
        //     let output_struct_name = Ident::new(
        //         format!("{}_output", ident).to_case(Case::Pascal).as_str(),
        //         Span::call_site(),
        //     );
        //     quote! {
        //         let output = #mod_ident::#output_struct_name {
        //             output
        //         };
        //         output.ser_shrink_wrap(&mut wr).map_err(|_| Error::response_ser_failed(#es))?;
        //     }
        // };
        let es0 = errors_seq.next_err();
        let es1 = errors_seq.next_err();
        let es2 = errors_seq.next_err();
        quote! {
            let mut wr = BufWriter::new(scratch_args);
            #ser_output
            let output_bytes = wr.finish_and_take().map_err(|_| Error::response_ser_failed(#es0))?;

            let mut event_wr = BufWriter::new(scratch_event);
            let event = Event {
                seq: #seq_path,
                result: Ok(EventKind::ReturnValue {
                    data: RefVec::Slice { slice: output_bytes }
                })
            };
            event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::response_ser_failed(#es1))?;
            Ok(event_wr.finish_and_take().map_err(|_| Error::response_ser_failed(#es2))?)
        }
    } else {
        let es = errors_seq.next_err();
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
        let es = error_seq.next_err();
        let args_des = quote! {
            let args = args.as_slice();
            let mut rd = BufReader::new(args);
            // TODO: Log _e ?
            let args = #mod_ident::#args_struct_ident::des_shrink_wrap(&mut rd).map_err(|_e| Error::new(#es, ErrorKind::ArgsDesFailed))?;
        };
        let idents = args.iter().map(|arg| &arg.ident);
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}

// fn deferred_method_return_ser_methods(
//     api_level: &ApiLevelOwned,
//     no_alloc: bool,
//     method_model: &MethodModel,
//     error_seq: &mut ErrorSeq,
// ) -> TokenStream {
//     let mut ts = TokenStream::new();
//     let return_ty = if no_alloc {
//         quote! { &'i [u8] }
//     } else {
//         quote! { Vec<u8> }
//     };
//     for item in &api_level.items {
//         let ApiItemKindOwned::Method { return_ty, .. } = &item.kind else {
//             continue;
//         };
//         if method_model.pick(&item.ident).unwrap() != MethodModelKind::Deferred {
//             continue;
//         }
//         let fn_name = Ident::new(
//             format!("{}_ser_return_event", ident).as_str(),
//             Span::call_site(),
//         );
//         let ser_output_or_unit = ser_method_output(
//             mod_ident(api_level, crate_name),
//             &api_level.mod_ident(api_level.source_location.crate_name()),
//             ident,
//             return_ty,
//             quote! { seq },
//             error_seq,
//         );
//         let maybe_output = match return_type {
//             Some(ty) => {
//                 let ty = ty.arg_pos_def(no_alloc);
//                 quote! { , output: #ty }
//             }
//             None => quote! {},
//         };
//         ts.append_all(quote! {
//             pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u16 #maybe_output) -> Result<#return_ty, Error> {
//                 #ser_output_or_unit
//             }
//         });
//     }
//     ts
// }
