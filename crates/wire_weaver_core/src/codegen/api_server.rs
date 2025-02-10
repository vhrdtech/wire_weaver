use convert_case::Casing;
use proc_macro2::{Span, TokenStream};
use quote::{quote, TokenStreamExt};
use syn::{Lit, LitInt};

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::codegen::api_common;

pub fn server_dispatcher(
    api_level: &ApiLevel,
    api_model_location: &syn::Path,
    no_alloc: bool,
) -> TokenStream {
    let args_structs = api_common::args_structs(api_level, no_alloc);
    let level_matchers = level_matchers(api_level, no_alloc);
    let ser_event = ser_event(no_alloc);
    let stream_send_methods = stream_ser_methods(api_level, no_alloc);
    // let ser_heartbeat = ser_heartbeat(api_model_location);
    let err_not_implemented = err_to_caller("OperationNotImplemented");
    let err_not_supported = err_to_caller("OperationNotSupported");
    let additional_use = if no_alloc {
        quote! { use wire_weaver::shrink_wrap::vec::RefVec; }
    } else {
        quote! {}
    };
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib16::Nib16
        };
        use #api_model_location::{Request, RequestKind, Event, EventKind, Error};
        #additional_use

        impl Context {
            pub async fn process_request<'a>(
                &'a mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
            ) -> Result<&[u8], ShrinkWrapError> {
                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd, ElementSize::Implied)?;
                let mut path_iter = request.path.iter();
                match path_iter.next() {
                    #level_matchers
                    None => {
                        match request.kind {
                            RequestKind::Version => { #err_not_implemented },
                            // RequestKind::Heartbeat => {
                            //     Err(Error::Unimplemented)
                            // },
                            _ => { #err_not_supported },
                        }
                    }
                }
            }

            #ser_event
            // #ser_heartbeat
        }

        #stream_send_methods
    }
}

fn level_matchers(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let ids = api_level.items.iter().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u16", item.id).as_str(),
            Span::call_site(),
        ))
    });
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_matcher(&item.kind, no_alloc));
    let err_bad_path = err_to_caller("BadPath");
    let check_err_on_no_alloc = if no_alloc {
        quote! { id?.0 }
    } else {
        quote! { id.0 }
    };
    quote! {
        Some(id) => match #check_err_on_no_alloc {
            #(#ids => { #handlers } ),*
            _ => { #err_bad_path }
        }
    }
}

fn level_matcher(kind: &ApiItemKind, no_alloc: bool) -> TokenStream {
    let err_bad_path = err_to_caller("BadPath");
    let err_not_implemented = err_to_caller("OperationNotImplemented");
    match kind {
        ApiItemKind::Method { ident, args } => {
            let (args_des, args_list) = des_args(ident, args, no_alloc);
            let is_args = if args.is_empty() {
                quote! { .. }
            } else {
                quote! { args }
            };
            quote! {
                match &request.kind {
                    RequestKind::Call { #is_args } => {
                        #args_des
                        self.#ident(#args_list).await;
                        // TODO: Implement return type & serdes
                        Ok(Self::ser_void_args_event(request.seq, scratch)?)
                    }
                    RequestKind::Introspect => {
                        #err_not_implemented
                    }
                    _ => {
                        #err_not_implemented
                    }
                }
            }
        }
        // ApiItemKind::Property => {}
        ApiItemKind::Stream {
            ident: _,
            ty: _,
            is_up,
        } => {
            let specific_ops = if *is_up {
                quote! {
                    RequestKind::ChangeRate { shaper_config: _ } => {
                        #err_not_implemented
                    }
                }
            } else {
                quote! {
                    RequestKind::Write { data } => {
                        #err_not_implemented
                    }
                }
            };
            quote! {
                match &request.kind {
                    RequestKind::OpenStream => { #err_not_implemented }
                    RequestKind::CloseStream => { #err_not_implemented }
                    #specific_ops
                    _ => { #err_bad_path }
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
        let err_args_des_failed = err_to_caller("ArgsDesFailed");
        let get_slice = if no_alloc {
            quote! {
                match args.byte_slice() {
                    Ok(args) => args,
                    Err(_e) => {
                        return #err_args_des_failed;
                    }
                }
            }
        } else {
            quote! { args.as_slice() }
        };
        let args_des = quote! {
            let args = #get_slice;
            let mut rd = BufReader::new(args);
            let args: #args_struct_ident = match rd.read(ElementSize::Implied) {
                Ok(args) => args,
                Err(_e) => {
                    return #err_args_des_failed;
                }
            };
        };
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}
fn ser_event(no_alloc: bool) -> TokenStream {
    let future_compatible_void_return = if no_alloc {
        quote! { RefVec::Slice { slice: &[0x00], element_size: ElementSize::Sized { size_bits: 8 } } }
    } else {
        quote! { vec![0] }
    };
    quote! {
        pub fn ser_ok_event<'a>(seq: u16, kind: EventKind, scratch: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
            let mut wr = BufWriter::new(scratch);
            let event = Event {
                seq,
                result: Ok(kind)
            };
            event.ser_shrink_wrap(&mut wr)?;
            Ok(wr.finish_and_take()?)
        }

        pub fn ser_err_event(seq: u16, error: Error, scratch: &mut [u8]) -> Result<&[u8], ShrinkWrapError> {
            let mut wr = BufWriter::new(scratch);
            let event = Event {
                seq,
                result: Err(error)
            };
            event.ser_shrink_wrap(&mut wr)?;
            Ok(wr.finish_and_take()?)
        }

        pub fn ser_void_args_event(seq: u16, scratch: &mut [u8]) -> Result<&[u8], ShrinkWrapError> {
            let mut wr = BufWriter::new(scratch);
            let event = Event {
                seq,
                result: Ok(EventKind::ReturnValue { data: #future_compatible_void_return })
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
        let path = if no_alloc {
            quote! { RefVec::Slice { slice: &[Nib16(1)], element_size: ElementSize::UnsizedSelfDescribing } }
        } else {
            quote! { vec![Nib16(1)] }
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

fn err_to_caller(err: &str) -> TokenStream {
    let err = proc_macro2::Ident::new(err, Span::call_site());
    quote! {
        Ok(Self::ser_err_event(request.seq, Error::#err, scratch)?)
    }
}
