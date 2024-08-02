use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};

pub fn server_dispatcher(
    api_level: &ApiLevel,
    api_model_location: &syn::Path,
    no_alloc: bool,
) -> TokenStream {
    let level_matchers = level_matchers(api_level, no_alloc);
    let ser_event = ser_event(&api_model_location);
    let ser_heartbeat = ser_heartbeat(&api_model_location);
    quote! {
        impl Context {
            pub fn process_request<'a>(
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
                            RequestKind::Version { protocol_id: u32, version: Version } => {
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

fn level_matchers(api_level: &ApiLevel, _no_alloc: bool) -> TokenStream {
    let ids = api_level.items.iter().map(|item| {
        Lit::Int(LitInt::new(
            format!("{}u16", item.id).as_str(),
            Span::call_site(),
        ))
    });
    let handlers = api_level.items.iter().map(|item| level_matcher(&item.kind));
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

fn level_matcher(kind: &ApiItemKind) -> TokenStream {
    match kind {
        ApiItemKind::Method { ident, args } => {
            let des_args = des_args(args);
            let is_args = if args.is_empty() {
                quote! { .. }
            } else {
                quote! { args }
            };
            quote! {
                match &request.kind {
                    RequestKind::Call { #is_args } => {
                        #des_args
                        #ident(self);
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
        // ApiItemKind::Stream => {}
        // ApiItemKind::ImplTrait => {}
        // ApiItemKind::Level(_) => {}
        _ => unimplemented!(),
    }
}

fn des_args(args: &[Argument]) -> TokenStream {
    if args.is_empty() {
        quote! {}
    } else {
        quote! {
            let rd = BufReader::new(args);
            let args: XArgs = rd.read()?;
        }
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
            match wr.finish() {
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
