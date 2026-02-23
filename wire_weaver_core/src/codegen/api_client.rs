//! # Implementation details:
//! * Client's index chain contains all indices up to last level (resource IDs + array index if used)
use crate::ast::api::{ApiItem, ApiItemKind, ApiLevel, Argument, Multiplicity, PropertyAccess};
use crate::codegen::api_common::args_structs;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::util::{maybe_call_since, maybe_quote};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use shrink_wrap_core::ast::path::Path;
use shrink_wrap_core::ast::{Docs, Type, Version};
use syn::LitStr;

#[derive(Copy, Clone, PartialEq)]
pub enum ClientModel {
    /// Prepare ww_client_server::Request and return it.
    /// Generates no_std, no_alloc and sync code.
    Raw,
    /// Prepare ww_client_server::Request, convert it to RequestOwned and
    /// send through wire_weaver_client_common::CommandSender to a worker thread.
    /// Generates std, async code that allocates.
    StdFullClient,
    StdTraitClient,
}

impl ClientModel {
    pub fn no_alloc(&self) -> bool {
        match self {
            ClientModel::Raw => true,
            ClientModel::StdFullClient => false,
            ClientModel::StdTraitClient => false,
        }
    }
}

/// Determines which resource path will be used in generated client code
#[derive(Copy, Clone, PartialEq)]
pub(crate) enum ClientPathMode {
    /// Absolute paths from user API root
    Absolute,
    /// Global full or compact paths starting from trait root (when used with client = "trait_client")
    GlobalTrait,
}

pub fn client(
    api_level: &ApiLevel,
    model: ClientModel,
    client_struct: &Ident,
    usb_connect: bool,
) -> TokenStream {
    let additional_use = if matches!(
        model,
        ClientModel::StdFullClient | ClientModel::StdTraitClient
    ) {
        quote! { use wire_weaver_client_common::ww_client_server::PathKind; }
    } else {
        quote! {}
    };
    let hl_init = if model == ClientModel::StdFullClient {
        let d = connect_disconnect_methods(usb_connect, api_level);
        quote! {
            impl super::#client_struct {
                #d
            }
        }
    } else {
        quote! {}
    };
    let path_mode = if matches!(model, ClientModel::StdTraitClient) {
        ClientPathMode::GlobalTrait
    } else {
        ClientPathMode::Absolute
    };

    let crate_name = api_level.source_location.crate_name();
    // let root_mod_name = api_level.mod_ident(Some(ext_crate_name));
    // let root_client_struct_name = api_level.client_struct_name(Some(ext_crate_name));
    let trait_clients = client_structs_recursive(
        api_level,
        IndexChain::new(),
        crate_name,
        model,
        path_mode,
        Some(client_struct),
    );
    quote! {
        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib32::UNib32, RefVec
        };
        use wire_weaver::ww_version;
        use wire_weaver_client_common::StreamEvent;
        use wire_weaver_client_common::ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
        #additional_use

        #hl_init

        #trait_clients
    }
}

fn client_structs_recursive(
    api_level: &ApiLevel,
    index_chain: IndexChain,
    crate_name: &Ident,
    model: ClientModel,
    path_mode: ClientPathMode,
    is_at_root: Option<&Ident>,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let args_structs = args_structs(api_level, model.no_alloc());

    let mod_name = api_level.mod_ident(crate_name);
    let use_external =
        api_level.use_external_types(Path::new_ident(crate_name.clone()), model.no_alloc());
    let client_struct_name = api_level.client_struct_name(crate_name);
    let gid_paths = api_level.gid_paths();
    let methods = level_methods(
        api_level,
        index_chain,
        model,
        path_mode,
        &gid_paths,
        is_at_root.is_some(),
    );

    // call before increment_length so that root level does not have it
    let maybe_index_chain_field = index_chain.struct_field_def();

    let mut child_ts = TokenStream::new();
    for item in &api_level.items {
        let ApiItemKind::ImplTrait { args: _, level } = &item.kind else {
            continue;
        };
        let level = level.as_ref().expect("empty level");
        let mut index_chain = index_chain;
        index_chain.increment_length();
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
        child_ts.extend(client_structs_recursive(
            level,
            index_chain,
            level.source_location.crate_name(),
            model,
            path_mode,
            None,
        ));
    }

    let trait_name = LitStr::new(&api_level.name.to_string(), api_level.name.span());
    let index_chain = if is_at_root.is_some() {
        quote! { vec![] }
    } else {
        quote! { self.index_chain.to_vec() }
    };
    let full = &gid_paths.0;
    let attachment = quote! {
        pub fn attachment(&self) -> wire_weaver_client_common::Attachment {
            let mut cmd_tx = self.cmd_tx.clone();
            cmd_tx.set_base_path(#index_chain);
            wire_weaver_client_common::Attachment::new(
                cmd_tx,
                #full,
                #trait_name
            )
        }
    };

    let impl_new_or_user_struct = if let Some(client_struct) = is_at_root {
        quote! {
            impl super::super::#client_struct {
                #methods
                #attachment
            }
        }
    } else {
        quote! {
            pub struct #client_struct_name<'i> {
                #maybe_index_chain_field
                pub args_scratch: &'i mut [u8],
                pub cmd_tx: &'i mut wire_weaver_client_common::CommandSender,
            }

            impl<'i> #client_struct_name<'i> {
                #methods
                #attachment
            }
        }
    };
    ts.extend(quote! {
        mod #mod_name {
            use super::*;
            #use_external
            #args_structs

            #impl_new_or_user_struct

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
    gid_paths: &(TokenStream, TokenStream),
    is_at_root: bool,
) -> TokenStream {
    let handlers = api_level.items.iter().map(|item| {
        level_method(
            item,
            index_chain,
            model,
            path_mode,
            gid_paths,
            is_at_root,
            api_level.source_location.crate_name(),
        )
    });
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    item: &ApiItem,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    is_at_root: bool,
    api_crate: &Ident,
) -> TokenStream {
    let id = item.id;
    let index_chain_push = index_chain.push_back(quote! { self. }, quote! { UNib32(#id) });
    let (index_chain_push, maybe_index_arg) = match &item.multiplicity {
        Multiplicity::Flat => (index_chain_push, quote! {}),
        Multiplicity::Array { index_type: None } => {
            let p = index_chain.push_back(quote! {}, quote! { UNib32(index) });
            (quote! { #index_chain_push #p }, quote! { , index: u32 })
        }
        Multiplicity::Array {
            index_type: Some(ty),
        } => {
            let p = index_chain.push_back(quote! {}, quote! { UNib32(index.into()) });
            (
                quote! { #index_chain_push #p },
                quote! { , index: #api_crate::#ty },
            )
        }
    };
    match &item.kind {
        ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => handle_method(
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            ident,
            args,
            return_type,
            &item.docs,
            &item.since,
        ),
        ApiItemKind::Property {
            access,
            ident,
            ty,
            user_result_ty,
        } => handle_property(
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            access,
            ident,
            ty,
            user_result_ty,
            &item.since,
        ),
        ApiItemKind::Stream { ident, ty, is_up } => handle_stream(
            model,
            path_mode,
            gid_paths,
            maybe_index_arg,
            index_chain_push,
            ident,
            ty,
            *is_up,
            &item.since,
        ),
        ApiItemKind::ImplTrait { args, level } => {
            let level_entry_fn_name = &args.resource_name;
            let level = level.as_ref().expect("api level");
            let crate_name = level.source_location.crate_name();
            let mod_name = level.mod_ident(crate_name);
            let client_struct_name = level.client_struct_name(crate_name);
            let maybe_ref_mut = maybe_quote(is_at_root, quote! { &mut });
            quote! {
                pub fn #level_entry_fn_name(&mut self #maybe_index_arg) -> #mod_name::#client_struct_name<'_> {
                    #index_chain_push
                    #mod_name::#client_struct_name {
                        index_chain,
                        args_scratch: #maybe_ref_mut self.args_scratch,
                        cmd_tx: #maybe_ref_mut self.cmd_tx,
                    }
                }
            }
        }
        ApiItemKind::Reserved => quote! {},
    }
}

fn handle_method(
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    index_chain_push: TokenStream,
    ident: &Ident,
    args: &[Argument],
    return_type: &Option<Type>,
    docs: &Docs,
    since: &Option<Version>,
) -> TokenStream {
    let (args_ser, args_list, _args_names) = ser_args(ident, args, model.no_alloc());
    let output_ty = if let Some(return_type) = &return_type {
        if let Type::External(ext_ty, lifetime) = return_type {
            if *lifetime {
                let mut ty_owned = ext_ty.clone();
                ty_owned.make_owned();
                let ty_owned = &ty_owned;
                quote! { #ty_owned }
            } else {
                return_type.def(model.no_alloc())
            }
        } else {
            return_type.def(model.no_alloc())
        }
    } else {
        quote! { () }
    };

    let path_kind = path_kind(path_mode, gid_paths);
    let maybe_mut = maybe_quote(!args.is_empty(), quote! { mut });
    let since = maybe_call_since(since);
    quote! {
        #docs
        pub fn #ident(& #maybe_mut self, #args_list) -> wire_weaver_client_common::PreparedCall<#output_ty> {
            #args_ser
            #index_chain_push
            let path_kind = #path_kind;
            self.cmd_tx.prepare_call(path_kind, args_bytes, #since)
        }
    }
}
fn handle_property(
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    index_chain_push: TokenStream,
    access: &PropertyAccess,
    prop_name: &Ident,
    ty: &Type,
    user_result_ty: &Option<Type>,
    since: &Option<Version>,
) -> TokenStream {
    let mut des = TokenStream::new();
    ty.buf_read(
        &Ident::new("value", Span::call_site()),
        model.no_alloc(),
        false,
        quote! { ? },
        &quote! { _ },
        &mut des,
    );
    let path_kind = path_kind(path_mode, gid_paths);
    let ty_def = ty.arg_pos_def2(model.no_alloc());
    let since = maybe_call_since(since);

    let write_fns = if matches!(
        access,
        PropertyAccess::ReadWrite | PropertyAccess::WriteOnly
    ) {
        let write_fn_name = Ident::new(&format!("write_{}", prop_name), prop_name.span());
        let user_result_ty = if let Some(ty) = user_result_ty {
            ty.arg_pos_def2(model.no_alloc())
        } else {
            quote! { () }
        };
        quote! {
            pub fn #write_fn_name(&mut self, #prop_name: #ty_def) -> wire_weaver_client_common::PreparedWrite<#user_result_ty> {
                let value = #prop_name.to_ww_bytes(&mut self.args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd_tx.prepare_write(path_kind, value, #since)
            }
        }
    } else {
        quote! {}
    };

    let read_fns = if matches!(
        access,
        PropertyAccess::Const | PropertyAccess::ReadWrite | PropertyAccess::ReadOnly
    ) {
        let read_fn_name = Ident::new(&format!("read_{}", prop_name), prop_name.span());
        quote! {
            pub fn #read_fn_name(&mut self) -> wire_weaver_client_common::PreparedRead<#ty_def> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd_tx.prepare_read(path_kind, #since)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #write_fns
        #read_fns
    }
}

fn handle_stream(
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    maybe_index_arg: TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    ty: &Type,
    is_up: bool,
    since: &Option<Version>,
) -> TokenStream {
    let ty_def = if ty.is_byte_slice() {
        quote! { wire_weaver::shrink_wrap::raw_slice::RawSliceOwned }
    } else {
        ty.arg_pos_def2(model.no_alloc())
    };
    let path_kind = path_kind(path_mode, gid_paths);
    let since = maybe_call_since(since);

    if is_up {
        // client in
        quote! {
            pub fn #ident(&mut self #maybe_index_arg) -> Result<wire_weaver_client_common::Stream<#ty_def>, wire_weaver_client_common::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                let stream = self.cmd_tx.prepare_stream(path_kind, #since)?;
                Ok(stream)
            }
        }
    } else {
        // client out
        quote! {
            pub fn #ident(&mut self #maybe_index_arg) -> Result<wire_weaver_client_common::Sink<#ty_def>, wire_weaver_client_common::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                let sink = self.cmd_tx.prepare_sink(path_kind, #since)?;
                Ok(sink)
            }
        }
    }
}

fn ser_args(
    method_ident: &Ident,
    args: &[Argument],
    no_alloc: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let args_struct_ident = Ident::new(
        format!("{}_args", method_ident)
            .to_case(Case::Pascal)
            .as_str(),
        method_ident.span(),
    );
    if args.is_empty() {
        if no_alloc {
            (
                quote! { let args_bytes = &[]; }, // TODO: &[0x00] when no arguments to allow adding them later, as Option?
                quote! {},
                quote! {},
            )
        } else {
            (
                quote! { let args_bytes = Ok(vec![]); },
                quote! {},
                quote! {},
            )
        }
    } else {
        let idents = args.iter().map(|arg| &arg.ident);

        // let maybe_to_vec = maybe_quote(!no_alloc, quote! { .to_vec() });
        let args_ser = quote! {
            let args = #args_struct_ident { #(#idents),* };
            let args_bytes = args.to_ww_bytes(&mut self.args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
        };
        let idents = args.iter().map(|arg| &arg.ident).collect::<Vec<_>>();
        let tys = args.iter().map(|arg| arg.ty.arg_pos_def(no_alloc));
        let mut args_list = quote! { #(#idents: #tys),* };
        if !args.is_empty() {
            args_list.extend(quote! { , });
        }
        let args_names = quote! { #(#idents),* };
        (args_ser, args_list, args_names)
    }
}

fn connect_disconnect_methods(usb_connect: bool, api_level: &ApiLevel) -> TokenStream {
    let (ww_self_bytes_const, api_signature_bytes) =
        crate::codegen::introspect::introspect(api_level);
    let connect_fn = |is_async: bool| {
        let maybe_async = maybe_quote(is_async, quote! { async });
        let maybe_await = maybe_quote(is_async, quote! { .await });
        let cmd_connect_fn = if is_async {
            quote! { connect }
        } else {
            quote! { connect_blocking }
        };
        let connect_fn = if is_async {
            quote! { connect_raw }
        } else {
            quote! { connect_raw_blocking }
        };
        quote! {
            pub #maybe_async fn #connect_fn(
                filter: wire_weaver_client_common::DeviceFilter,
                api_version: wire_weaver::ww_version::FullVersion<'static>,
                on_error: wire_weaver_client_common::OnError,
                local_timeout: std::time::Duration,
                scratch: [u8; 4096],
            ) -> Result<Self, wire_weaver_client_common::Error> {
                use tokio::sync::mpsc;
                let (transport_cmd_tx, transport_cmd_rx) = mpsc::unbounded_channel();
                let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
                let mut cmd_tx = wire_weaver_client_common::CommandSender::new(transport_cmd_tx, dispatcher_msg_rx);
                cmd_tx.set_local_timeout(local_timeout);
                tokio::spawn(async move {
                    wire_weaver_usb_host::usb_worker(transport_cmd_rx, dispatcher_msg_tx).await;
                });
                cmd_tx.#cmd_connect_fn(filter, api_version.into(), on_error)#maybe_await?;
                pub const WW_SELF_BYTES: #ww_self_bytes_const;
                pub const WW_API_SIGNATURE_BYTES: #api_signature_bytes;
                cmd_tx.set_client_introspect_bytes(&WW_SELF_BYTES, &WW_API_SIGNATURE_BYTES);
                Ok(Self {
                    args_scratch: scratch,
                    cmd_tx,
                })
            }
        }
    };
    let (connect_async, connect_blocking) = if usb_connect {
        (connect_fn(true), connect_fn(false))
    } else {
        (quote! {}, quote! {})
    };
    quote! {
        #connect_async
        #connect_blocking

        pub async fn disconnect_and_exit(&mut self) -> Result<(), wire_weaver_client_common::Error> {
            let (cmd, done_rx) = wire_weaver_client_common::Command::disconnect_and_exit();
            self.cmd_tx
                .send(cmd)
                .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            let _ = done_rx.await.map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            Ok(())
        }

        pub fn disconnect_and_exit_blocking(&mut self) -> Result<(), wire_weaver_client_common::Error> {
            let (cmd, done_rx) = wire_weaver_client_common::Command::disconnect_and_exit();
            self.cmd_tx
                .send(cmd)
                .map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            let _ = done_rx.blocking_recv().map_err(|_| wire_weaver_client_common::Error::EventLoopNotRunning)?;
            Ok(())
        }

        pub fn disconnect_and_exit_forget(&mut self) -> Result<(), wire_weaver_client_common::Error> {
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
fn path_kind(path_mode: ClientPathMode, gid_paths: &(TokenStream, TokenStream)) -> TokenStream {
    let full = &gid_paths.0;
    let compact = &gid_paths.1;
    match path_mode {
        ClientPathMode::Absolute => {
            quote! { PathKind::absolute(&index_chain) }
        }
        ClientPathMode::GlobalTrait => quote! {
            PathKind::global(#full, #compact, &index_chain)
        },
    }
}
