use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};

pub fn server_dispatcher(
    api_level: &ApiLevel,
    api_model_location: syn::Path,
    no_alloc: bool,
) -> TokenStream {
    let level_matchers = level_matchers(api_level, no_alloc);
    quote! {
        impl Context {
            pub fn process_request<F: FnMut(&[u8])>(&mut self, bytes: &[u8], scratch: &mut [u8], send_event: F) {
                use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, buf_reader::BufReader, traits::ElementSize};
                // settings to generate it automatically or use provided?
                use #api_model_location::{Request, RequestKind, Event, EventKind};

                let mut rd = BufReader::new(bytes);
                let request = Request::des_shrink_wrap(&mut rd, ElementSize::Implied).unwrap();
                let path_iter = request.path.iter();
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
                        return;
                    }
                }
            }
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
        #(Some(#ids) => { #handlers } ),*
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
