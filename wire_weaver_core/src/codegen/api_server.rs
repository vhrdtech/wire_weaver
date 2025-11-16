use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use syn::{Lit, LitInt};

use crate::ast::Type;
use crate::ast::api::{
    ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument, Multiplicity, PropertyAccess,
};
use crate::ast::path::Path;
use crate::codegen::api_common;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{add_prefix, maybe_quote};
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};

pub fn impl_server_dispatcher(
    api_level: &ApiLevel,
    no_alloc: bool,
    use_async: bool,
    method_model: &MethodModel,
    property_model: &PropertyModel,
    context_ident: &Ident,
    handler_ident: &Ident,
) -> TokenStream {
    let stream_send_methods = stream_ser_methods(api_level, no_alloc);
    let additional_use = maybe_quote(
        no_alloc,
        quote! { use wire_weaver::shrink_wrap::{RefVec, RefVecIter}; },
    );
    let maybe_async = maybe_quote(use_async, quote! { async });
    let maybe_await = maybe_quote(use_async, quote! { .await });
    let deferred_return_methods =
        deferred_method_return_ser_methods(api_level, no_alloc, method_model);
    let external_crate_name = match &api_level.source_location {
        ApiLevelSourceLocation::File { part_of_crate, .. } => part_of_crate,
        ApiLevelSourceLocation::Crate { crate_name, .. } => crate_name,
    };
    let cx = ApiServerCGContext {
        ident_prefix: None,
        no_alloc,
        use_async,
        method_model,
        property_model,
    };
    let process_request_inner = process_request_inner_recursive(
        Ident::new("process_request_inner", Span::call_site()),
        api_level,
        IndexChain::new(),
        Some(external_crate_name),
        &cx,
    );
    let mod_doc = &api_level.docs;
    let args_structs = args_structs_recursive(api_level, Some(external_crate_name), no_alloc);
    let use_external =
        api_level.use_external_types(Path::new_ident(external_crate_name.clone()), no_alloc);
    quote! {
        #mod_doc
        mod api_impl {
            #args_structs

            use wire_weaver::shrink_wrap::{
                DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter,
                Error as ShrinkWrapError, nib32::UNib32, ElementSize
            };
            use ww_client_server::{Request, RequestKind, Event, EventKind, PathKind, Error, StreamSidebandCommand, util::{ser_ok_event, ser_err_event, ser_unit_return_event}};
            #additional_use
            #use_external

            impl super::#context_ident {
                /// Returns an Error only if request deserialization or error serialization failed.
                /// If there are any other errors, they are returned to the remote caller.
                pub #maybe_async fn #handler_ident<'a>(
                    &mut self,
                    bytes: &[u8],
                    scratch_args: &'a mut [u8],
                    scratch_event: &'a mut [u8],
                    scratch_err: &'a mut [u8]
                ) -> Result<&'a [u8], ShrinkWrapError> {
                    let mut rd = BufReader::new(bytes);
                    let request = Request::des_shrink_wrap(&mut rd)?;
                    // if matches!(request.kind, RequestKind::Read) && request.seq == 0 { // TODO: Move to property read
                    //     return Ok(ser_err_event(scratch_err, request.seq, Error::ReadPropertyWithSeqZero).map_err(|_| Error::ResponseSerFailed)?)
                    // }
                    // TODO: handle trait paths on server side
                    let PathKind::Absolute { path } = &request.path_kind else {
                        let mut wr = BufWriter::new(scratch_err);
                        let event = Event { seq: request.seq, result: Err(Error::PathKindNotSupported) };
                        event.ser_shrink_wrap(&mut wr)?;
                        return Ok(wr.finish_and_take()?);
                    };
                    let mut path_iter = path.iter();
                    match self.process_request_inner(path.clone(), &mut path_iter, &request, scratch_args, scratch_event)#maybe_await {
                        Ok(response_bytes) => Ok(response_bytes),
                        Err(e) => {
                            let mut wr = BufWriter::new(scratch_err);
                            let event = Event {
                                seq: request.seq,
                                result: Err(e)
                            };
                            event.ser_shrink_wrap(&mut wr)?;
                            Ok(wr.finish_and_take()?)
                        }
                    }
                }

                #process_request_inner

                #deferred_return_methods
            }

            #stream_send_methods
        }
    }
}

#[derive(Clone)]
struct ApiServerCGContext<'i> {
    ident_prefix: Option<Ident>,
    no_alloc: bool,
    use_async: bool,
    method_model: &'i MethodModel,
    property_model: &'i PropertyModel,
}

fn process_request_inner_recursive(
    ident: Ident,
    api_level: &ApiLevel,
    index_chain: IndexChain,
    ext_crate_name: Option<&Ident>,
    cx: &ApiServerCGContext<'_>,
) -> TokenStream {
    let maybe_async = maybe_quote(cx.use_async, quote! { async });
    let level_matchers = level_matchers(api_level, index_chain, ext_crate_name, cx);
    let maybe_index_chain_def = index_chain.fun_argument_def();

    let mut ts = quote! {
        #maybe_async fn #ident<'a>(
            &mut self,
            #maybe_index_chain_def
            path: RefVec<'_, UNib32>,
            path_iter: &mut RefVecIter<'_, UNib32>,
            request: &Request<'_>,
            scratch_args: &'a mut [u8],
            scratch_event: &'a mut [u8],
        ) -> Result<&'a [u8], Error> {
            match path_iter.next() {
                #level_matchers
                None => {
                    match request.kind {
                        // RequestKind::Version => { Err(Error::OperationNotImplemented) },
                        // RequestKind::Introspect { Err(Error::OperationNotImplemented) },
                        _ => { Err(Error::OperationNotSupported) },
                    }
                }
            }
        }
    };

    for item in &api_level.items {
        let ApiItemKind::ImplTrait { args, level } = &item.kind else {
            continue;
        };
        let level = level.as_ref().expect("empty level");
        let process_fn_name = Ident::new(
            format!(
                "process_{}",
                args.trait_name.to_string().to_case(Case::Snake)
            )
            .as_str(),
            Span::call_site(),
        );
        let mut cx = cx.clone();
        cx.ident_prefix = Some(Ident::new(
            args.trait_name.to_string().to_case(Case::Snake).as_str(),
            Span::call_site(),
        ));
        ts.extend(process_request_inner_recursive(
            process_fn_name,
            level,
            index_chain,
            args.location.crate_name().as_ref(),
            &cx,
        ));
    }
    ts
}

fn level_matchers(
    api_level: &ApiLevel,
    mut index_chain: IndexChain,
    ext_crate_name: Option<&Ident>,
    cx: &ApiServerCGContext<'_>,
) -> TokenStream {
    let ids = api_level.items.iter().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u32", item.id).as_str(),
            Span::call_site(),
        ))
    });
    let handlers = api_level.items.iter().map(|item| match item.multiplicity {
        Multiplicity::Flat => level_matcher(
            &item.kind,
            index_chain,
            api_level.mod_ident(ext_crate_name),
            cx,
        ),
        Multiplicity::Array { .. } => {
            let check_err_on_no_alloc = if cx.no_alloc {
                quote! { .map_err(|_| Error::ArrayIndexDesFailed)?.0 }
            } else {
                quote! { .0 }
            };
            let maybe_index_chain_push =
                index_chain.push_back( quote! { }, quote! { path_iter.next().ok_or(Error::ExpectedArrayIndexGotNone)?#check_err_on_no_alloc });
            let lm = level_matcher(
                &item.kind,
                index_chain,
                api_level.mod_ident(ext_crate_name),
                cx,
            );
            quote! {
                #maybe_index_chain_push
                #lm
            }
        }
    });
    let check_err_on_no_alloc = if cx.no_alloc {
        quote! { .map_err(|_| Error::PathDesFailed)?.0 }
    } else {
        quote! { .0 }
    };
    quote! {
        Some(id) => match id #check_err_on_no_alloc {
            #(#ids => { #handlers } ),*
            _ => { Err(Error::BadPath) }
        }
    }
}

fn level_matcher(
    kind: &ApiItemKind,
    index_chain: IndexChain,
    mod_ident: Ident,
    cx: &ApiServerCGContext<'_>,
) -> TokenStream {
    match kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => handle_method(index_chain, &mod_ident, cx, ident, args, return_type),
        ApiItemKind::Property { access, ident, ty } => {
            handle_property(index_chain, cx, ident, ty, *access)
        }
        ApiItemKind::Stream { ident, ty, is_up } => {
            handle_stream(index_chain, cx, ident, ty, *is_up)
        }
        ApiItemKind::ImplTrait { args, .. } => {
            let process_fn_name = Ident::new(
                format!(
                    "process_{}",
                    args.trait_name.to_string().to_case(Case::Snake)
                )
                .as_str(),
                Span::call_site(),
            );
            let maybe_await = maybe_quote(cx.use_async, quote! { .await });
            let maybe_index_chain_arg = index_chain.fun_argument_call();
            quote! {
                Ok(self.#process_fn_name(#maybe_index_chain_arg path_iter, request, scratch_event, scratch_args)#maybe_await?)
            }
        }
    }
}

fn handle_method(
    index_chain: IndexChain,
    mod_ident: &Ident,
    cx: &ApiServerCGContext,
    ident: &Ident,
    args: &[Argument],
    return_type: &Option<Type>,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_let_output = maybe_quote(return_type.is_some(), quote! { let output = });
    let maybe_index_chain_arg = index_chain.fun_argument_call();

    let (args_des, args_list) = des_args(mod_ident, ident, args, cx.no_alloc);
    let is_args = if args.is_empty() {
        quote! { .. }
    } else {
        quote! { args }
    };

    let ser_output_or_unit =
        ser_method_output(mod_ident, ident, return_type, quote! { request.seq });
    let ident = add_prefix(cx.ident_prefix.as_ref(), ident);
    let call_and_handle_deferred = match cx.method_model.pick(ident.to_string().as_str()).unwrap() {
        MethodModelKind::Immediate => quote! {
            #maybe_let_output self.#ident(#maybe_index_chain_arg #args_list)#maybe_await;
            if request.seq != 0 {
                #ser_output_or_unit
            } else {
                Ok(&[])
            }
        },
        MethodModelKind::Deferred => quote! {
            let output = match self.#ident(#maybe_index_chain_arg request.seq, #args_list)#maybe_await {
                Some(o) => o,
                None => {
                    return Ok(&[])
                }
            };
            #ser_output_or_unit
        },
    };

    quote! {
        match &request.kind {
            RequestKind::Call { #is_args } => {
                #args_des
                #call_and_handle_deferred
            }
            RequestKind::Introspect => {
                Err(Error::OperationNotImplemented)
            }
            _ => {
                Err(Error::OperationNotSupported)
            }
        }
    }
}

fn handle_property(
    index_chain: IndexChain,
    cx: &ApiServerCGContext,
    ident: &Ident,
    ty: &Type,
    access: PropertyAccess,
) -> TokenStream {
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });
    let maybe_index_chain_arg = index_chain.fun_argument_call();
    let maybe_index_chain_indices = index_chain.array_indices();
    let mut des = TokenStream::new();
    ty.buf_read(
        &Ident::new("value", Span::call_site()),
        cx.no_alloc,
        quote! { .map_err(|_| Error::PropertyDesFailed)? },
        &mut des,
    );
    let property_model_pick = cx.property_model.pick(ident.to_string().as_str()).unwrap();
    let set_property = match property_model_pick {
        PropertyModelKind::GetSet => {
            let set_property = Ident::new(format!("set_{}", ident).as_str(), Span::call_site());
            quote! {
                self.#set_property(#maybe_index_chain_arg value)#maybe_await;
            }
        }
        PropertyModelKind::ValueOnChanged => {
            let on_property_changed =
                Ident::new(format!("on_{}_changed", ident).as_str(), Span::call_site());
            quote! {
                if self.#ident != value {
                    self.#ident = value;
                    self.#on_property_changed(#maybe_index_chain_arg)#maybe_await;
                }
            }
        }
    };
    let get_and_ser_property = match property_model_pick {
        PropertyModelKind::GetSet => {
            let get_property = Ident::new(format!("get_{}", ident).as_str(), Span::call_site());
            let mut ser = TokenStream::new();
            ty.buf_write(
                FieldPath::Value(quote! { value }),
                cx.no_alloc,
                quote! { .map_err(|_| Error::ResponseSerFailed)? },
                &mut ser,
            );
            quote! {
                let value = self.#get_property(#maybe_index_chain_arg)#maybe_await;
                let mut wr = BufWriter::new(scratch_args);
                #ser
            }
        }
        PropertyModelKind::ValueOnChanged => {
            let mut ser = TokenStream::new();
            ty.buf_write(
                FieldPath::Value(quote! { self.#ident #maybe_index_chain_indices }),
                cx.no_alloc,
                quote! { .map_err(|_| Error::ResponseSerFailed)? },
                &mut ser,
            );
            quote! {
                let mut wr = BufWriter::new(scratch_args);
                #ser
            }
        }
    };
    let write = quote! {
        RequestKind::Write { data } => {
            let data = data.as_slice();
            let mut rd = BufReader::new(data);
            #des
            #set_property
            if request.seq == 0 {
                Ok(&[])
            } else {
                Ok(ser_ok_event(scratch_event, request.seq, EventKind::Written).map_err(|_| Error::ResponseSerFailed)?)
            }
        }
    };
    let maybe_write = maybe_quote(
        matches!(
            access,
            PropertyAccess::WriteOnly | PropertyAccess::ReadWrite
        ),
        write,
    );
    let read = quote! {
        RequestKind::Read => {
            #get_and_ser_property
            let output_bytes = wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?;
            let kind = EventKind::ReadValue {
                    data: RefVec::Slice { slice: output_bytes }
                };
            Ok(ser_ok_event(scratch_event, request.seq, kind).map_err(|_| Error::ResponseSerFailed)?)
        }
    };
    let maybe_read = maybe_quote(
        matches!(
            access,
            PropertyAccess::Const | PropertyAccess::ReadOnly | PropertyAccess::ReadWrite
        ),
        read,
    );
    quote! {
        match &request.kind {
            #maybe_write
            #maybe_read
            _ => { Err(Error::OperationNotSupported) }
        }
    }
}

fn handle_stream(
    index_chain: IndexChain,
    cx: &ApiServerCGContext<'_>,
    ident: &Ident,
    ty: &Type,
    is_up: bool,
) -> TokenStream {
    let maybe_index_chain_call = index_chain.fun_argument_call();
    let maybe_await = maybe_quote(cx.use_async, quote! { .await });

    let sideband_fn = Ident::new(format!("{}_sideband", ident).as_str(), ident.span());
    let handle_sideband_cmd = quote! {
        // user fn returns Option<StreamSidebandEvent>
        let r = self.#sideband_fn(#maybe_index_chain_call sideband_cmd)#maybe_await;
        match r {
            Some(sideband_event) => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path, sideband_event })
                };
                Ok(event.to_ww_bytes(scratch_event).map_err(|_| Error::ResponseSerFailed)?)
            }
            None => {
                Err(Error::OperationNotImplemented)
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
        let write = Ident::new(format!("{}_write", ident).as_str(), ident.span());
        let (des_data, arg) = match ty {
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    (quote! {}, quote! { () })
                } else {
                    todo!("tuple")
                }
            }
            Type::Vec(inner) => {
                if matches!(inner.as_ref(), Type::U8) {
                    (quote! {}, quote! { data })
                } else {
                    todo!("vec of other")
                }
            }
            _ => {
                todo!("other")
            }
        };
        let maybe_comma = maybe_quote(!index_chain.is_empty(), quote! { , });
        quote! {
            RequestKind::Write { data } => {
                #des_data
                let r = self.#write(#maybe_index_chain_call #maybe_comma #arg)#maybe_await;
                Ok(&[]) // do not send acknowledgements on stream writes
            }
            RequestKind::StreamSideband { sideband_cmd } => {
                let sideband_cmd = *sideband_cmd;
                #handle_sideband_cmd
            }
        }
    };
    quote! {
        match &request.kind {
            #specific_ops
            _ => { Err(Error::OperationNotImplemented) }
        }
    }
}

fn args_structs_recursive(
    api_level: &ApiLevel,
    ext_crate_name: Option<&Ident>,
    no_alloc: bool,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let args_structs = api_common::args_structs(api_level, no_alloc);

    let mod_name = api_level.mod_ident(ext_crate_name);
    let use_external = api_level.use_external_types(
        ext_crate_name
            .map(|n| Path::new_ident(n.clone()))
            .unwrap_or(Path::new_path("super::super")),
        no_alloc,
    );
    ts.extend(quote! {
        mod #mod_name {
            use super::*;
            #use_external
            #args_structs
        }
    });
    for item in &api_level.items {
        let ApiItemKind::ImplTrait { args, level } = &item.kind else {
            continue;
        };
        let level = level.as_ref().expect("empty level");
        ts.extend(args_structs_recursive(
            level,
            args.location.crate_name().as_ref(),
            no_alloc,
        ));
    }
    ts
}

fn ser_method_output(
    mod_ident: &Ident,
    ident: &Ident,
    return_type: &Option<Type>,
    seq_path: TokenStream,
) -> TokenStream {
    if let Some(ty) = return_type {
        let ser_output = if matches!(ty, /*Type::Sized(_, _) |*/ Type::External(_, _)) {
            quote! { output.ser_shrink_wrap(&mut wr).map_err(|_| Error::ResponseSerFailed)?; }
        } else {
            let output_struct_name = Ident::new(
                format!("{}_output", ident).to_case(Case::Pascal).as_str(),
                Span::call_site(),
            );
            quote! {
                let output = #mod_ident::#output_struct_name {
                    output
                };
                output.ser_shrink_wrap(&mut wr).map_err(|_| Error::ResponseSerFailed)?;
            }
        };
        quote! {
            let mut wr = BufWriter::new(scratch_args);
            #ser_output
            let output_bytes = wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?;

            let mut event_wr = BufWriter::new(scratch_event);
            let event = Event {
                seq: #seq_path,
                result: Ok(EventKind::ReturnValue {
                    data: RefVec::Slice { slice: output_bytes }
                })
            };
            event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::ResponseSerFailed)?;
            Ok(event_wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?)
        }
    } else {
        quote! {
            Ok(ser_unit_return_event(scratch_event, request.seq).map_err(|_| Error::ResponseSerFailed)?)
        }
    }
}

fn des_args(
    mod_ident: &Ident,
    method_ident: &Ident,
    args: &[Argument],
    _no_alloc: bool,
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
        let args_des = quote! {
            let args = args.as_slice();
            let mut rd = BufReader::new(args);
            // TODO: Log _e ?
            let args = #mod_ident::#args_struct_ident::des_shrink_wrap(&mut rd).map_err(|_e| Error::ArgsDesFailed)?;
        };
        let idents = args.iter().map(|arg| &arg.ident);
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}

fn stream_ser_methods(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &api_level.items {
        let ApiItemKind::Stream { ident, ty, is_up } = &item.kind else {
            continue;
        };
        if !*is_up {
            continue;
        }
        let stream_ser_fn = Ident::new(format!("{}_data_ser", ident).as_str(), Span::call_site());
        let lifetimes = if ty.potential_lifetimes() {
            quote! { 'i, 'a }
        } else {
            quote! { 'a }
        };
        let ty = ty.def(no_alloc);

        let bytes_to_container = if no_alloc {
            quote! { RefVec::Slice { slice: value_bytes } }
        } else {
            quote! { Vec::from(value_bytes) }
        };

        // TODO: Handle other levels
        let id = item.id;
        let path = if no_alloc {
            quote! { RefVec::Slice { slice: &[UNib32(#id)] } }
        } else {
            quote! { vec![UNib32(#id)] }
        };
        ts.append_all(quote! {
            #[doc = "Serialize stream value, put it's bytes into Event with StreamUpdate kind and serialize it"]
            pub fn #stream_ser_fn<#lifetimes>(value: &#ty, scratch_value: &mut [u8], scratch_event: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
                let mut wr = BufWriter::new(scratch_value);
                value.ser_shrink_wrap(&mut wr)?;
                let value_bytes = wr.finish_and_take()?;

                let mut wr = BufWriter::new(scratch_event);
                let data = #bytes_to_container;
                let event = Event {
                    seq: 0,
                    result: Ok(EventKind::StreamData { path: #path, data })
                };
                event.ser_shrink_wrap(&mut wr)?;
                Ok(wr.finish_and_take()?)
            }
        });
    }
    ts
}

fn deferred_method_return_ser_methods(
    api_level: &ApiLevel,
    no_alloc: bool,
    method_model: &MethodModel,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let return_ty = if no_alloc {
        quote! { &'i [u8] }
    } else {
        quote! { Vec<u8> }
    };
    for item in &api_level.items {
        let ApiItemKind::Method {
            ident, return_type, ..
        } = &item.kind
        else {
            continue;
        };
        if method_model.pick(ident.to_string().as_str()).unwrap() != MethodModelKind::Deferred {
            continue;
        }
        let fn_name = Ident::new(
            format!("{}_ser_return_event", ident).as_str(),
            Span::call_site(),
        );
        let ser_output_or_unit = ser_method_output(
            &api_level.mod_ident(None),
            ident,
            return_type,
            quote! { seq },
        );
        let maybe_output = match return_type {
            Some(ty) => {
                let ty = ty.arg_pos_def(no_alloc);
                quote! { , output: #ty }
            }
            None => quote! {},
        };
        ts.append_all(quote! {
            pub fn #fn_name<'i>(scratch_args: &'i mut [u8], scratch_event: &'i mut [u8], seq: u16 #maybe_output) -> Result<#return_ty, Error> {
                #ser_output_or_unit
            }
        });
    }
    ts
}
