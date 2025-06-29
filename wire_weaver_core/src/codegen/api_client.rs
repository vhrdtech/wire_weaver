use crate::ast::Type;
use crate::ast::api::{ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument};
use crate::ast::path::Path;
use crate::codegen::api_common::args_structs;
use crate::codegen::ty::FieldPath;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

pub fn client(
    api_level: &ApiLevel,
    no_alloc: bool,
    high_level_client: bool,
    ident: &Ident,
) -> TokenStream {
    let args_structs = args_structs(api_level, no_alloc);
    let root_level = level_methods(api_level, no_alloc, high_level_client);
    let output_des = output_des_fns(api_level, no_alloc);
    let additional_use = if no_alloc {
        quote! { use wire_weaver::shrink_wrap::RefVec; }
    } else {
        quote! {}
    };
    let (generics_a, generics_b) = if high_level_client {
        (quote! { <F, E: core::fmt::Debug> }, quote! { <F, E> })
    } else {
        (quote! {}, quote! {})
    };
    let hl_init = hl_init_methods(high_level_client);

    let parent = match &api_level.source_location {
        ApiLevelSourceLocation::File { part_of_crate, .. } => part_of_crate,
        ApiLevelSourceLocation::Crate { crate_name, .. } => crate_name,
    };
    let use_external = api_level.use_external_types(Path::new_ident(parent.clone()));
    quote! {
        mod api_client {
            #args_structs

            use wire_weaver::shrink_wrap::{
                DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
                Error as ShrinkWrapError, nib32::UNib32
            };
            use ww_client_server::{Request, RequestKind, Event, EventKind, Error};
            #additional_use
            #use_external

            impl #generics_a super::#ident #generics_b {
                #root_level
                #output_des
                #hl_init
            }
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
    id: u32,
    no_alloc: bool,
    high_level_client: bool,
) -> TokenStream {
    let path = if no_alloc {
        quote! { &[UNib32(#id)] }
    } else {
        quote! { vec![UNib32(#id)] }
    };
    let return_ty = if no_alloc {
        quote! { &[u8] }
    } else {
        quote! { Vec<u8> }
    };
    let path_ty = if no_alloc {
        quote! { &[UNib32] }
    } else {
        quote! { Vec<UNib32> }
    };
    match kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => {
            let (args_ser, args_list, args_names, args_bytes) = ser_args(ident, args, no_alloc);
            let ll_fn_name = Ident::new(format!("{}_ser_args_path", ident).as_str(), ident.span());
            let hl_fn_name = &ident;
            let hl_fn = if high_level_client {
                let (output_ty, maybe_dot_output) = if let Some(return_type) = &return_type {
                    let maybe_dot_output = if matches!(return_type, Type::External(_, _)) {
                        quote! {} // User type directly returned from method
                    } else {
                        quote! { .output } // Return type is wrapped in a struct
                    };
                    (return_type.def(no_alloc), maybe_dot_output)
                } else {
                    (quote! { () }, quote! {})
                };
                let des_output_fn =
                    Ident::new(format!("{}_des_output", ident).as_str(), ident.span());
                let handle_output = if return_type.is_some() {
                    quote! { Ok(Self::#des_output_fn(&data)? #maybe_dot_output) }
                } else {
                    quote! { _ = data; Ok(()) }
                };
                quote! {
                    pub async fn #hl_fn_name(&mut self, timeout: wire_weaver_client_common::Timeout, #args_list) -> Result<#output_ty, wire_weaver_client_common::Error<E>> {
                        let (args, path) = self.#ll_fn_name(#args_names)?;
                        let (args, path) = (args.to_vec(), path.to_vec());
                        let data =
                            wire_weaver_client_common::util::send_call_receive_reply(&mut self.cmd_tx, args, path, timeout)
                                .await?;
                        #handle_output
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
        ApiItemKind::Property {
            access,
            ident: prop_name,
            ty,
        } => {
            let mut ser = TokenStream::new();
            let ty_def = ty.arg_pos_def(no_alloc);
            ty.buf_write(
                FieldPath::Value(quote! { #prop_name }),
                no_alloc,
                quote! { ? },
                &mut ser,
            );
            let prop_ser_value_path = Ident::new(
                format!("{}_ser_value_path", prop_name).as_str(),
                prop_name.span(),
            );
            let prop_path = Ident::new(format!("{}_path", prop_name).as_str(), prop_name.span());
            let prop_des_value = Ident::new(
                format!("{}_des_value", prop_name).as_str(),
                prop_name.span(),
            );
            let finish_wr = if no_alloc {
                quote! { wr.finish_and_take()? }
            } else {
                quote! { wr.finish()?.to_vec() }
            };
            let mut des = TokenStream::new();
            ty.buf_read(
                &Ident::new("value", Span::call_site()),
                no_alloc,
                quote! { ? },
                &mut des,
            );
            let hl_fn_name = &prop_name;
            let hl_fn = if high_level_client {
                quote! {
                    pub async fn #hl_fn_name(&mut self, timeout: wire_weaver_client_common::Timeout, #prop_name: #ty_def) -> Result<(), wire_weaver_client_common::Error<E>> {
                        let (args, path) = self.#prop_ser_value_path(#prop_name)?;
                        let (args, path) = (args.to_vec(), path.to_vec());
                        let _data =
                            wire_weaver_client_common::util::send_write_receive_reply(&mut self.cmd_tx, args, path, timeout)
                                .await?;
                        Ok(())
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                #[inline]
                pub fn #prop_ser_value_path(&mut self, #prop_name: #ty_def) -> Result<(#return_ty, #path_ty), ShrinkWrapError> {
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

                #hl_fn
            }
        }
        ApiItemKind::Stream {
            ident,
            ty: _,
            is_up: _,
        } => {
            let fn_name = Ident::new(format!("{}_stream_path", ident).as_str(), ident.span());
            quote! {
                pub fn #fn_name(&self) -> #path_ty {
                    #path
                }
            }
        }
        ApiItemKind::ImplTrait { .. } => {
            quote! {}
        }
    }
}

fn ser_args(
    method_ident: &Ident,
    args: &[Argument],
    no_alloc: bool,
) -> (TokenStream, TokenStream, TokenStream, TokenStream) {
    let args_struct_ident = Ident::new(
        format!("{}_args", method_ident)
            .to_case(Case::Pascal)
            .as_str(),
        method_ident.span(),
    );
    if args.is_empty() {
        if no_alloc {
            (
                quote! {},
                quote! {},
                quote! {},
                // 0 when no arguments to allow adding them later, as Option
                quote! { &[0x00] },
            )
        } else {
            (quote! {}, quote! {}, quote! {}, quote! { vec![] })
        }
    } else {
        let idents = args.iter().map(|arg| &arg.ident);

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
        let idents = args.iter().map(|arg| &arg.ident).collect::<Vec<_>>();
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
        _ => None,
    });
    quote! {
        #(#handlers)*
    }
}

fn output_des_fn(ident: &Ident, return_type: &Type, no_alloc: bool) -> TokenStream {
    let fn_name = Ident::new(format!("{}_des_output", ident).as_str(), ident.span());

    let ty_def = if matches!(return_type, Type::External(_, _)) {
        return_type.def(no_alloc)
    } else {
        let output_struct_name = Ident::new(
            format!("{}_output", ident).to_case(Case::Pascal).as_str(),
            ident.span(),
        );
        quote! { #output_struct_name }
    };
    quote! {
        pub fn #fn_name(bytes: &[u8]) -> Result<#ty_def, ShrinkWrapError> {
            let mut rd = BufReader::new(bytes);
            Ok(#ty_def::des_shrink_wrap(&mut rd)?)
        }
    }
}

fn hl_init_methods(high_level_client: bool) -> TokenStream {
    if high_level_client {
        quote! {
            pub async fn disconnect_and_exit(&mut self) -> Result<(), wire_weaver_client_common::Error<E>> {
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                self.cmd_tx
                    .send(wire_weaver_client_common::Command::DisconnectAndExit {
                        disconnected_tx: Some(done_tx),
                    })
                    .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
                let _ = done_rx.await.map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
                Ok(())
            }

            pub fn disconnect_and_exit_non_blocking(&mut self) -> Result<(), wire_weaver_client_common::Error<E>> {
                self.cmd_tx
                    .send(wire_weaver_client_common::Command::DisconnectAndExit {
                        disconnected_tx: None,
                    })
                    .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
                Ok(())
            }

            /// Disconnect from a connected device. Event loop will be left running, and error mode will be set to KeepRetrying.
            pub fn disconnect_keep_streams_non_blocking(&mut self) -> Result<(), wire_weaver_client_common::Error<E>> {
                self.cmd_tx
                    .send(wire_weaver_client_common::Command::DisconnectKeepStreams {
                        disconnected_tx: None,
                    })
                    .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
                Ok(())
            }
        }
    } else {
        quote! {}
    }
}
