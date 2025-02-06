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
    let ser_event = ser_event(api_model_location);
    let ser_heartbeat = ser_heartbeat(api_model_location);
    quote! {
        #args_structs

        impl Context {
            pub async fn process_request<'a>(
                    &'a mut self,
                    bytes: &[u8],
                    scratch: &'a mut [u8],
            ) -> &[u8] {
                use wire_weaver::shrink_wrap::{
                    DeserializeShrinkWrap, nib16::Nib16, buf_reader::BufReader, traits::ElementSize
                };
                use #api_model_location::{Request, RequestKind, Event, EventKind, Error};

                let mut rd = BufReader::new(bytes);
                let request = match Request::des_shrink_wrap(&mut rd, ElementSize::Implied) {
                    Ok(r) => r,
                    Err(e) => {
                        // TODO: only log if enabled
                        defmt::error!("Request deserialize fail: {}", e);
                        // TODO: count error if enabled
                        return &[];
                    }
                };
                let mut path_iter = request.path.iter();
                match path_iter.next() {
                    #level_matchers
                    None => {
                        match request.kind {
                            RequestKind::Version => {
                                // send version
                            },
                            RequestKind::Heartbeat => { },
                            _ => {
                                // send error
                            }
                        }
                    }
                }
                &[]
            }

            #ser_event
            #ser_heartbeat
        }
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
    quote! {
        #(Some(Ok(Nib16(#ids))) => { #handlers } ),*
        Some(Ok(_)) => {
            // send BadUri
        }
        Some(Err(_e)) => {
            // send error
        }
    }
}

fn level_matcher(kind: &ApiItemKind, no_alloc: bool) -> TokenStream {
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
                    }
                    RequestKind::Introspect => {

                    }
                    _ => {
                        // return error
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
                    RequestKind::ChangeRate { _shaper_config } => {}
                }
            } else {
                quote! {
                    RequestKind::Write { data } => {}
                }
            };
            quote! {
                match &request.kind {
                    RequestKind::OpenStream => {}
                    RequestKind::CloseStream => {}
                    #specific_ops
                }
            }
        }
        // ApiItemKind::ImplTrait => {}
        // ApiItemKind::Level(_) => {}
        _ => unimplemented!(),
    }
}

fn des_args(
    method_ident: &Ident,
    args: &[Argument],
    _no_alloc: bool,
) -> (TokenStream, TokenStream) {
    let args_struct_ident =
        format!("{}_args", method_ident.sym).to_case(convert_case::Case::Pascal);
    let args_struct_ident = Ident::new(args_struct_ident);
    if args.is_empty() {
        (quote! {}, quote! {})
    } else {
        let args_des = quote! {
            // TODO: send error back instead
            let mut rd = BufReader::new(args.byte_slice().unwrap());
            let args: #args_struct_ident = rd.read(ElementSize::Implied).unwrap();
        };
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });
        let args_list = quote! { #(args.#idents),* };
        (args_des, args_list)
    }
}
fn ser_event(api_model_location: &syn::Path) -> TokenStream {
    quote! {
        pub fn ser_event<'a>(&'a mut self, event: & #api_model_location::Event, scratch: &'a mut [u8]) -> &[u8] {
            use wire_weaver::shrink_wrap::SerializeShrinkWrap;

            let mut wr = wire_weaver::shrink_wrap::buf_writer::BufWriter::new(scratch);
            if let Err(e) = event.ser_shrink_wrap(&mut wr) {
                defmt::error!("send_event serialize: {}", e);
                return &[];
            }
            match wr.finish_and_take() {
                Ok(event_bytes) => {
                    event_bytes
                },
                Err(e) => {
                    defmt::error!("ser_event wr.finish(): {}", e);
                    &[]
                }
            }
        }
    }
}

fn ser_heartbeat(api_model_location: &syn::Path) -> TokenStream {
    quote! {
        pub fn ser_heartbeat<'a>(&'a mut self, scratch: &'a mut [u8]) -> &[u8] {
            use wire_weaver::shrink_wrap::vec::RefVec;
            use wire_weaver::shrink_wrap::{SerializeShrinkWrap, traits::ElementSize};
            use #api_model_location::{Event, EventKind};

            let data = RefVec::Slice {
                slice: &[0xaa, 0xbb],
                element_size: ElementSize::Sized { size_bits: 8 }
            };
            let event = Event {
                seq: 123,
                result: Ok(EventKind::Heartbeat { data })
            };
            self.ser_event(&event, scratch)
        }
    }
}

fn stream_serializer(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &api_level.items {
        let ApiItemKind::Stream { ident, ty, is_up } = &item.kind else {
            continue;
        };
        ts.append_all(quote! {});
    }
    ts
}
