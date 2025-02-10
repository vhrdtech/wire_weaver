use convert_case::Casing;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::codegen::api_common::args_structs;

pub fn client(api_level: &ApiLevel, api_model_location: &syn::Path, no_alloc: bool) -> TokenStream {
    let args_structs = args_structs(api_level, no_alloc);
    let root_level = level_methods(api_level, api_model_location, no_alloc);
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib16::Nib16
        };
        use #api_model_location::{Request, RequestKind, Event, EventKind, Error};

        impl Client {
            pub fn new() -> Self {
                Client {}
            }

            #root_level
        }
    }
}

fn level_methods(
    api_level: &ApiLevel,
    api_model_location: &syn::Path,
    no_alloc: bool,
) -> TokenStream {
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_method(&item.kind, item.id, api_model_location, no_alloc));
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    kind: &ApiItemKind,
    id: u16,
    api_model_location: &syn::Path,
    no_alloc: bool,
) -> TokenStream {
    match kind {
        ApiItemKind::Method { ident, args } => {
            let (args_ser, args_list, args_bytes) = ser_args(ident, args, no_alloc);
            quote! {
                pub fn #ident(&mut self, #args_list) -> Vec<u8> {
                    use wire_weaver::shrink_wrap::{
                        traits::SerializeShrinkWrap,
                        buf_writer::BufWriter,
                        nib16::Nib16,
                    };
                    use #api_model_location::{Request, RequestKind};
                    #args_ser
                    let request = Request {
                        // TODO: Handle sequence numbers properly
                        seq: 123,
                        // TODO: Handle sub-levels
                        path: vec![Nib16(#id)],
                        kind: RequestKind::Call { args: #args_bytes }
                    };
                    // TODO: get Vec from pool
                    let mut buf = [0u8; 128];
                    let mut wr = BufWriter::new(&mut buf);
                    request.ser_shrink_wrap(&mut wr).unwrap();
                    let request_bytes = wr.finish().unwrap().to_vec();
                    request_bytes
                }
            }
        }
        // ApiItemKind::Property => {}
        ApiItemKind::Stream {
            ident: _,
            ty: _,
            is_up: _,
        } => {
            quote! {}
        }
        // ApiItemKind::ImplTrait => {}
        // ApiItemKind::Level(_) => {}
        _ => unimplemented!(),
    }
}

fn ser_args(
    method_ident: &Ident,
    args: &[Argument],
    no_alloc: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let args_struct_ident =
        format!("{}_args", method_ident.sym).to_case(convert_case::Case::Pascal);
    let args_struct_ident = Ident::new(args_struct_ident);
    if args.is_empty() {
        (quote! {}, quote! {}, quote! { vec![] })
    } else {
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });

        let args_ser = quote! {
            let args = #args_struct_ident { #(#idents),* };
            // TODO: get Vec from pool
            let mut buf = [0u8; 128];
            let mut wr = BufWriter::new(&mut buf);
            args.ser_shrink_wrap(&mut wr).unwrap();
            let args_bytes = wr.finish().unwrap().to_vec();
        };
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });
        let tys = args.iter().map(|arg| arg.ty.def(no_alloc));
        let args_list = quote! { #(#idents: #tys),* };
        (args_ser, args_list, quote! { args_bytes })
    }
}
