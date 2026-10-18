//! # Implementation details:
//! * Client's index chain contains all indices up to last level (resource IDs + array index if used)
use crate::codegen::api_common::args_structs;
use crate::codegen::client::observe as client_observe;
use crate::codegen::index_chain::IndexChain;
use crate::codegen::server::introspect::{IntrospectTs, introspect_prepare};
use crate::codegen::ty_def::{TyPos, ty_def, ty_def_by_idx};
use crate::codegen::util;
use crate::codegen::util::maybe_quote;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{LitStr, Path};
use ww_self::errors::{error_list_docs, fmt_error_list, method_err_ty};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Batching,
    Delivery, ErrorVariantOwned, FieldsOwned, FlowControl, Multiplicity, PropertyAccess,
    StreamFnRef, TypeOwned,
};

/// API client code generation configuration.
pub struct GenClientConfig {
    /// Kind of client to generate.
    pub model: ClientModel,
    /// 'impl <client_struct_path> { client methods }' code will be generated.
    /// Pass e.g., "crate::MyDeviceClient".
    pub client_struct_path: String,
}

/// API client code generation configuration.
/// Same as [GenClientConfig], but with syn::Path instead of String for `client_struct_path`.
/// More convenient to use from proc-macro context.
pub struct GenClientConfigRaw {
    /// Kind of client to generate.
    pub model: ClientModel,
    /// 'impl <client_struct_path> { client methods }' code will be generated.
    /// Pass e.g., "crate::MyDeviceClient".
    pub client_struct_path: Path,
}

impl From<GenClientConfig> for GenClientConfigRaw {
    fn from(config: GenClientConfig) -> Self {
        Self {
            model: config.model,
            client_struct_path: super::util::str_to_path(&config.client_struct_path),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum ClientModel {
    /// Prepare ww_client_server::Request and return it.
    /// Generates no_std, no_alloc and sync code.
    /// Currently NOT working.
    Raw,
    /// Prepare ww_client_server::Request, convert it to RequestOwned and
    /// send through wire_weaver_client::Commander to a worker thread.
    /// Generates std, sync + async code that allocates.
    StdFullClient,
    /// Similar to StdFullClient, but uses different addressing mode starting from traits, and not API root.
    /// Useful when generating client code for traits, e.g., for Gpio, regardless of where it actually might be implemented.
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

/// Generates API client code for the given API bundle and configuration.
/// ApiBundleOwned can be loaded using [crate::load] or [crate::load_dep].
/// Pass a [GenClientConfig] or [GenClientConfigRaw] to configure code generation.
///
/// Alternatively, use `wire_weaver_derive::ww_codegen!` proc-macro if you do not want to use build.rs.
pub fn gen_client(
    api_bundle: &ApiBundleOwned,
    config: impl Into<GenClientConfigRaw>,
) -> TokenStream {
    let config = config.into();
    let additional_use = if matches!(
        config.model,
        ClientModel::StdFullClient | ClientModel::StdTraitClient
    ) {
        quote! { #[allow(unused_imports)] use wire_weaver_client::ww_client_server::PathKind; }
    } else {
        quote! {}
    };
    let api_level = &api_bundle.root;
    let client_struct_path = config.client_struct_path;
    let hl_init = if config.model == ClientModel::StdFullClient {
        let d = connect_disconnect_methods(api_bundle);
        quote! {
            impl #client_struct_path {
                #d
            }
        }
    } else {
        quote! {}
    };
    let path_mode = if matches!(config.model, ClientModel::StdTraitClient) {
        ClientPathMode::GlobalTrait
    } else {
        ClientPathMode::Absolute
    };

    // let root_mod_name = api_level.mod_ident(Some(ext_crate_name));
    // let root_client_struct_name = api_level.client_struct_name(Some(ext_crate_name));
    let trait_clients = client_structs_recursive(
        api_bundle,
        api_level,
        IndexChain::new(),
        config.model,
        path_mode,
        Some(&client_struct_path),
        None,
        false,
    );
    quote! {
        #[allow(unused_imports)]
        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrap, BufReader, BufWriter, traits::ElementSize,
            Error as ShrinkWrapError, nib32::UNib32, RefVec
        };
        #[allow(unused_imports)]
        use wire_weaver::{ww_version, ValidIndicesOwned};
        #[allow(unused_imports)]
        use wire_weaver_client::StreamEvent;
        #[allow(unused_imports)]
        use wire_weaver_client::ww_client_server::StreamSideband;
        #additional_use

        #hl_init

        #trait_clients
    }
}

/// Paths to a trait's `FULL_GID` and `COMPACT_GID` constants, and its name.
type GidPaths = (TokenStream, TokenStream, TokenStream);

fn client_struct_name(mod_name: &str) -> Ident {
    Ident::new(
        format!("{}_client", mod_name)
            .to_case(Case::Pascal)
            .as_str(),
        Span::call_site(),
    )
}

#[allow(clippy::too_many_arguments)]
fn client_structs_recursive(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    is_at_root: Option<&Path>,
    root_gid_paths: Option<&GidPaths>,
    under_array: bool,
) -> TokenStream {
    let mut ts = TokenStream::new();
    // observing a level under a resource array isn't possible yet (SRV-18)
    let observable = model != ClientModel::Raw
        && !under_array
        && !client_observe::observable_props(api_level).is_empty();
    let observe_types = if observable {
        client_observe::level_types(api_bundle, api_level)
    } else {
        quote! {}
    };
    let args_structs = args_structs(api_bundle, api_level, model.no_alloc());

    let crate_name = api_level.crate_name(api_bundle).unwrap();
    let mod_name = util::mod_name(api_level, api_bundle);
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
    let trait_name = LitStr::new(&api_level.trait_name, Span::call_site());
    let gid_paths = (
        quote! { #crate_name::#full_gid },
        quote! { #crate_name::#compact_gid },
        quote! { #trait_name },
    );
    // index chains of nested levels start from the root trait of this client, so global paths name that trait
    let root_gid_paths = root_gid_paths.unwrap_or(&gid_paths);
    let methods = level_methods(
        api_bundle,
        api_level,
        index_chain,
        model,
        path_mode,
        root_gid_paths,
        observable,
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
        let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
        if is_array {
            index_chain.increment_length();
        }
        child_ts.extend(client_structs_recursive(
            api_bundle,
            level,
            index_chain,
            model,
            path_mode,
            None,
            Some(root_gid_paths),
            under_array || is_array,
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
        pub fn attachment(&self) -> wire_weaver_client::Attachment {
            let mut cmd = self.cmd.clone();
            cmd.set_base_path(#index_chain);
            wire_weaver_client::Attachment::new(
                cmd,
                #full.make_owned(),
                #trait_name.to_string()
            )
        }
    };

    let impl_new_or_user_struct = if let Some(client_struct_path) = is_at_root {
        let path = if let Some(first) = client_struct_path.segments.first()
            && first.ident == "super"
        {
            quote! { super::#client_struct_path }
        } else {
            quote! { #client_struct_path }
        };
        quote! {
            impl #path {
                #methods
                #attachment
            }
        }
    } else {
        quote! {
            pub struct #client_struct_name<'i> {
                #maybe_index_chain_field
                pub cmd: &'i wire_weaver_client::Commander,
            }

            impl<'i> #client_struct_name<'i> {
                #methods
                #attachment
            }
        }
    };
    let source_marker = util::source_marker(api_level, api_bundle);
    ts.extend(quote! {
        // pub: users name the level's observe types (`<Trait>Observe`, `<Trait>Update`)
        pub mod #mod_name {
            #source_marker
            use super::*;

            use wire_weaver::shrink_wrap::prelude::*;
            #args_structs
            #observe_types

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
    gid_paths: &GidPaths,
    observable: bool,
) -> TokenStream {
    // the level's own path, for observing it
    let level_chain = observable.then(|| {
        if index_chain.is_empty() {
            quote! { let index_chain: [UNib32; 0] = []; }
        } else {
            quote! { let index_chain = self.index_chain; }
        }
    });
    let handlers = api_level.items.iter().map(|item| {
        level_method(
            api_bundle,
            api_level,
            item,
            index_chain,
            model,
            path_mode,
            gid_paths,
            level_chain.as_ref(),
        )
    });
    let observe_fn = level_chain.as_ref().map(|level_chain| {
        client_observe::level_observe_fn(
            api_bundle,
            api_level,
            level_chain.clone(),
            &path_kind(path_mode, gid_paths),
        )
    });
    quote! {
        #(#handlers)*
        #observe_fn
    }
}

#[allow(clippy::too_many_arguments)]
fn level_method(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    item: &ApiItemOwned,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    level_chain: Option<&TokenStream>,
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
            let ty = ty_def_by_idx(api_bundle, type_idx.0, false, TyPos::Arg).unwrap();
            (quote! { #index_chain_push_pre #p }, quote! { , index: #ty })
        }
    };
    let ident = Ident::new(&item.ident, Span::call_site());
    // names the item in CallError::Unimplemented
    let resource = format!("{}::{}", api_level.trait_name, item.ident);
    let lm = match &item.kind {
        ApiItemKindOwned::Method {
            args,
            return_ty,
            errors,
        } => handle_method(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            &ident,
            args,
            return_ty,
            errors.as_deref(),
            &item.docs,
            &resource,
        ),
        ApiItemKindOwned::Property {
            access,
            ty,
            write_err_ty,
            errors,
            batched,
            ..
        } => handle_property(
            api_bundle,
            api_level,
            // observed through the level, see client::observe
            level_chain.filter(|_| item.multiplicity == Multiplicity::Flat),
            model,
            path_mode,
            gid_paths,
            index_chain_push,
            access,
            batched.is_some(),
            &ident,
            ty,
            write_err_ty,
            errors.as_deref(),
            &resource,
        ),
        ApiItemKindOwned::Stream { is_up: true, .. } | ApiItemKindOwned::StreamFn { .. } => {
            handle_stream_fn(
                api_bundle,
                model,
                path_mode,
                gid_paths,
                maybe_index_arg,
                index_chain_push,
                &ident,
                item.kind.as_stream_fn().expect("a stream"),
                &item.docs,
                &resource,
            )
        }
        ApiItemKindOwned::Stream { ty, is_up: false } => handle_stream(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            maybe_index_arg,
            index_chain_push,
            &ident,
            ty,
            false,
            false,
        ),
        ApiItemKindOwned::FlowSink { ty, flow } => handle_stream(
            api_bundle,
            model,
            path_mode,
            gid_paths,
            maybe_index_arg,
            index_chain_push,
            &ident,
            ty,
            false,
            matches!(flow, FlowControl::Credit),
        ),
        ApiItemKindOwned::Trait { .. } => {
            let level = item.get_as_level(api_bundle).expect("api level");
            let level_entry_fn_name = Ident::new(&item.ident, Span::call_site());
            let mod_name = util::mod_name(level, api_bundle);
            let client_struct_name = client_struct_name(&mod_name.to_string());
            quote! {
                pub fn #level_entry_fn_name(&self #maybe_index_arg) -> #mod_name::#client_struct_name<'_> {
                    #index_chain_push
                    #mod_name::#client_struct_name {
                        index_chain,
                        cmd: &self.cmd,
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
            pub fn #read_fn_name(&self) -> wire_weaver_client::PreparedRead<ValidIndicesOwned> {
                #index_chain_push_pre
                let path_kind = #path_kind;
                self.cmd.prepare_read(path_kind)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_method(
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    index_chain_push: TokenStream,
    ident: &Ident,
    args: &[ArgumentOwned],
    return_type: &Option<TypeOwned>,
    errors: Option<&[ErrorVariantOwned]>,
    docs: &[String],
    resource: &str,
) -> TokenStream {
    let (args_ser, args_list, _args_names) = ser_args(api_bundle, ident, args, model.no_alloc());
    // a method returning Result<T, E> gives Result<T, CallError<E>> instead of Result<Result<T, E>, Error>
    let output_ty = match return_type {
        Some(return_type) => match return_type.get_in_line(api_bundle).unwrap() {
            TypeOwned::Result { ok_ty, err_ty } => {
                let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).unwrap();
                let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
                quote! { #ok_ty, wire_weaver_client::CallError<#err_ty> }
            }
            _ => ty_def(api_bundle, return_type, true, TyPos::Arg).unwrap(),
        },
        None => quote! { () },
    };

    let path_kind = path_kind(path_mode, gid_paths);
    let maybe_mut = maybe_quote(!args.is_empty(), quote! { mut });
    let mut docs = docs.to_vec();
    let mut with_error_list = quote! {};
    if let Some(errors) = errors
        && let Some(err_ty) = method_err_ty(return_type.as_ref(), api_bundle)
    {
        docs.extend(error_list_doc(errors, err_ty, api_bundle));
        let listed = error_list_patterns(errors, err_ty, api_bundle);
        let names = fmt_error_list(errors);
        with_error_list = quote! {
            .with_error_list(wire_weaver_client::ErrorList {
                listed: |e| match e {
                    wire_weaver_client::CallError::Device(e) => matches!(e, #listed),
                    _ => true,
                },
                names: #names,
            })
        };
    }
    let docs = docs.iter().map(|s| quote! { #[doc = #s] });
    quote! {
        #(#docs)*
        pub fn #ident(& #maybe_mut self, #args_list) -> wire_weaver_client::PreparedCall<#output_ty> {
            #args_ser
            #index_chain_push
            let path_kind = #path_kind;
            self.cmd.prepare_call(path_kind, args_bytes, #resource) #with_error_list
        }
    }
}

/// `# Errors` section of a method's, stream function's or property write's docs from its `#[errors(..)]` list
/// (API-32), each entry with its variant's doc line.
fn error_list_doc(
    errors: &[ErrorVariantOwned],
    err_ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
) -> Vec<String> {
    let mut docs = vec![String::new(), "# Errors".into(), String::new()];
    for (name, doc) in error_list_docs(errors, err_ty, api_bundle) {
        docs.push(match doc {
            Some(doc) => format!("- `{name}`: {doc}"),
            None => format!("- `{name}`"),
        });
    }
    docs
}

/// Patterns of the listed variants of `err_ty` (API-32), for a `matches!` in `.with_error_list(..)`, so that the
/// client warns when a device returns another one.
fn error_list_patterns(
    errors: &[ErrorVariantOwned],
    err_ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
) -> TokenStream {
    let err_path = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
    let variants = match err_ty.get_in_line(api_bundle) {
        Ok(TypeOwned::Enum(item_enum)) => item_enum.variants.as_slice(),
        _ => &[],
    };
    let mut patterns = vec![];
    for entry in errors {
        let variant = Ident::new(&entry.ident, Span::call_site());
        match &entry.nested {
            None => patterns.push(quote! { #err_path::#variant { .. } }),
            Some(nested) => {
                let field_ty = variants
                    .iter()
                    .find(|v| v.ident == entry.ident)
                    .and_then(|v| match &v.fields {
                        FieldsOwned::Unnamed(fields) if fields.len() == 1 => Some(&fields[0].ty),
                        _ => None,
                    })
                    .expect("checked when the trait was parsed");
                let inner_path = ty_def(api_bundle, field_ty, true, TyPos::Arg).unwrap();
                for n in nested {
                    let n = Ident::new(n, Span::call_site());
                    patterns.push(quote! { #err_path::#variant(#inner_path::#n { .. }) });
                }
            }
        }
    }
    quote! { #(#patterns)|* }
}

#[allow(clippy::too_many_arguments)]
fn handle_property(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    level_chain: Option<&TokenStream>,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    index_chain_push: TokenStream,
    access: &PropertyAccess,
    batched: bool,
    prop_name: &Ident,
    ty: &TypeOwned,
    user_result_ty: &Option<TypeOwned>,
    errors: Option<&[ErrorVariantOwned]>,
    resource: &str,
) -> TokenStream {
    let path_kind = path_kind(path_mode, gid_paths);
    let def_ty = ty_def(api_bundle, ty, !model.no_alloc(), TyPos::Def).unwrap();
    let ty = ty_def(api_bundle, ty, !model.no_alloc(), TyPos::Arg).unwrap();

    let write_fns = if matches!(
        access,
        PropertyAccess::ReadWrite { .. } | PropertyAccess::WriteOnly
    ) {
        let write_fn_name = Ident::new(&format!("write_{}", prop_name), Span::call_site());
        let write_err_ty = if let Some(ty) = user_result_ty {
            let ty = ty_def(api_bundle, ty, !model.no_alloc(), TyPos::Arg).unwrap();
            quote! { wire_weaver_client::CallError<#ty> }
        } else {
            quote! { wire_weaver_client::Error }
        };
        let ser_value = if model.no_alloc() {
            quote! {
                let mut args_scratch = [0u8; 128]; // TODO: Vec based writer
                let value = #prop_name.to_ww_bytes(&mut args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
            }
        } else {
            quote! {
                let value = #prop_name.to_ww_bytes_owned().map_err(|e| e.into());
            }
        };
        let mut docs = vec![];
        let mut with_error_list = quote! {};
        if let (Some(errors), Some(err_ty)) = (errors, user_result_ty) {
            docs = error_list_doc(errors, err_ty, api_bundle);
            docs.remove(0); // no docs above the section
            let listed = error_list_patterns(errors, err_ty, api_bundle);
            let names = fmt_error_list(errors);
            with_error_list = quote! {
                .with_error_list(wire_weaver_client::ErrorList {
                    listed: |e| match e {
                        wire_weaver_client::CallError::Device(e) => matches!(e, #listed),
                        _ => true,
                    },
                    names: #names,
                })
            };
        }
        let docs = docs.iter().map(|s| quote! { #[doc = #s] });
        quote! {
            #(#docs)*
            pub fn #write_fn_name(&self, #prop_name: #ty) -> wire_weaver_client::PreparedWrite<#write_err_ty> {
                #ser_value
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_write(path_kind, value, #resource) #with_error_list
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
        // a const is read once per connection, then answered locally (API-36)
        let (prepare_read, doc) = if matches!(access, PropertyAccess::Const) {
            (
                quote! { prepare_const_read },
                quote! { #[doc = " A const: read from the device once per connection, later reads return that value."] },
            )
        } else {
            (quote! { prepare_read }, quote! {})
        };
        quote! {
            #doc
            pub fn #read_fn_name(&self) -> wire_weaver_client::PreparedRead<#ty> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.#prepare_read(path_kind)
            }
        }
    } else {
        quote! {}
    };

    let observe_fns = if let (
        Some(level_chain),
        PropertyAccess::ReadWrite { observe: true } | PropertyAccess::ReadOnly { observe: true },
    ) = (level_chain, access)
    {
        client_observe::property_observe_fn(
            api_bundle,
            api_level,
            prop_name,
            &def_ty,
            batched,
            level_chain.clone(),
            &path_kind,
        )
    } else if matches!(
        access,
        PropertyAccess::ReadWrite { .. } | PropertyAccess::ReadOnly { .. }
    ) {
        // without `+observe`: change notifications a device sends by hand as stream data on the property's path
        let observe = Ident::new(&format!("observe_{}", prop_name), Span::call_site());
        let observe_blocking = Ident::new(
            &format!("observe_{}_blocking", prop_name),
            Span::call_site(),
        );
        quote! {
            /// Property updates sent by the device (with generated `stream_data_ser().<property>_send()`).
            pub async fn #observe(&self) -> Result<wire_weaver_client::Stream<#ty>, wire_weaver_client::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_stream(path_kind).await
            }

            /// Property updates sent by the device (with generated `stream_data_ser().<property>_send()`).
            pub fn #observe_blocking(&self) -> Result<wire_weaver_client::Stream<#ty>, wire_weaver_client::Error> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_stream_blocking(path_kind)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #write_fns
        #read_fns
        #observe_fns
    }
}

/// A stream (device to host) is opened by calling it (API-31): `name(args)` prepares the open, whose `.call()`
/// returns the stream with the stream function's result; counted and reliable streams also get
/// `name_resume(cursor)` and `name_status()`, the read on the stream.
#[allow(clippy::too_many_arguments)]
fn handle_stream_fn(
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    maybe_index_arg: TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    stream: StreamFnRef<'_>,
    docs: &[String],
    resource: &str,
) -> TokenStream {
    let item_ty = if stream.ty.is_byte_slice(api_bundle).unwrap() {
        quote! { wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned }
    } else {
        ty_def(api_bundle, stream.ty, !model.no_alloc(), TyPos::Arg).unwrap()
    };
    let result_ty = match stream.return_ty {
        Some(ty) => ty_def(api_bundle, ty, true, TyPos::Arg).unwrap(),
        None => quote! { () },
    };
    let (handle_ty, delivery) = match stream.delivery {
        Delivery::Plain => (quote! { wire_weaver_client::Stream<#item_ty> }, None),
        Delivery::Counted => (
            quote! { wire_weaver_client::PositionedStream<#item_ty> },
            Some(quote! { wire_weaver_client::ww_self::Delivery::Counted }),
        ),
        Delivery::Reliable => (
            quote! { wire_weaver_client::PositionedStream<#item_ty> },
            Some(quote! { wire_weaver_client::ww_self::Delivery::Reliable }),
        ),
    };
    let args = stream.args.to_vec();
    let (args_ser, args_list, _) = ser_args(api_bundle, ident, &args, model.no_alloc());
    let path_kind = path_kind(path_mode, gid_paths);
    let maybe_mut = maybe_quote(!args.is_empty(), quote! { mut });
    let mut docs = docs.to_vec();
    let mut with_error_list = quote! {};
    if let Some(errors) = stream.errors
        && let Some(err_ty) = method_err_ty(stream.return_ty, api_bundle)
    {
        docs.extend(error_list_doc(errors, err_ty, api_bundle));
        let listed = error_list_patterns(errors, err_ty, api_bundle);
        let names = fmt_error_list(errors);
        with_error_list = quote! {
            .with_error_list(wire_weaver_client::ErrorList {
                listed: |e| matches!(e, #listed),
                names: #names,
            }, #resource)
        };
    }
    // a batched stream's events are batches: the handle unpacks them into items (API-41)
    let batched = match stream.batched {
        None => quote! {},
        Some(Batching::Timed) => {
            quote! { .batched(wire_weaver_client::unpack_timed::<#item_ty>) }
        }
        Some(Batching::Untimed) => {
            quote! { .batched(wire_weaver_client::unpack_untimed::<#item_ty>) }
        }
        Some(Batching::SharedClock) => {
            let e = format!("batched stream {ident}: SharedClock is the batching of properties");
            quote! { compile_error!(#e) }
        }
        Some(Batching::Unknown(_)) => {
            let e = format!(
                "batched stream {ident}: a form of batching this wire_weaver doesn't know, update it"
            );
            quote! { compile_error!(#e) }
        }
    };
    let docs = docs.iter().map(|s| quote! { #[doc = #s] });
    let delivery_opt = match &delivery {
        Some(d) => quote! { Some(#d) },
        None => quote! { None },
    };
    let mut ts = quote! {
        #(#docs)*
        ///
        /// Opens the stream: `.call().await` returns the stream function's result with the stream.
        pub fn #ident(& #maybe_mut self #maybe_index_arg, #args_list) -> wire_weaver_client::PreparedStreamOpen<#result_ty, #handle_ty> {
            #args_ser
            #index_chain_push
            let path_kind = #path_kind;
            self.cmd.prepare_stream_open(path_kind, #delivery_opt, args_bytes) #with_error_list #batched
        }
    };
    if let Some(delivery) = delivery {
        let resume = Ident::new(&format!("{ident}_resume"), ident.span());
        let status = Ident::new(&format!("{ident}_status"), ident.span());
        ts.extend(quote! {
            /// Resume the stream from `cursor` (saved with the data), with the arguments its session was opened with
            /// on the device.
            pub fn #resume(&self #maybe_index_arg, cursor: wire_weaver_client::ww_client_server::stream::StreamCursor) -> wire_weaver_client::PreparedStreamOpen<(), #handle_ty> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_stream_resume(path_kind, #delivery, cursor) #batched
            }

            /// State of the stream on the device: open or not, session, positions, store use.
            pub fn #status(&self #maybe_index_arg) -> wire_weaver_client::PreparedRead<wire_weaver_client::ww_client_server::stream::StreamStatus> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_read(path_kind)
            }
        });
    }
    ts
}

#[allow(clippy::too_many_arguments)]
fn handle_stream(
    api_bundle: &ApiBundleOwned,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    maybe_index_arg: TokenStream,
    index_chain_push: TokenStream,
    ident: &Ident,
    ty: &TypeOwned,
    is_up: bool,
    credit: bool,
) -> TokenStream {
    let ty_def = if ty.is_byte_slice(api_bundle).unwrap() {
        quote! { wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned }
    } else {
        ty_def(api_bundle, ty, !model.no_alloc(), TyPos::Arg).unwrap()
    };
    let path_kind = path_kind(path_mode, gid_paths);

    let stream_accessors = |is_async| {
        let ident = if is_async {
            ident.clone()
        } else {
            Ident::new(&format!("{}_blocking", ident), ident.span())
        };
        let maybe_async = maybe_quote(is_async, quote! { async });
        let maybe_await = maybe_quote(is_async, quote! { .await });
        if is_up {
            // client in
            let prepare_fn = if is_async {
                quote! { prepare_stream }
            } else {
                quote! { prepare_stream_blocking }
            };
            quote! {
                pub #maybe_async fn #ident(&self #maybe_index_arg) -> Result<wire_weaver_client::Stream<#ty_def>, wire_weaver_client::Error> {
                    #index_chain_push
                    let path_kind = #path_kind;
                    let stream = self.cmd.#prepare_fn(path_kind) #maybe_await ?;
                    Ok(stream)
                }
            }
        } else {
            // client out
            let prepare_fn = match (is_async, credit) {
                (true, false) => quote! { prepare_sink },
                (false, false) => quote! { prepare_sink_blocking },
                (true, true) => quote! { prepare_credit_sink },
                (false, true) => quote! { prepare_credit_sink_blocking },
            };
            quote! {
                pub #maybe_async fn #ident(&self #maybe_index_arg) -> Result<wire_weaver_client::Sink<#ty_def>, wire_weaver_client::Error> {
                    #index_chain_push
                    let path_kind = #path_kind;
                    let sink = self.cmd.#prepare_fn(path_kind) #maybe_await ?;
                    Ok(sink)
                }
            }
        }
    };
    let sync_accessors = stream_accessors(false);
    let async_accessors = stream_accessors(true);
    quote! {
        #sync_accessors
        #async_accessors
    }
}

fn ser_args(
    api_bundle: &ApiBundleOwned,
    method_ident: &Ident,
    args: &[ArgumentOwned],
    no_alloc: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let args_struct_ident =
        crate::codegen::api_common::args_struct_ident(&method_ident.to_string(), no_alloc);
    if args.is_empty() {
        if no_alloc {
            (
                // nothing to send: if arguments are added later, the evolved args struct reads them as None/empty from no bytes
                quote! { let args_bytes = &[]; },
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

        let args_ser = if no_alloc {
            quote! {
                let mut args_scratch = [0u8; 128]; // TODO: Vec based writer
                let args = #args_struct_ident { #(#idents),* };
                let args_bytes = args.to_ww_bytes(&mut args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
            }
        } else {
            quote! {
                let args = #args_struct_ident { #(#idents),* };
                let args_bytes = args.to_ww_bytes_owned().map_err(|e| e.into());
            }
        };
        let tys: Result<Vec<TokenStream>, _> = args
            .iter()
            .map(|arg| ty_def(api_bundle, &arg.ty, !no_alloc, TyPos::Arg))
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

fn connect_disconnect_methods(api_bundle: &ApiBundleOwned) -> TokenStream {
    let IntrospectTs {
        introspect_bytes,
        no_docs_hash,
        with_docs_hash,
    } = introspect_prepare(api_bundle, true);
    // API crate name and version, same constant the device reports, checked for compatibility during link setup
    let root = &api_bundle.root;
    let crate_name = Ident::new(root.crate_name(api_bundle).unwrap(), Span::call_site());
    let full_gid = Ident::new(
        format!("{}_FULL_GID", root.trait_name)
            .to_case(Case::Constant)
            .as_str(),
        Span::call_site(),
    );
    quote! {
        pub fn new() -> wire_weaver_client::PreparedConnection<Self> {
            wire_weaver_client::PreparedConnection::new(Self::generated_config())
        }

        pub fn config<C: Fn(wire_weaver_client::ClientConfig) -> wire_weaver_client::ClientConfig>(c: C) -> wire_weaver_client::PreparedConnection<Self> {
            let config = c(Self::generated_config());
            wire_weaver_client::PreparedConnection::new(config)
        }

        fn generated_config() -> wire_weaver_client::ClientConfig {
            <Self as wire_weaver_client::WwClient>::default_config()
                .introspect_client(Self::introspect_bytes(), Self::api_hash().make_owned())
                .client_version(#crate_name::#full_gid.make_owned())
        }

        fn introspect_bytes() -> &'static [u8] {
            const WW_SELF_BYTES: #introspect_bytes;
            &WW_SELF_BYTES
        }

        fn api_hash() -> wire_weaver::ww_version::ApiHashPair<'static> {
            const WW_API_HASH_NO_DOCS: #no_docs_hash;
            const WW_API_HASH_WITH_DOCS: #with_docs_hash;
            use wire_weaver::ww_version::ApiHash;
            wire_weaver::ww_version::ApiHashPair {
                no_docs: ApiHash::new(&WW_API_HASH_NO_DOCS),
                with_docs: ApiHash::new(&WW_API_HASH_WITH_DOCS),
            }
        }

        /// Disconnect from a device, call `.asynch().await`, `.blocking()` or other methods in the returned PreparedDisconnect`, `.blocking()` or other methods in the returned PreparedDisconnect.
        pub fn disconnect(&self) -> wire_weaver_client::PreparedDisconnect {
            self.cmd.disconnect()
        }
    }
}

/// Creates ww_client_server::PathKind in user context
fn path_kind(path_mode: ClientPathMode, gid_paths: &GidPaths) -> TokenStream {
    let (full, compact, trait_name) = gid_paths;
    match path_mode {
        ClientPathMode::Absolute => {
            quote! { PathKind::absolute(&index_chain) }
        }
        ClientPathMode::GlobalTrait => quote! {
            PathKind::global(#full, #compact, #trait_name, &index_chain)
        },
    }
}
