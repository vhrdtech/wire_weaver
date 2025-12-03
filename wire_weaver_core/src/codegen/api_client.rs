//! # Implementation details:
//! * Client's index chain contains all indices up to last level (resource IDs + array index if used)
use crate::ast::api::{
    ApiItem, ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument, Multiplicity, PropertyAccess,
};
use crate::ast::path::Path;
use crate::ast::{Docs, Type};
use crate::codegen::api_common::args_structs;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::maybe_quote;
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
            use wire_weaver_client_common::StreamEvent;
            use wire_weaver_client_common::ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
            #additional_use

            impl super::#client_struct {
                pub fn root(&mut self) -> #root_mod_name::#root_client_struct_name<'_> {
                    #root_mod_name::#root_client_struct_name {
                        args_scratch: &mut self.args_scratch,
                        cmd_tx: &mut self.cmd_tx,
                        timeout: self.timeout,
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
    index_chain: IndexChain,
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
        let mut index_chain = index_chain.clone();
        index_chain.increment_length();
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
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

            pub struct #client_struct_name<'i> {
                #maybe_index_chain_field
                pub args_scratch: &'i mut [u8],
                pub cmd_tx: &'i mut wire_weaver_client_common::CommandSender,
                pub timeout: core::time::Duration,
            }

            impl<'i> #client_struct_name<'i> {
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
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_method(&item, index_chain, model, path_mode, full_gid_path));
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    item: &ApiItem,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
) -> TokenStream {
    let id = item.id;
    let index_chain_push = index_chain.push_back(quote! { self. }, quote! { UNib32(#id) });
    let (index_chain_push, maybe_index_arg) =
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            let p = index_chain.push_back(quote! {}, quote! { UNib32(index) });
            (quote! { #index_chain_push #p }, quote! { , index: u32 })
        } else {
            (index_chain_push, quote! {})
        };
    match &item.kind {
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
            &item.docs,
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
        ApiItemKind::Stream { ident, ty, is_up } => handle_stream(
            model,
            path_mode,
            full_gid_path,
            index_chain_push,
            ident,
            ty,
            *is_up,
        ),
        ApiItemKind::ImplTrait { args, level } => {
            let level_entry_fn_name = &args.resource_name;
            let level = level.as_ref().expect("api level");
            let ext_crate_name = args.location.crate_name().clone();
            let mod_name = level.mod_ident(ext_crate_name.as_ref());
            let client_struct_name = level.client_struct_name(ext_crate_name.as_ref());
            quote! {
                pub fn #level_entry_fn_name(&mut self #maybe_index_arg) -> #mod_name::#client_struct_name<'_> {
                    #index_chain_push
                    #mod_name::#client_struct_name {
                        index_chain,
                        args_scratch: self.args_scratch,
                        cmd_tx: self.cmd_tx,
                        timeout: self.timeout,
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
    docs: &Docs,
) -> TokenStream {
    let (args_ser, args_list, _args_names, args_bytes) = ser_args(ident, args, model.no_alloc());
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
    let fn_call = |default_timeout: bool| {
        let (maybe_timeout_arg, timeout_val) = timeout_arg_val(default_timeout);
        let timeout_fn_name = Ident::new(&format!("{}_timeout", ident), ident.span());
        if default_timeout {
            let args_idents = args.iter().map(|arg| &arg.ident).collect::<Vec<_>>();
            let maybe_comma = maybe_quote(!args.is_empty(), quote! { , });
            quote! {
                #docs
                #[doc = "NOTE: This method uses `self.timeout` as timeout."]
                pub async fn #ident(&mut self, #args_list) -> Result<#output_ty, wire_weaver_client_common::Error> {
                    self.#timeout_fn_name(#(#args_idents),* #maybe_comma self.timeout).await
                }
            }
        } else {
            quote! {
                #docs
                pub async fn #timeout_fn_name(&mut self, #args_list #maybe_timeout_arg) -> Result<#output_ty, wire_weaver_client_common::Error> {
                    #args_ser
                    let args_bytes = #args_bytes;
                    #index_chain_push
                    let path_kind = #path_kind;
                    let return_bytes = self.cmd_tx.send_call_receive_reply(
                        path_kind,
                        args_bytes,
                        #timeout_val
                    ).await?;
                    #handle_output
                }
            }
        }
    };

    let fn_call_forget = if return_type.is_none() {
        let forget_fn_name = Ident::new(&format!("{}_forget", ident), ident.span());
        quote! {
            #docs
            #[doc = "NOTE: This method does not wait for the answer from remote device."]
            pub async fn #forget_fn_name(&mut self, #args_list) -> Result<(), wire_weaver_client_common::Error> {
                #args_ser
                let args_bytes = #args_bytes;
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd_tx.send_call_forget(path_kind, args_bytes).await?;
                Ok(())
            }
        }
    } else {
        quote! {}
    };

    let fn_call_default_timeout = fn_call(true);
    let fn_call = fn_call(false);
    quote! {
        #fn_call_default_timeout
        #fn_call
        #fn_call_forget
    }
}

fn timeout_arg_val(default_timeout: bool) -> (TokenStream, TokenStream) {
    let maybe_timeout_arg = maybe_quote(!default_timeout, quote! { timeout: core::time::Duration });
    let timeout_val = if default_timeout {
        quote! { self.timeout }
    } else {
        quote! { timeout }
    };
    (maybe_timeout_arg, timeout_val)
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
    let (ty_def, ser) = ser_write_value(model, prop_name, ty);
    let mut des = TokenStream::new();
    ty.buf_read(
        &Ident::new("value", Span::call_site()),
        model.no_alloc(),
        quote! { ? },
        &mut des,
    );
    let path_kind = path_kind(path_mode, full_gid_path);

    let write_fns = if matches!(
        access,
        PropertyAccess::ReadWrite | PropertyAccess::WriteOnly
    ) {
        let prop_write = |default_timeout: bool| {
            let (maybe_timeout_arg, timeout_val) = timeout_arg_val(default_timeout);
            let write_fn_name = if default_timeout {
                Ident::new(&format!("write_{}", prop_name), prop_name.span())
            } else {
                Ident::new(&format!("write_{}_timeout", prop_name), prop_name.span())
            };
            quote! {
                pub async fn #write_fn_name(&mut self, #prop_name: #ty_def, #maybe_timeout_arg) -> Result<(), wire_weaver_client_common::Error> {
                    #ser
                    #index_chain_push
                    let path_kind = #path_kind;
                    let _data = self.cmd_tx.send_write_receive_reply(
                        path_kind,
                        value,
                        #timeout_val
                    ).await?;
                    Ok(())
                }
            }
        };
        let prop_write_default_timeout = prop_write(true);
        // let prop_write = prop_write(false);
        quote! {
            #prop_write_default_timeout
            // #prop_write
        }
    } else {
        quote! {}
    };

    let read_fns = if matches!(
        access,
        PropertyAccess::Const | PropertyAccess::ReadWrite | PropertyAccess::ReadOnly
    ) {
        let prop_read = |default_timeout: bool| {
            let (maybe_timeout_arg, timeout_val) = timeout_arg_val(default_timeout);
            let read_fn_name = if default_timeout {
                Ident::new(&format!("read_{}", prop_name), prop_name.span())
            } else {
                Ident::new(&format!("read_{}_timeout", prop_name), prop_name.span())
            };
            let maybe_comma = maybe_quote(!default_timeout, quote! { , });
            quote! {
                pub async fn #read_fn_name(&mut self #maybe_comma #maybe_timeout_arg) -> Result<#ty_def, wire_weaver_client_common::Error> {
                    #index_chain_push
                    let path_kind = #path_kind;
                    let bytes = self.cmd_tx.send_read_receive_reply(
                        path_kind,
                        #timeout_val
                    ).await?;
                    let mut rd = BufReader::new(&bytes);
                    #des
                    Ok(value)
                }
            }
        };
        let prop_read_default_timout = prop_read(true);
        // let prop_read = prop_read(false);
        quote! {
            #prop_read_default_timout
            // #prop_read
        }
    } else {
        quote! {}
    };

    quote! {
        #write_fns
        #read_fns
    }
}

fn ser_write_value(model: ClientModel, prop_name: &Ident, ty: &Type) -> (TokenStream, TokenStream) {
    // unit - use empty slice directly
    if let Type::Tuple(elements) = ty
        && elements.is_empty()
    {
        return if model.no_alloc() {
            (quote! { () }, quote! { &[] })
        } else {
            (quote! { () }, quote! { Vec::new() })
        };
    }

    // byte slice - use directly
    if let Type::Vec(inner) = ty
        && matches!(inner.as_ref(), Type::U8)
    {
        return if model.no_alloc() {
            (quote! { &[u8] }, quote! {})
        } else {
            (quote! { Vec<u8> }, quote! {})
        };
    }

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
    let ser = quote! {
        let mut wr = BufWriter::new(&mut self.args_scratch);
        #ser
        let value = #finish_wr;
    };
    (ty_def, ser)
}

fn handle_stream(
    model: ClientModel,
    path_mode: ClientPathMode,
    full_gid_path: &TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    ty: &Type,
    is_up: bool,
) -> TokenStream {
    let sideband_fn_name = Ident::new(format!("{}_sideband", ident).as_str(), ident.span());
    let (ty_def, ser) = ser_write_value(model, &Ident::new("value", Span::call_site()), ty);
    let path_kind = path_kind(path_mode, full_gid_path);

    let specific_methods = if is_up {
        // client in
        let subscribe_fn = Ident::new(format!("{}_sub", ident).as_str(), ident.span());
        quote! {
            pub fn #subscribe_fn(&mut self) -> Result<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>, wire_weaver_client_common::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let _data = self.cmd_tx.send_stream_open(path_kind, tx)?;
                Ok(rx)
            }
        }
    } else {
        // client out
        let publish_fn = Ident::new(format!("{}_pub", ident).as_str(), ident.span());
        quote! {
            pub fn #publish_fn(&mut self, value: #ty_def) -> Result<(), wire_weaver_client_common::Error> {
                #ser
                #index_chain_push
                let path_kind = #path_kind;
                let _data = self.cmd_tx.send_write_forget(path_kind, value)?;
                Ok(())
            }
            // TOD: client stream out?
        }
    };
    quote! {
        #specific_methods

        pub async fn #sideband_fn_name(&self, sideband_cmd: StreamSidebandCommand) -> Result</*StreamSidebandEvent*/ (), wire_weaver_client_common::Error> {
            #index_chain_push
            let path_kind = #path_kind;

            Ok(())
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
        let mut args_list = quote! { #(#idents: #tys),* };
        if !args.is_empty() {
            args_list.extend(quote! { , });
        }
        let args_names = quote! { #(#idents),* };
        (args_ser, args_list, args_names, quote! { args_bytes })
    }
}

fn cmd_tx_disconnect_methods() -> TokenStream {
    quote! {
        pub async fn disconnect_and_exit(&mut self) -> Result<(), wire_weaver_client_common::Error> {
            let (cmd, done_rx) = wire_weaver_client_common::Command::disconnect_and_exit();
            self.cmd_tx
                .send(cmd)
                .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            let _ = done_rx.await.map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            Ok(())
        }

        pub fn disconnect_and_exit_non_blocking(&mut self) -> Result<(), wire_weaver_client_common::Error> {
            self.cmd_tx
                .send(wire_weaver_client_common::Command::DisconnectAndExit {
                    disconnected_tx: None,
                })
                .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            Ok(())
        }

        /// Disconnect from a connected device. Event loop will be left running, and error mode will be set to KeepRetrying.
        pub fn disconnect_keep_streams_non_blocking(&mut self) -> Result<(), wire_weaver_client_common::Error> {
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
