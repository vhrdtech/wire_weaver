use crate::ast::Type;
use crate::ast::api::{ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument, PropertyAccess};
use crate::ast::path::Path;
use crate::codegen::api_common::args_structs;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::ty::FieldPath;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

#[derive(Copy, Clone, PartialEq)]
pub enum ClientModel {
    /// Prepare ww_client_server::Request and return it.
    /// Generates no_std, no_alloc and sync code.
    Raw,
    /// Prepare ww_client_server::Request, convert it to RequestOwned and
    /// send through wire_weaver_client_common::CommandSender to a worker thread.
    /// Generates std, async code that allocates.
    AsyncWorker,
}

impl ClientModel {
    fn no_alloc(&self) -> bool {
        match self {
            ClientModel::Raw => true,
            ClientModel::AsyncWorker => false,
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum ClientPathMode {
    Absolute,
    GlobalTrait,
}

pub fn client(
    api_level: &ApiLevel,
    model: ClientModel,
    path_mode: ClientPathMode,
    client_struct: &Ident,
) -> TokenStream {
    let additional_use = if model == ClientModel::AsyncWorker {
        quote! { use wire_weaver_client_common::ww_client_server::PathKind; }
    } else {
        quote! {}
    };
    let hl_init = if model == ClientModel::AsyncWorker {
        cmd_tx_disconnect_methods()
    } else {
        quote! {}
    };

    let ext_crate_name = match &api_level.source_location {
        ApiLevelSourceLocation::File { part_of_crate, .. } => part_of_crate,
        ApiLevelSourceLocation::Crate { crate_name, .. } => crate_name,
    };
    let root_mod_name = api_level.mod_ident(Some(ext_crate_name));
    let root_client_struct_name = api_level.client_struct_name(Some(ext_crate_name));
    let trait_clients = client_structs_recursive(
        api_level,
        IndexChain::new(),
        Some(ext_crate_name),
        model,
        path_mode,
    );
    quote! {
        mod api_client {
            use wire_weaver::shrink_wrap::{
                DeserializeShrinkWrap, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
                Error as ShrinkWrapError, nib32::UNib32, RefVec
            };
            #additional_use

            impl<F, E: core::fmt::Debug> super::#client_struct<F, E> {
                pub fn root(&mut self) -> #root_mod_name::#root_client_struct_name<'_, F, E> {
                    #root_mod_name::#root_client_struct_name {
                        args_scratch: &mut self.args_scratch,
                        cmd_tx: &mut self.cmd_tx,
                    }
                }

                #hl_init
            }

            #trait_clients
        }
    }
}

fn client_structs_recursive(
    api_level: &ApiLevel,
    mut index_chain: IndexChain,
    ext_crate_name: Option<&Ident>,
    model: ClientModel,
    path_mode: ClientPathMode,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let args_structs = args_structs(api_level, model.no_alloc());

    let mod_name = api_level.mod_ident(ext_crate_name);
    let use_external = api_level.use_external_types(
        ext_crate_name
            .map(|n| Path::new_ident(n.clone()))
            .unwrap_or(Path::new_path("super::super")),
        model.no_alloc(),
    );
    let client_struct_name = api_level.client_struct_name(ext_crate_name);
    let full_gid_path = api_level.full_gid_path();
    let methods = level_methods(api_level, index_chain, model, path_mode, &full_gid_path);

    // call before increment_length so that root level does not have it
    let maybe_index_chain_field = index_chain.struct_field_def();

    let mut child_ts = TokenStream::new();
    for item in &api_level.items {
        let ApiItemKind::ImplTrait { args, level } = &item.kind else {
            continue;
        };
        let level = level.as_ref().expect("empty level");
        index_chain.increment_length();
        child_ts.extend(client_structs_recursive(
            level,
            index_chain,
            args.location.crate_name().as_ref(),
            model,
            path_mode,
        ));
    }

    ts.extend(quote! {
        mod #mod_name {
            use super::*;
            #use_external
            #args_structs

            pub struct #client_struct_name<'i, F, E> {
                #maybe_index_chain_field
                pub args_scratch: &'i mut [u8],
                pub cmd_tx: &'i mut wire_weaver_client_common::CommandSender<F, E>
            }

            impl<'i, F, E: core::fmt::Debug> #client_struct_name<'i, F, E> {
                #methods
            }

            #child_ts
        }
    });
    ts
}

fn level_methods(
    api_level: &ApiLevel,
    index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
) -> TokenStream {
    let handlers = api_level.items.iter().map(|item| {
        level_method(
            &item.kind,
            item.id,
            index_chain,
            model,
            path_mode,
            full_gid_path,
        )
    });
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    kind: &ApiItemKind,
    id: u32,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
) -> TokenStream {
    let index_chain_push = index_chain.push_back(quote! { self. }, quote! { UNib32(#id) });
    match kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => handle_method(
            model,
            path_mode,
            full_gid_path,
            index_chain_push,
            ident,
            args,
            return_type,
        ),
        ApiItemKind::Property { access, ident, ty } => handle_property(
            model,
            path_mode,
            full_gid_path,
            index_chain_push,
            access,
            ident,
            ty,
        ),
        ApiItemKind::Stream {
            ident,
            ty: _,
            is_up: _,
        } => {
            let fn_name = Ident::new(format!("{}_stream_path", ident).as_str(), ident.span());
            let ty = index_chain.return_ty_def();
            quote! {
                pub fn #fn_name(&self) -> #ty {
                    #index_chain_push
                    index_chain
                }
            }
        }
        ApiItemKind::ImplTrait { args, level } => {
            let level_entry_fn_name = &args.resource_name;
            let level = level.as_ref().expect("api level");
            let ext_crate_name = args.location.crate_name().clone();
            let mod_name = level.mod_ident(ext_crate_name.as_ref());
            let client_struct_name = level.client_struct_name(ext_crate_name.as_ref());
            quote! {
                pub fn #level_entry_fn_name(&mut self) -> #mod_name::#client_struct_name<'_, F, E> {
                    #index_chain_push;
                    #mod_name::#client_struct_name {
                        index_chain,
                        args_scratch: self.args_scratch,
                        cmd_tx: self.cmd_tx,
                    }
                }
            }
        }
    }
}

fn handle_method(
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    args: &[Argument],
    return_type: &Option<Type>,
) -> TokenStream {
    let (args_ser, args_list, _args_names, args_bytes) = ser_args(ident, args, model.no_alloc());
    let call_fn_name = &ident;
    let (output_ty, maybe_dot_output) = if let Some(return_type) = &return_type {
        let maybe_dot_output = if matches!(return_type, Type::External(_, _)) {
            quote! {} // User type directly returned from method
        } else {
            quote! { .output } // Return type is wrapped in a struct
        };
        (return_type.def(model.no_alloc()), maybe_dot_output)
    } else {
        (quote! { () }, quote! {})
    };
    let handle_output = if let Some(return_type) = return_type {
        let ty_def = if matches!(return_type, Type::External(_, _)) {
            return_type.def(model.no_alloc())
        } else {
            let output_struct_name = Ident::new(
                format!("{}_output", ident).to_case(Case::Pascal).as_str(),
                ident.span(),
            );
            quote! { #output_struct_name }
        };
        quote! {
            let mut rd = BufReader::new(&return_bytes);
            Ok(#ty_def::des_shrink_wrap(&mut rd)? #maybe_dot_output)
        }
    } else {
        quote! { _ = return_bytes; Ok(()) }
    };

    let path_kind = path_kind(path_mode, full_gid_path);
    quote! {
        pub async fn #call_fn_name(&mut self, timeout: wire_weaver_client_common::Timeout, #args_list) -> Result<#output_ty, wire_weaver_client_common::Error<E>> {
            #args_ser
            let args_bytes = #args_bytes;
            #index_chain_push
            let path_kind = #path_kind;
            let return_bytes = self.cmd_tx.send_call_receive_reply(
                path_kind,
                args_bytes,
                timeout
            ).await?;
            #handle_output
        }
    }
}

fn handle_property(
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
    index_chain_push: TokenStream,
    access: &PropertyAccess,
    prop_name: &Ident,
    ty: &Type,
) -> TokenStream {
    let mut ser = TokenStream::new();
    let ty_def = if ty.potential_lifetimes() && !model.no_alloc() {
        let mut ty_owned = ty.clone();
        ty_owned.make_owned();
        ty_owned.arg_pos_def(model.no_alloc())
    } else {
        ty.arg_pos_def(model.no_alloc())
    };
    ty.buf_write(
        FieldPath::Value(quote! { #prop_name }),
        model.no_alloc(),
        quote! { ? },
        &mut ser,
    );
    let finish_wr = if model.no_alloc() {
        quote! { wr.finish_and_take()? }
    } else {
        quote! { wr.finish()?.to_vec() }
    };
    let mut des = TokenStream::new();
    ty.buf_read(
        &Ident::new("value", Span::call_site()),
        model.no_alloc(),
        quote! { ? },
        &mut des,
    );
    let path_kind = path_kind(path_mode, full_gid_path);
    let hl_write_fn = Ident::new(format!("write_{}", prop_name).as_str(), prop_name.span());
    let hl_write_fn = if matches!(
        access,
        PropertyAccess::ReadWrite | PropertyAccess::WriteOnly
    ) {
        quote! {
            pub async fn #hl_write_fn(&mut self, timeout: wire_weaver_client_common::Timeout, #prop_name: #ty_def) -> Result<(), wire_weaver_client_common::Error<E>> {
                let mut wr = BufWriter::new(&mut self.args_scratch);
                #ser
                let args = #finish_wr;
                #index_chain_push
                let path_kind = #path_kind;
                let _data = self.cmd_tx.send_write_receive_reply(
                    path_kind,
                    args,
                    timeout
                ).await?;
                Ok(())
            }
        }
    } else {
        quote! {}
    };
    let hl_read_fn = Ident::new(format!("read_{}", prop_name).as_str(), prop_name.span());
    let hl_read_fn = if matches!(
        access,
        PropertyAccess::Const | PropertyAccess::ReadWrite | PropertyAccess::ReadOnly
    ) {
        quote! {
            pub async fn #hl_read_fn(&mut self, timeout: wire_weaver_client_common::Timeout) -> Result<#ty_def, wire_weaver_client_common::Error<E>> {
                #index_chain_push
                let path_kind = #path_kind;
                let bytes = self.cmd_tx.send_read_receive_reply(
                    path_kind,
                    timeout
                ).await?;
                let mut rd = BufReader::new(&bytes);
                #des
                Ok(value)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #hl_write_fn
        #hl_read_fn
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

fn cmd_tx_disconnect_methods() -> TokenStream {
    quote! {
        pub async fn disconnect_and_exit(&mut self) -> Result<(), wire_weaver_client_common::Error<E>> {
            let (cmd, done_rx) = wire_weaver_client_common::Command::disconnect_and_exit();
            self.cmd_tx
                .send(cmd)
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
}

/// Creates ww_client_server::PathKind in user context
fn path_kind(path_mode: ClientPathMode, full_gid_path: &TokenStream) -> TokenStream {
    match path_mode {
        ClientPathMode::Absolute => {
            quote! { PathKind::Absolute { path: RefVec::Slice { slice: &index_chain } } }
        }
        ClientPathMode::GlobalTrait => quote! {
            PathKind::GlobalFull {
                gid: #full_gid_path,
                path_from_trait: RefVec::Slice { slice: &index_chain },
            }
        },
    }
}
