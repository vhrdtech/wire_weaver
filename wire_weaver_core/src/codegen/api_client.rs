//! # Implementation details:
//! * Client's index chain contains all indices up to last level (resource IDs + array index if used)
use crate::codegen::api_common::args_structs;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::server::introspect::introspect_prepare;
use crate::codegen::ty_def::{ty_def, ty_def_by_idx};
use crate::codegen::util;
use crate::codegen::util::maybe_quote;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::LitStr;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Multiplicity,
    PropertyAccess, TypeOwned,
};

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
    api_bundle: &ApiBundleOwned,
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
    let api_level = &api_bundle.root;
    let hl_init = if model == ClientModel::StdFullClient {
        let d = connect_disconnect_methods(usb_connect, api_bundle);
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

    let crate_name = api_level.crate_name(api_bundle).unwrap();
    // let root_mod_name = api_level.mod_ident(Some(ext_crate_name));
    // let root_client_struct_name = api_level.client_struct_name(Some(ext_crate_name));
    let trait_clients = client_structs_recursive(
        api_bundle,
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
        use wire_weaver::{ww_version, ValidIndicesOwned};
        use wire_weaver_client_common::StreamEvent;
        use wire_weaver_client_common::ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
        #additional_use

        #hl_init

        #trait_clients
    }
}

fn client_struct_name(mod_name: &str) -> Ident {
    Ident::new(
        format!("{}_client", mod_name)
            .to_case(Case::Pascal)
            .as_str(),
        Span::call_site(),
    )
}

fn client_structs_recursive(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    crate_name: &str,
    model: ClientModel,
    path_mode: ClientPathMode,
    is_at_root: Option<&Ident>,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let args_structs = args_structs(api_bundle, api_level, model.no_alloc());

    let mod_name = util::mod_name(crate_name, api_level);
    let client_struct_name = client_struct_name(&mod_name.to_string());
    let full_gid = Ident::new(
        format!("{}_FULL_GID", api_level.trait_name)
            .to_case(Case::Constant)
            .as_str(),
        Span::call_site(),
    );
    let compact_gid = Ident::new(
        format!("{}_COMPACT_GID", api_level.trait_name)
            .to_case(Case::Constant)
            .as_str(),
        Span::call_site(),
    );
    let crate_name = Ident::new(crate_name, Span::call_site());
    let gid_paths = (
        quote! { #crate_name::#full_gid },
        quote! { #crate_name::#compact_gid },
    );
    let methods = level_methods(
        api_bundle,
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
        if !matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
            continue;
        }
        let level = item.get_as_level(api_bundle).unwrap();
        let mut index_chain = index_chain;
        index_chain.increment_length();
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            index_chain.increment_length();
        }
        child_ts.extend(client_structs_recursive(
            api_bundle,
            level,
            index_chain,
            level.crate_name(api_bundle).unwrap(),
            model,
            path_mode,
            None,
        ));
    }

    let trait_name = LitStr::new(&api_level.trait_name, Span::call_site());
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
                #full.make_owned(),
                #trait_name.to_string()
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

            use wire_weaver::shrink_wrap::prelude::*;
            #args_structs

            #impl_new_or_user_struct

            #child_ts
        }
    });
    ts
}

fn level_methods(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    is_at_root: bool,
) -> TokenStream {
    let handlers = api_level.items.iter().map(|item| {
        level_method(
            api_bundle,
            item,
            index_chain,
            model,
            path_mode,
            gid_paths,
            is_at_root,
        )
    });
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    api_bundle: &ApiBundleOwned,
    item: &ApiItemOwned,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    is_at_root: bool,
) -> TokenStream {
    let id = item.id.0;
    let index_chain_push_pre = index_chain.push_back(quote! { self. }, quote! { UNib32(#id) });
    let (index_chain_push, maybe_index_arg) = match &item.multiplicity {
        Multiplicity::Flat => (index_chain_push_pre.clone(), quote! {}),
        Multiplicity::Array {
            index_type_idx: None,
        } => {
            let p = index_chain.push_back(quote! {}, quote! { UNib32(index) });
            (quote! { #index_chain_push_pre #p }, quote! { , index: u32 })
        }
        Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } => {
            let p = index_chain.push_back(quote! {}, quote! { UNib32(index.into()) });
            let ty = ty_def_by_idx(api_bundle, type_idx.0, false, true).unwrap();
            (quote! { #index_chain_push_pre #p }, quote! { , index: #ty })
        }
    };
    let ident = Ident::new(&item.ident, Span::call_site());
    let lm = match &item.kind {
        ApiItemKindOwned::Method { args, return_ty } => handle_method(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            &ident,
            args,
            return_ty,
            &item.docs,
        ),
        ApiItemKindOwned::Property {
            access,
            ty,
            write_err_ty,
        } => handle_property(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            access,
            &ident,
            ty,
            write_err_ty,
        ),
        ApiItemKindOwned::Stream { ty, is_up } => handle_stream(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            maybe_index_arg,
            index_chain_push,
            &ident,
            ty,
            *is_up,
        ),
        ApiItemKindOwned::Trait { .. } => {
            let level = item.get_as_level(api_bundle).expect("api level");
            let level_entry_fn_name = Ident::new(&item.ident, Span::call_site());
            let crate_name = level.crate_name(api_bundle).unwrap();
            let mod_name = util::mod_name(crate_name, level);
            let client_struct_name = client_struct_name(&mod_name.to_string());
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
    };
    if item.multiplicity == Multiplicity::Flat {
        lm
    } else {
        let read_fn_name = Ident::new(&format!("{}_valid_indices", item.ident), Span::call_site());
        let path_kind = path_kind(path_mode, gid_paths);
        quote! {
            #lm
            pub fn #read_fn_name(&mut self) -> wire_weaver_client_common::PreparedRead<ValidIndicesOwned> {
                #index_chain_push_pre
                let path_kind = #path_kind;
                self.cmd_tx.prepare_read(path_kind)
            }
        }
    }
}

fn handle_method(
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    index_chain_push: TokenStream,
    ident: &Ident,
    args: &[ArgumentOwned],
    return_type: &Option<TypeOwned>,
    docs: &[String],
) -> TokenStream {
    let (args_ser, args_list, _args_names) = ser_args(api_bundle, ident, args, model.no_alloc());
    let output_ty = if let Some(return_type) = &return_type {
        ty_def(api_bundle, return_type, true, true).unwrap()
    } else {
        quote! { () }
    };

    let path_kind = path_kind(path_mode, gid_paths);
    let maybe_mut = maybe_quote(!args.is_empty(), quote! { mut });
    let docs = docs.iter().map(|s| quote! { #[doc = #s] });
    quote! {
        #(#docs)*
        pub fn #ident(& #maybe_mut self, #args_list) -> wire_weaver_client_common::PreparedCall<#output_ty> {
            #args_ser
            #index_chain_push
            let path_kind = #path_kind;
            self.cmd_tx.prepare_call(path_kind, args_bytes)
        }
    }
}
fn handle_property(
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    index_chain_push: TokenStream,
    access: &PropertyAccess,
    prop_name: &Ident,
    ty: &TypeOwned,
    user_result_ty: &Option<TypeOwned>,
) -> TokenStream {
    let path_kind = path_kind(path_mode, gid_paths);
    let ty = ty_def(api_bundle, ty, !model.no_alloc(), true).unwrap();

    let write_fns = if matches!(
        access,
        PropertyAccess::ReadWrite { .. } | PropertyAccess::WriteOnly
    ) {
        let write_fn_name = Ident::new(&format!("write_{}", prop_name), Span::call_site());
        let user_result_ty = if let Some(ty) = user_result_ty {
            ty_def(api_bundle, ty, !model.no_alloc(), true).unwrap()
        } else {
            quote! { () }
        };
        quote! {
            pub fn #write_fn_name(&mut self, #prop_name: #ty) -> wire_weaver_client_common::PreparedWrite<#user_result_ty> {
                let value = #prop_name.to_ww_bytes(&mut self.args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd_tx.prepare_write(path_kind, value)
            }
        }
    } else {
        quote! {}
    };

    let read_fns = if matches!(
        access,
        PropertyAccess::Const | PropertyAccess::ReadWrite { .. } | PropertyAccess::ReadOnly { .. }
    ) {
        let read_fn_name = Ident::new(&format!("read_{}", prop_name), Span::call_site());
        quote! {
            pub fn #read_fn_name(&mut self) -> wire_weaver_client_common::PreparedRead<#ty> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd_tx.prepare_read(path_kind)
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
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &(TokenStream, TokenStream),
    maybe_index_arg: TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    ty: &TypeOwned,
    is_up: bool,
) -> TokenStream {
    let ty_def = if ty.is_byte_slice(api_bundle).unwrap() {
        quote! { wire_weaver::shrink_wrap::raw_slice::RawSliceOwned }
    } else {
        ty_def(api_bundle, ty, !model.no_alloc(), true).unwrap()
    };
    let path_kind = path_kind(path_mode, gid_paths);

    if is_up {
        // client in
        quote! {
            pub fn #ident(&mut self #maybe_index_arg) -> Result<wire_weaver_client_common::Stream<#ty_def>, wire_weaver_client_common::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                let stream = self.cmd_tx.prepare_stream(path_kind)?;
                Ok(stream)
            }
        }
    } else {
        // client out
        quote! {
            pub fn #ident(&mut self #maybe_index_arg) -> Result<wire_weaver_client_common::Sink<#ty_def>, wire_weaver_client_common::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                let sink = self.cmd_tx.prepare_sink(path_kind)?;
                Ok(sink)
            }
        }
    }
}

fn ser_args(
    api_bundle: &ApiBundleOwned,
    method_ident: &Ident,
    args: &[ArgumentOwned],
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
        let idents = args
            .iter()
            .map(|arg| Ident::new(&arg.ident, Span::call_site()))
            .collect::<Vec<_>>();

        // let maybe_to_vec = maybe_quote(!no_alloc, quote! { .to_vec() });
        let args_ser = quote! {
            let args = #args_struct_ident { #(#idents),* };
            let args_bytes = args.to_ww_bytes(&mut self.args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
        };
        let tys: Result<Vec<TokenStream>, _> = args
            .iter()
            .map(|arg| ty_def(api_bundle, &arg.ty, !no_alloc, true))
            .collect();
        let tys = tys.unwrap();
        let mut args_list = quote! { #(#idents: #tys),* };
        if !args.is_empty() {
            args_list.extend(quote! { , });
        }
        let args_names = quote! { #(#idents),* };
        (args_ser, args_list, args_names)
    }
}

fn connect_disconnect_methods(usb_connect: bool, api_bundle: &ApiBundleOwned) -> TokenStream {
    let (ww_self_bytes_const, api_signature_bytes) = introspect_prepare(api_bundle);
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
                cmd_tx.set_client_introspect_bytes(Self::introspect_bytes(), Self::api_signature());
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

        fn introspect_bytes() -> &'static [u8] {
            const WW_SELF_BYTES: #ww_self_bytes_const;
            &WW_SELF_BYTES
        }

        fn api_signature() -> &'static [u8] {
            const WW_API_SIGNATURE_BYTES: #api_signature_bytes;
            &WW_API_SIGNATURE_BYTES
        }

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
