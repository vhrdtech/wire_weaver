use convert_case::Casing;
use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::ast::Type;
use crate::ast::api::{ApiItemKind, ApiLevel, Argument};
use crate::ast::ident::Ident;
use crate::codegen::api_common::args_structs;
use crate::codegen::ty::FieldPath;

pub fn client(
    api_level: &ApiLevel,
    api_model_location: &Option<syn::Path>,
    no_alloc: bool,
    high_level_client: bool,
) -> TokenStream {
    let args_structs = args_structs(api_level, no_alloc);
    let root_level = level_methods(api_level, no_alloc, high_level_client);
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
    let (generics_a, generics_b) = if high_level_client {
        (quote! { <F, E: core::fmt::Debug> }, quote! { <F, E> })
    } else {
        (quote! {}, quote! {})
    };
    let hl_init = hl_init_methods(high_level_client);
    quote! {
        #args_structs

        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib16::Nib16
        };
        #api_model_includes
        #additional_use

        impl #generics_a Client #generics_b {
            #root_level
            #output_des
            #hl_init
        }
    }
}

fn level_methods(api_level: &ApiLevel, no_alloc: bool, high_level_client: bool) -> TokenStream {
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_method(&item.kind, item.id, no_alloc, high_level_client));
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    kind: &ApiItemKind,
    id: u16,
    no_alloc: bool,
    high_level_client: bool,
) -> TokenStream {
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
            return_type,
        } => {
            let (args_ser, args_list, args_names, args_bytes) = ser_args(ident, args, no_alloc);
            let ll_fn_name = Ident::new(format!("{}_ser_args_path", ident.sym));
            let hl_fn_name = &ident;
            let des_output_fn = Ident::new(format!("{}_des_output", ident.sym));
            let hl_fn = if high_level_client {
                let output_ty = return_type
                    .as_ref()
                    .map(|t| t.def(no_alloc))
                    .unwrap_or(quote! { () });
                quote! {
                    pub async fn #hl_fn_name(&mut self, timeout: Option<std::time::Duration>, #args_list) -> Result<#output_ty, wire_weaver_client_server::Error<E>> {
                        let (args, path) = self.#ll_fn_name(#args_names)?;
                        let (args, path) = (args.to_vec(), path.to_vec());
                        let data =
                            wire_weaver_client_server::util::send_call_receive_reply(&mut self.cmd_tx, args, path, timeout)
                                .await?;
                        Ok(Self::#des_output_fn(&data)?.output)
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                pub fn #ll_fn_name(&mut self, #args_list) -> Result<(#return_ty, #path_ty), ShrinkWrapError> {
                    #args_ser
                    Ok((#args_bytes, #path))
                }
                #hl_fn
            }
        }
        ApiItemKind::Property { ident, ty } => {
            let mut ser = TokenStream::new();
            let ty_def = ty.arg_pos_def(no_alloc);
            ty.buf_write(
                FieldPath::Value(quote! { #ident }),
                no_alloc,
                quote! { ? },
                &mut ser,
            );
            let prop_ser_value_path = Ident::new(format!("{}_ser_value_path", ident.sym));
            let prop_path = Ident::new(format!("{}_path", ident.sym));
            let prop_des_value = Ident::new(format!("{}_des_value", ident.sym));
            let finish_wr = if no_alloc {
                quote! { wr.finish_and_take()? }
            } else {
                quote! { wr.finish()?.to_vec() }
            };
            let mut des = TokenStream::new();
            ty.buf_read(
                proc_macro2::Ident::new("value", Span::call_site()),
                no_alloc,
                quote! { ? },
                &mut des,
            );
            quote! {
                #[inline]
                pub fn #prop_ser_value_path(&mut self, #ident: #ty_def) -> Result<(#return_ty, #path_ty), ShrinkWrapError> {
                    let mut wr = BufWriter::new(&mut self.args_scratch);
                    #ser
                    let args_bytes = #finish_wr;
                    Ok((args_bytes, #path))
                }

                #[inline]
                pub fn #prop_path(&self) -> #path_ty {
                    #path
                }

                #[inline]
                pub fn #prop_des_value(value_bytes: &[u8]) -> Result<#ty_def, ShrinkWrapError> {
                    let mut rd = BufReader::new(value_bytes);
                    #des
                    Ok(value)
                }
            }
        }
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
        u => unimplemented!("{u:?}"),
    }
}

fn ser_args(
    method_ident: &Ident,
    args: &[Argument],
    no_alloc: bool,
) -> (TokenStream, TokenStream, TokenStream, TokenStream) {
    let args_struct_ident =
        format!("{}_args", method_ident.sym).to_case(convert_case::Case::Pascal);
    let args_struct_ident = Ident::new(args_struct_ident);
    if args.is_empty() {
        if no_alloc {
            (
                quote! {},
                quote! {},
                quote! {},
                // 0 when no arguments to allow adding them later, as Option
                // quote! { RefVec::Slice { slice: &[0x00], element_size: ElementSize::Sized { size_bits: 8 } } },
                quote! { &[0x00] },
            )
        } else {
            (quote! {}, quote! {}, quote! {}, quote! { vec![] })
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
        let idents = args
            .iter()
            .map(|arg| {
                let ident: proc_macro2::Ident = (&arg.ident).into();
                ident
            })
            .collect::<Vec<_>>();
        let tys = args.iter().map(|arg| arg.ty.arg_pos_def(no_alloc));
        let args_list = quote! { #(#idents: #tys),* };
        let args_names = quote! { #(#idents),* };
        (args_ser, args_list, args_names, quote! { args_bytes })
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

fn hl_init_methods(high_level_client: bool) -> TokenStream {
    if high_level_client {
        quote! {
            pub async fn disconnect_and_exit(&mut self) -> Result<(), wire_weaver_client_server::Error<E>> {
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                self.cmd_tx
                    .send(wire_weaver_client_server::Command::DisconnectAndExit {
                        disconnected_tx: Some(done_tx),
                    })
                    .map_err(|_| wire_weaver_client_server::Error::EventLoopNotRunning)?;
                let _ = done_rx.await.map_err(|_| wire_weaver_client_server::Error::EventLoopNotRunning)?;
                Ok(())
            }

            pub fn disconnect_and_exit_non_blocking(&mut self) -> Result<(), wire_weaver_client_server::Error<E>> {
                self.cmd_tx
                    .send(wire_weaver_client_server::Command::DisconnectAndExit {
                        disconnected_tx: None,
                    })
                    .map_err(|_| wire_weaver_client_server::Error::EventLoopNotRunning)?;
                Ok(())
            }

            /// Disconnect from connected device. Event loop will be left running and error mode will lbe set to KeepRetrying.
            pub fn disconnect_keep_streams_non_blocking(&mut self) -> Result<(), wire_weaver_client_server::Error<E>> {
                self.cmd_tx
                    .send(wire_weaver_client_server::Command::DisconnectKeepStreams {
                        disconnected_tx: None,
                    })
                    .map_err(|_| wire_weaver_client_server::Error::EventLoopNotRunning)?;
                Ok(())
            }
        }
    } else {
        quote! {}
    }
}
