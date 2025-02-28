use convert_case::Casing;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::ast::Type;
use crate::codegen::api_common::args_structs;

pub fn client(
    api_level: &ApiLevel,
    api_model_location: &Option<syn::Path>,
    no_alloc: bool,
) -> TokenStream {
    let args_structs = args_structs(api_level, no_alloc);
    let root_level = level_methods(api_level, no_alloc);
    let output_des = output_des_fns(api_level, no_alloc);
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
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib16::Nib16
        };
        #api_model_includes
        #additional_use

        impl Client {
            #root_level
            #output_des
        }
    }
}

fn level_methods(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_method(&item.kind, item.id, no_alloc));
    quote! {
        #(#handlers)*
    }
}

fn level_method(kind: &ApiItemKind, id: u16, no_alloc: bool) -> TokenStream {
    // TODO: Handle sub-levels
    let path = if no_alloc {
        // quote! { RefVec::Slice { slice: &[Nib16(#id)], element_size: ElementSize::UnsizedSelfDescribing } }
        quote! { &[Nib16(#id)] }
    } else {
        quote! { vec![Nib16(#id)] }
    };
    let return_ty = if no_alloc {
        quote! { &[u8] }
    } else {
        quote! { Vec<u8> }
    };
    // let finish_wr = if no_alloc {
    //     quote! { wr.finish_and_take()? }
    // } else {
    //     quote! { wr.finish()?.to_vec() }
    // };
    let path_ty = if no_alloc {
        quote! { &[Nib16] }
    } else {
        // should be u16?
        quote! { Vec<Nib16> }
    };
    match kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type: _,
        } => {
            let (args_ser, args_list, args_bytes) = ser_args(ident, args, no_alloc);
            let fn_name = Ident::new(format!("{}_ser_args_path", ident.sym));
            quote! {
                pub fn #fn_name(&mut self, #args_list) -> Result<(#return_ty, #path_ty), ShrinkWrapError> {
                    #args_ser
                    // let request = Request {
                    //     seq: 123,
                    //     path: #path,
                    //     kind: RequestKind::Call { args: #args_bytes }
                    // };
                    // let mut wr = BufWriter::new(&mut self.event_scratch);
                    // request.ser_shrink_wrap(&mut wr)?;
                    // let request_bytes = #finish_wr;
                    // Ok(request_bytes)
                    Ok((#args_bytes, #path))
                }
            }
        }
        // ApiItemKind::Property => {}
        ApiItemKind::Stream {
            ident,
            ty: _,
            is_up: _,
        } => {
            let fn_name = Ident::new(format!("{}_stream_path", ident.sym));
            quote! {
                pub fn #fn_name(&self) -> #path_ty {
                    #path
                }
            }
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
        if no_alloc {
            (
                quote! {},
                quote! {},
                // 0 when no arguments to allow adding them later, as Option
                // quote! { RefVec::Slice { slice: &[0x00], element_size: ElementSize::Sized { size_bits: 8 } } },
                quote! { &[0x00] },
            )
        } else {
            (quote! {}, quote! {}, quote! { vec![] })
        }
    } else {
        let idents = args.iter().map(|arg| {
            let ident: proc_macro2::Ident = (&arg.ident).into();
            ident
        });

        let finish_wr = if no_alloc {
            quote! { wr.finish_and_take()? }
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

fn output_des_fns(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let handlers = api_level.items.iter().filter_map(|item| match &item.kind {
        ApiItemKind::Method {
            ident,
            args: _,
            return_type,
        } => return_type
            .as_ref()
            .map(|ty| output_des_fn(ident, ty, no_alloc)),
        ApiItemKind::Level(_) => unimplemented!(),
        _ => None,
    });
    quote! {
        #(#handlers)*
    }
}

fn output_des_fn(ident: &Ident, return_type: &Type, no_alloc: bool) -> TokenStream {
    let fn_name = Ident::new(format!("{}_des_output", ident.sym));

    let ty_def = if matches!(return_type, Type::Unsized(_, _) | Type::Sized(_, _)) {
        return_type.def(no_alloc)
    } else {
        let output_struct_name =
            Ident::new(format!("{}_output", ident.sym).to_case(convert_case::Case::Pascal));
        quote! { #output_struct_name }
    };
    quote! {
        pub fn #fn_name(bytes: &[u8]) -> Result<#ty_def, ShrinkWrapError> {
            let mut rd = BufReader::new(bytes);
            Ok(rd.read(ElementSize::Implied)?)
        }
    }
}
