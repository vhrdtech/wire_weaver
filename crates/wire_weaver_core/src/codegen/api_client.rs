use convert_case::Casing;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::codegen::api_common::args_structs;

pub fn client(api_level: &ApiLevel, api_model_location: &syn::Path, no_alloc: bool) -> TokenStream {
    let args_structs = args_structs(api_level, no_alloc);
    let root_level = level_methods(api_level, api_model_location, no_alloc);
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

        impl Client {
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
    // TODO: Handle sub-levels
    let path = if no_alloc {
        quote! { RefVec::Slice { slice: &[Nib16(#id)], element_size: ElementSize::UnsizedSelfDescribing } }
    } else {
        quote! { vec![Nib16(#id)] }
    };
    let return_ty = if no_alloc {
        quote! { &[u8] }
    } else {
        quote! { Vec<u8> }
    };
    let finish_wr = if no_alloc {
        quote! { wr.finish_and_take()? }
    } else {
        quote! { wr.finish()?.to_vec() }
    };
    match kind {
        ApiItemKind::Method { ident, args } => {
            let (args_ser, args_list, args_bytes) = ser_args(ident, args, no_alloc);
            quote! {
                pub fn #ident(&mut self, #args_list) -> Result<#return_ty, ShrinkWrapError> {
                    use #api_model_location::{Request, RequestKind};
                    #args_ser
                    let request = Request {
                        // TODO: Handle sequence numbers properly
                        seq: 123,
                        path: #path,
                        kind: RequestKind::Call { args: #args_bytes }
                    };
                    let mut wr = BufWriter::new(&mut self.event_scratch);
                    request.ser_shrink_wrap(&mut wr)?;
                    let request_bytes = #finish_wr;
                    Ok(request_bytes)
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

        let finish_wr = if no_alloc {
            quote! { RefVec::Slice { slice: wr.finish()?, element_size: ElementSize::Sized { size_bits: 8 } } }
        } else {
            quote! { wr.finish()?.to_vec() }
        };

        let args_ser = quote! {
            let args = #args_struct_ident { #(#idents),* };
            let mut wr = BufWriter::new(&mut self.args_scratch);
            args.ser_shrink_wrap(&mut wr)?;
            let args_bytes = #finish_wr;
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
