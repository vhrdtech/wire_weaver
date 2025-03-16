use convert_case::Casing;
use proc_macro2::{Span, TokenStream};
use quote::{TokenStreamExt, quote};
use syn::{Lit, LitInt};

use crate::ast::Type;
use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::codegen::api_common;
use crate::codegen::ty::FieldPath;

pub fn server_dispatcher(
    api_level: &ApiLevel,
    api_model_location: &Option<syn::Path>,
    no_alloc: bool,
    use_async: bool,
) -> TokenStream {
    let args_structs = api_common::args_structs(api_level, no_alloc);
    let level_matchers = level_matchers(api_level, no_alloc, use_async);
    let ser_event = ser_event(no_alloc);
    let stream_send_methods = stream_ser_methods(api_level, no_alloc);
    let additional_use = if no_alloc {
        quote! { use wire_weaver::shrink_wrap::vec::RefVec; }
    } else {
        quote! {}
    };
    let api_model_includes = if let Some(api_model_location) = api_model_location {
        quote! {
            use #api_model_location::{Request, RequestKind, Event, EventKind, Error};
        }
    } else {
        quote! {}
    };
    let (maybe_async, maybe_await) = if use_async {
        (quote! { async }, quote! { .await })
    } else {
        (quote! {}, quote! {})
    };
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib16::Nib16
        };
        #api_model_includes
        #additional_use

        impl Context {
            /// Returns an Error only if request deserialization or error serialization failed.
            /// If there are any other errors, they are returned to the remote caller.
            pub #maybe_async fn process_request<'a>(
                &'a mut self,
                bytes: &[u8],
                error_scratch: &'a mut [u8]
            ) -> Result<&[u8], ShrinkWrapError> {
                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd, ElementSize::Implied)?;

                match self.process_request_inner(&request)#maybe_await {
                    Ok(response_bytes) => Ok(response_bytes),
                    Err(e) => {
                        let mut wr = BufWriter::new(error_scratch);
                        let event = Event {
                            seq: request.seq,
                            result: Err(e)
                        };
                        event.ser_shrink_wrap(&mut wr)?;
                        Ok(wr.finish_and_take()?)
                    }
                }
            }

            #maybe_async fn process_request_inner(
                &mut self,
                request: &Request<'_>,
            ) -> Result<&[u8], Error> {
                let mut path_iter = request.path.iter();
                match path_iter.next() {
                    #level_matchers
                    None => {
                        match request.kind {
                            RequestKind::Version => { Err(Error::OperationNotImplemented) },
                            // RequestKind::Heartbeat => {
                            //     Err(Error::Unimplemented)
                            // },
                            _ => { Err(Error::OperationNotSupported) },
                        }
                    }
                }
            }

            #ser_event
        }

        #stream_send_methods
    }
}

fn level_matchers(api_level: &ApiLevel, no_alloc: bool, use_async: bool) -> TokenStream {
    let ids = api_level.items.iter().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u16", item.id).as_str(),
            Span::call_site(),
        ))
    });
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_matcher(&item.kind, no_alloc, use_async));
    let check_err_on_no_alloc = if no_alloc {
        quote! { id.map_err(|_| Error::PathDesFailed)?.0 }
    } else {
        quote! { id.0 }
    };
    quote! {
        Some(id) => match #check_err_on_no_alloc {
            #(#ids => { #handlers } ),*
            _ => { Err(Error::BadPath) }
        }
    }
}

fn level_matcher(kind: &ApiItemKind, no_alloc: bool, use_async: bool) -> TokenStream {
    let maybe_await = if use_async {
        quote! { .await }
    } else {
        quote! {}
    };
    match kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => {
            let (args_des, args_list) = des_args(ident, args, no_alloc);
            let is_args = if args.is_empty() {
                quote! { .. }
            } else {
                quote! { args }
            };

            let maybe_let_output = if return_type.is_some() {
                quote! { let output = }
            } else {
                quote! {}
            };
            let ser_output_or_unit = if let Some(ty) = return_type {
                let ser_output = if matches!(ty, Type::Sized(_, _) | Type::Unsized(_, _)) {
                    quote! { wr.write(&output).map_err(|_| Error::ResponseSerFailed)?; }
                } else {
                    let output_struct_name = Ident::new(
                        format!("{}_output", ident.sym).to_case(convert_case::Case::Pascal),
                    );
                    quote! {
                        let output = #output_struct_name {
                            output: output
                        };
                        wr.write(&output).map_err(|_| Error::ResponseSerFailed)?;
                    }
                };
                quote! {
                    let mut wr = BufWriter::new(&mut self.output_scratch);
                    #ser_output
                    let output_bytes = wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?;

                    let mut event_wr = BufWriter::new(&mut self.event_scratch);
                    let event = Event {
                        seq: request.seq,
                        result: Ok(EventKind::ReturnValue {
                            data: RefVec::Slice { slice: output_bytes, element_size: ElementSize::Sized { size_bits: 8 } }
                        })
                    };
                    event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::ResponseSerFailed)?;
                    Ok(event_wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?)
                }
            } else {
                quote! {
                    Ok(self.ser_unit_return_event(request.seq).map_err(|_| Error::ResponseSerFailed)?)
                }
            };

            quote! {
                match &request.kind {
                    RequestKind::Call { #is_args } => {
                        #args_des
                        #maybe_let_output self.#ident(#args_list)#maybe_await;
                        #ser_output_or_unit
                    }
                    RequestKind::Introspect => {
                        Err(Error::OperationNotImplemented)
                    }
                    _ => {
                        Err(Error::OperationNotImplemented)
                    }
                }
            }
        }
        ApiItemKind::Property { ident, ty } => {
            let get_slice = get_slice(Ident::new("data"), no_alloc);
            let mut des = TokenStream::new();
            ty.buf_read(
                proc_macro2::Ident::new("value", Span::call_site()),
                no_alloc,
                quote! { .map_err(|_| Error::PropertyDesFailed)? },
                &mut des,
            );
            let mut ser = TokenStream::new();
            ty.buf_write(
                FieldPath::Value(quote! { self.#ident }),
                no_alloc,
                quote! { .map_err(|_| Error::ResponseSerFailed)? },
                &mut ser,
            );
            let on_property_changed = Ident::new(format!("on_{}_changed", ident.sym));
            quote! {
                match &request.kind {
                    RequestKind::Write { data } => {
                        let data = #get_slice;
                        let mut rd = BufReader::new(data);
                        #des
                        if self.#ident != value {
                            self.#ident = value;
                            self.#on_property_changed()#maybe_await;
                        }
                        if request.seq == 0 {
                            Ok(&[])
                        } else {
                            Ok(Self::ser_ok_event(&mut self.event_scratch, request.seq, EventKind::Written).map_err(|_| Error::ResponseSerFailed)?)
                        }
                    }
                    RequestKind::Read => {
                        let mut wr = BufWriter::new(&mut self.output_scratch);
                        #ser
                        let output_bytes = wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?;
                        let kind = EventKind::ReadValue {
                                data: RefVec::Slice { slice: output_bytes, element_size: ElementSize::Sized { size_bits: 8 } }
                            };
                        Ok(Self::ser_ok_event(&mut self.event_scratch, request.seq, kind).map_err(|_| Error::ResponseSerFailed)?)

                        // let mut event_wr = BufWriter::new(&mut self.event_scratch);
                        // let event = Event {
                        //     seq: request.seq,
                        //     result: Ok(EventKind::ReadValue {
                        //         data: RefVec::Slice { slice: output_bytes, element_size: ElementSize::Sized { size_bits: 8 } }
                        //     })
                        // };
                        // event.ser_shrink_wrap(&mut event_wr).map_err(|_| Error::ResponseSerFailed)?;
                        // Ok(event_wr.finish_and_take().map_err(|_| Error::ResponseSerFailed)?)
                    }
                    _ => { Err(Error::OperationNotSupported) }
                }
            }
        }
        ApiItemKind::Stream {
            ident: _,
            ty: _,
            is_up,
        } => {
            let specific_ops = if *is_up {
                quote! {
                    RequestKind::ChangeRate { shaper_config: _ } => {
                        Err(Error::OperationNotImplemented)
                    }
                }
            } else {
                quote! {
                    RequestKind::Write { data } => {
                        Err(Error::OperationNotImplemented)
                    }
                }
            };
            quote! {
                match &request.kind {
                    RequestKind::OpenStream => { Err(Error::OperationNotImplemented) }
                    RequestKind::CloseStream => { Err(Error::OperationNotImplemented) }
                    #specific_ops
                    _ => { Err(Error::OperationNotImplemented) }
                }
            }
        }
        // ApiItemKind::ImplTrait => {}
        // ApiItemKind::Level(_) => {}
        _ => unimplemented!(),
    }
}

fn des_args(method_ident: &Ident, args: &[Argument], no_alloc: bool) -> (TokenStream, TokenStream) {
    let args_struct_ident =
        format!("{}_args", method_ident.sym).to_case(convert_case::Case::Pascal);
    let args_struct_ident = Ident::new(args_struct_ident);
    if args.is_empty() {
        (quote! {}, quote! {})
    } else {
        let get_slice = get_slice(Ident::new("args"), no_alloc);
        let args_des = quote! {
            let args = #get_slice;
            let mut rd = BufReader::new(args);
            // TODO: Log _e ?
            let args: #args_struct_ident = rd.read(ElementSize::Implied).map_err(|_e| Error::ArgsDesFailed)?;
        };
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}

fn get_slice(ref_vec_or_vec: Ident, no_alloc: bool) -> TokenStream {
    if no_alloc {
        quote! {
            match #ref_vec_or_vec.byte_slice() {
                Ok(slice) => slice,
                Err(_e) => {
                    return Err(Error::ArgsDesFailed);
                }
            }
        }
    } else {
        quote! { #ref_vec_or_vec.as_slice() }
    }
}

/// generates ser_unit_return_event(seq)
fn ser_event(no_alloc: bool) -> TokenStream {
    let future_compatible_unit_return = if no_alloc {
        quote! { RefVec::Slice { slice: &[0x00], element_size: ElementSize::Sized { size_bits: 8 } } }
    } else {
        quote! { vec![0] }
    };
    quote! {
        fn ser_ok_event<'b>(scratch: &'b mut [u8], seq: u16, kind: EventKind) -> Result<&'b [u8], ShrinkWrapError> {
            let mut wr = BufWriter::new(scratch);
            let event = Event {
                seq,
                result: Ok(kind)
            };
            event.ser_shrink_wrap(&mut wr)?;
            Ok(wr.finish_and_take()?)
        }

        fn ser_err_event(&mut self, seq: u16, error: Error) -> Result<&[u8], ShrinkWrapError> {
            let mut wr = BufWriter::new(&mut self.event_scratch);
            let event = Event {
                seq,
                result: Err(error)
            };
            event.ser_shrink_wrap(&mut wr)?;
            Ok(wr.finish_and_take()?)
        }

        fn ser_unit_return_event(&mut self, seq: u16) -> Result<&[u8], ShrinkWrapError> {
            if seq == 0 {
                return Ok(&[]);
            }
            let mut wr = BufWriter::new(&mut self.event_scratch);
            let event = Event {
                seq,
                result: Ok(EventKind::ReturnValue { data: #future_compatible_unit_return })
            };
            event.ser_shrink_wrap(&mut wr)?;
            Ok(wr.finish_and_take()?)
        }
    }
}

// fn ser_heartbeat(api_model_location: &syn::Path) -> TokenStream {
//     quote! {
//         pub fn ser_heartbeat<'a>(&'a mut self, scratch: &'a mut [u8]) -> &[u8] {
//             use wire_weaver::shrink_wrap::vec::RefVec;
//             use wire_weaver::shrink_wrap::{SerializeShrinkWrap, traits::ElementSize};
//             use #api_model_location::{Event, EventKind};
//
//             let data = RefVec::Slice {
//                 slice: &[0xaa, 0xbb],
//                 element_size: ElementSize::Sized { size_bits: 8 }
//             };
//             let event = Event {
//                 seq: 123,
//                 result: Ok(EventKind::Heartbeat { data })
//             };
//             self.ser_event(&event, scratch)
//         }
//     }
// }

fn stream_ser_methods(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &api_level.items {
        let ApiItemKind::Stream { ident, ty, is_up } = &item.kind else {
            continue;
        };
        if !*is_up {
            continue;
        }
        let stream_ser_fn = proc_macro2::Ident::new(
            format!("{}_stream_ser", ident.sym).as_str(),
            Span::call_site(),
        );
        let lifetimes = if ty.potential_lifetimes() {
            quote! { 'i, 'a }
        } else {
            quote! { 'a }
        };
        let ty = ty.def(no_alloc);

        let bytes_to_container = if no_alloc {
            quote! { RefVec::Slice { slice: value_bytes, element_size: ElementSize::Sized { size_bits: 8 } } }
        } else {
            quote! { Vec::from(value_bytes) }
        };

        // TODO: Handle other levels
        let id = item.id;
        let path = if no_alloc {
            quote! { RefVec::Slice { slice: &[Nib16(#id)], element_size: ElementSize::UnsizedSelfDescribing } }
        } else {
            quote! { vec![Nib16(#id)] }
        };
        // TODO: Make this more efficient and not use 2 buffers?
        ts.append_all(quote! {
            #[doc = "Serialize stream value, put it's bytes into Event with StreamUpdate kind and serialize it"]
            pub fn #stream_ser_fn<#lifetimes>(value: &#ty, scratch_value: &mut [u8], scratch: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
                let mut wr = BufWriter::new(scratch_value);
                value.ser_shrink_wrap(&mut wr)?;
                let value_bytes = wr.finish_and_take()?;

                let mut wr = BufWriter::new(scratch);
                let data = #bytes_to_container;
                let event = Event {
                    seq: 0,
                    result: Ok(EventKind::StreamUpdate { path: #path, data })
                };
                event.ser_shrink_wrap(&mut wr)?;
                Ok(wr.finish_and_take()?)
            }
        });
    }
    ts
}
