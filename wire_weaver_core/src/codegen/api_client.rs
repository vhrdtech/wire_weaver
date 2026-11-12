//! # Implementation details:
//! * Client's index chain contains all indices up to last level (resource IDs + array index if used)
use crate::codegen::CodegenBundle;
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
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Batching, Delivery,
    ErrorVariantOwned, FieldsOwned, FlowControl, Multiplicity, PropertyAccess, StreamFnRef,
    TypeOwned,
};

/// API client code generation configuration.
pub struct GenClientConfig {
    /// Kind of client to generate.
    pub model: ClientModel,
    /// 'impl <client_struct_path> { client methods }' code will be generated.
    /// Pass e.g., "crate::MyDeviceClient".
    pub client_struct_path: String,
    /// How the generated code names crates it doesn't depend on (API-60), from [crate::load_dep_with_paths];
    /// default: every crate by its own name.
    pub crate_paths: crate::transform::CratePaths,
    /// Sub-level traits reached through a client generated once with [ClientModel::StdSubLevelClient] (CLT-68),
    /// instead of a client of their own in this one.
    pub shared_sub_levels: Vec<SharedSubLevel>,
}

/// A sub-level trait whose client is generated once with [ClientModel::StdSubLevelClient] and shared by the clients
/// of several lines (CLT-68): `ww_indication::Indication => crate::IndicationClient`.
#[derive(Clone, Debug)]
pub struct SharedSubLevel {
    /// The crate of the trait, as Rust names it (`ww_indication`).
    pub crate_name: String,
    /// Modules from the crate's root to the trait, empty at the root (`["common"]` of `thermo_api::common::Channel`).
    pub modules: Vec<String>,
    pub trait_name: String,
    /// The shared client type (`crate::IndicationClient`), what the accessor returns.
    pub client: Path,
}

impl SharedSubLevel {
    /// `ww_indication::Indication`, as written.
    pub fn trait_path(&self) -> String {
        [self.crate_name.as_str()]
            .into_iter()
            .chain(self.modules.iter().map(String::as_str))
            .chain([self.trait_name.as_str()])
            .collect::<Vec<_>>()
            .join("::")
    }
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
    /// How the generated code names crates it doesn't depend on (API-60), from [crate::load_dep_with_paths];
    /// default: every crate by its own name.
    pub crate_paths: crate::transform::CratePaths,
    /// See [GenClientConfig::shared_sub_levels].
    pub shared_sub_levels: Vec<SharedSubLevel>,
}

impl From<GenClientConfig> for GenClientConfigRaw {
    fn from(config: GenClientConfig) -> Self {
        Self {
            model: config.model,
            client_struct_path: super::util::str_to_path(&config.client_struct_path),
            crate_paths: config.crate_paths,
            shared_sub_levels: config.shared_sub_levels,
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
    /// A client of one sub-level trait reached by path (CLT-68), `client = "sub_level"`: the struct named by
    /// `client_struct_path` (a plain identifier) is generated too, holding the trait's absolute path, which each
    /// line's client sets when it reaches the trait (`shared_sub_levels`). One type for every line that has the
    /// trait with the same signature; no global paths, the device is unchanged.
    StdSubLevelClient,
}

impl ClientModel {
    pub fn no_alloc(&self) -> bool {
        match self {
            ClientModel::Raw => true,
            ClientModel::StdFullClient => false,
            ClientModel::StdTraitClient => false,
            ClientModel::StdSubLevelClient => false,
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
    api_bundle: &ww_self::ApiBundleOwned,
    config: impl Into<GenClientConfigRaw>,
) -> TokenStream {
    let config: GenClientConfigRaw = config.into();
    let api_bundle = &CodegenBundle::new(api_bundle, &config.crate_paths);
    let additional_use = if matches!(
        config.model,
        ClientModel::StdFullClient | ClientModel::StdTraitClient | ClientModel::StdSubLevelClient
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

    let shared = &config.shared_sub_levels;
    if !shared.is_empty() && path_mode == ClientPathMode::GlobalTrait {
        let msg = "sub_levels: a trait client's paths start at its trait (global paths), a path-reached shared \
                   sub-level client can't be used in one (CLT-68)";
        return quote! { compile_error!(#msg); };
    }
    let mut shared_use = quote! {};
    let (top, index_chain) = if config.model == ClientModel::StdSubLevelClient {
        let Some(ident) = client_struct_path.get_ident() else {
            let msg = "client = \"sub_level\" generates the client struct itself: name it with a plain identifier, \
                       `for IndicationClient`";
            return quote! { compile_error!(#msg); };
        };
        let mod_name = util::mod_name(api_level, api_bundle);
        shared_use = quote! { pub use #mod_name::#ident; };
        (Top::Shared(ident.clone()), IndexChain::dynamic())
    } else {
        (Top::UserRoot(&client_struct_path), IndexChain::new())
    };
    let trait_clients = client_structs_recursive(
        &Ctx {
            api_bundle,
            model: config.model,
            path_mode,
            shared,
        },
        api_level,
        index_chain,
        top,
        None,
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
        #shared_use
    }
}

/// What every level of one client's generation shares.
struct Ctx<'a, 'b> {
    api_bundle: &'a CodegenBundle<'b>,
    model: ClientModel,
    path_mode: ClientPathMode,
    shared: &'a [SharedSubLevel],
}

/// Where a level's client struct comes from.
enum Top<'a> {
    /// The API root: `impl` on the user's struct.
    UserRoot(&'a Path),
    /// The trait of a shared sub-level client (CLT-68): the struct is generated, named so.
    Shared(Ident),
    /// A nested level: a generated struct named after its module.
    Nested,
}

/// The shared client of `level`'s trait, when this client reaches it through one (CLT-68).
fn shared_for<'s>(
    shared: &'s [SharedSubLevel],
    api_bundle: &CodegenBundle,
    level: &ApiLevelOwned,
) -> Option<&'s SharedSubLevel> {
    let crate_name = level.crate_name(api_bundle).ok()?.replace('-', "_");
    let modules = api_bundle.trait_module(&crate_name, &level.trait_name);
    shared.iter().find(|s| {
        s.crate_name == crate_name && s.trait_name == level.trait_name && s.modules == modules
    })
}

/// Signature of a trait as a shared sub-level client sees it (CLT-68): its shape signature (as for skipped definitions,
/// `ww_self::signature`, docs left out) with every `#[since]` cleared. A shared trait has a revision per line
/// (`#[since = "w1.2, w2.1"]`, EVO-37) and none loaded on its own; the client doesn't need one, since the per-call
/// check resolves the path in the line's own bundle. `trait_idx`: the trait in `bundle.traits`, `None` for the root.
pub fn shared_signature(
    bundle: &ww_self::ApiBundleOwned,
    trait_idx: Option<u32>,
) -> anyhow::Result<u64> {
    use ww_self::visit_mut::{self, VisitMut};
    struct Clear;
    impl VisitMut for Clear {
        fn visit_api_item(&mut self, node: &mut ApiItemOwned) {
            node.since = None;
            visit_mut::visit_api_item(self, node);
        }
        fn visit_field(&mut self, node: &mut ww_self::FieldOwned) {
            node.since = None;
            visit_mut::visit_field(self, node);
        }
        fn visit_variant(&mut self, node: &mut ww_self::VariantOwned) {
            node.since = None;
            visit_mut::visit_variant(self, node);
        }
    }
    let mut bundle = bundle.clone();
    let trait_idx = match trait_idx {
        Some(idx) => idx,
        None => {
            let crate_idx = bundle.root.crate_idx;
            bundle.traits.push(ww_self::ApiLevelLocationOwned::InLine {
                level: bundle.root.clone(),
                crate_idx,
            });
            bundle.traits.len() as u32 - 1
        }
    };
    Clear.visit_api_bundle(&mut bundle);
    let signature =
        ww_self::signature::trait_signatures(&bundle, trait_idx, &ww_self::signature::no_resolve)?;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&signature.shape[..8]);
    Ok(u64::from_le_bytes(bytes))
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

fn client_structs_recursive(
    ctx: &Ctx,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    top: Top,
    root_gid_paths: Option<&GidPaths>,
) -> TokenStream {
    let Ctx {
        api_bundle, model, ..
    } = *ctx;
    let mut ts = TokenStream::new();
    // a level under resource arrays too: each element is observed by its own path (API-63)
    let observable =
        model != ClientModel::Raw && !client_observe::observable_props(api_level).is_empty();
    let observe_types = if observable {
        client_observe::level_types(api_bundle, api_level)
    } else {
        quote! {}
    };
    let args_structs = args_structs(api_bundle, api_level, model.no_alloc());

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
    let trait_module = api_bundle.trait_module_path(api_level);
    let trait_name = LitStr::new(&api_level.trait_name, Span::call_site());
    let gid_paths = (
        quote! { #trait_module::#full_gid },
        quote! { #trait_module::#compact_gid },
        quote! { #trait_name },
    );
    // index chains of nested levels start from the root trait of this client, so global paths name that trait
    let root_gid_paths = root_gid_paths.unwrap_or(&gid_paths);
    let methods = level_methods(ctx, api_level, index_chain, root_gid_paths, observable);

    let mut maybe_signature = quote! {};
    let mut maybe_doc = quote! {};
    // call before increment_length so that root level does not have it
    let maybe_index_chain_field = index_chain.struct_field_def();

    let mut child_ts = TokenStream::new();
    for item in &api_level.items {
        if !matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
            continue;
        }
        let level = item.get_as_level(api_bundle).unwrap();
        if shared_for(ctx.shared, api_bundle, level).is_some() {
            // reached through the shared client (CLT-68), see level_method
            continue;
        }
        let mut index_chain = index_chain;
        index_chain.increment_length();
        let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
        if is_array {
            index_chain.increment_length();
        }
        child_ts.extend(client_structs_recursive(
            ctx,
            level,
            index_chain,
            Top::Nested,
            Some(root_gid_paths),
        ));
    }

    let trait_name = LitStr::new(&api_level.trait_name, Span::call_site());
    let index_chain = if matches!(top, Top::UserRoot(_)) {
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

    let impl_new_or_user_struct = if let Top::UserRoot(client_struct_path) = top {
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
        let client_struct_name = match top {
            Top::Shared(ident) => {
                // checked by each line's client that reaches the trait through this one (CLT-68)
                let signature = match shared_signature(api_bundle, None) {
                    Ok(s) => quote! { #s },
                    Err(e) => {
                        let msg = format!("signature of `{}`: {e:#}", api_level.trait_name);
                        quote! { compile_error!(#msg) }
                    }
                };
                let doc = format!(
                    " Client of `{}` reached by path, shared by the clients of every line that has it with the same \
                     signature (`sub_levels = [.. => ..]`, CLT-68).",
                    api_level.trait_name
                );
                maybe_signature = quote! {
                    impl #ident<'_> {
                        /// The trait's signature (docs and `#[since]` left out), checked against each line's at
                        /// compile time.
                        pub const WW_SIGNATURE: u64 = #signature;
                    }
                };
                maybe_doc = quote! { #[doc = #doc] };
                ident
            }
            _ => client_struct_name,
        };
        let struct_and_impls = quote! {
            pub struct #client_struct_name<'i> {
                #maybe_index_chain_field
                pub cmd: &'i wire_weaver_client::Commander,
            }

            impl<'i> #client_struct_name<'i> {
                #methods
                #attachment
            }
        };
        quote! {
            #maybe_doc
            #struct_and_impls

            #maybe_signature
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
    ctx: &Ctx,
    api_level: &ApiLevelOwned,
    index_chain: IndexChain,
    gid_paths: &GidPaths,
    observable: bool,
) -> TokenStream {
    let (api_bundle, model, path_mode) = (ctx.api_bundle, ctx.model, ctx.path_mode);
    // the level's own path, for observing it
    let level_chain = observable.then(|| {
        if index_chain.is_empty() {
            quote! { let index_chain: [UNib32; 0] = []; }
        } else if index_chain.is_dynamic() {
            quote! { let index_chain = self.index_chain.clone(); }
        } else {
            quote! { let index_chain = self.index_chain; }
        }
    });
    let handlers = api_level.items.iter().map(|item| {
        level_method(
            ctx,
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
    ctx: &Ctx,
    api_level: &ApiLevelOwned,
    item: &ApiItemOwned,
    mut index_chain: IndexChain,
    model: ClientModel,
    path_mode: ClientPathMode,
    gid_paths: &GidPaths,
    level_chain: Option<&TokenStream>,
) -> TokenStream {
    let api_bundle = ctx.api_bundle;
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
            if let Some(shared) = shared_for(ctx.shared, api_bundle, level) {
                return shared_accessor(
                    api_bundle,
                    item,
                    shared,
                    index_chain_push,
                    maybe_index_arg,
                )
                .unwrap_or_else(|e| {
                    let msg = format!("sub_levels `{}`: {e:#}", shared.trait_path());
                    quote! { compile_error!(#msg); }
                });
            }
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
    api_bundle: &CodegenBundle,
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
        pub fn #ident(&self, #args_list) -> wire_weaver_client::PreparedCall<#output_ty> {
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    api_bundle: &CodegenBundle,
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
    // a timed batched stream's items come with their own time, the full point of its Tm (CLT-70)
    let time_ty = match (stream.batched, stream.batch_time_ty()) {
        (Some(Batching::Timed), Some(tm)) => {
            Some(ty_def(api_bundle, tm, true, TyPos::Arg).unwrap())
        }
        _ => None,
    };
    let handle_args = match &time_ty {
        Some(tm) => quote! { #item_ty, <#tm as wire_weaver_client::ww_si::TimePoint>::Full },
        None => quote! { #item_ty },
    };
    let (handle_ty, delivery) = match stream.delivery {
        Delivery::Plain => (quote! { wire_weaver_client::Stream<#handle_args> }, None),
        Delivery::Counted => (
            quote! { wire_weaver_client::PositionedStream<#handle_args> },
            Some(quote! { wire_weaver_client::ww_self::Delivery::Counted }),
        ),
        Delivery::Reliable => (
            quote! { wire_weaver_client::PositionedStream<#handle_args> },
            Some(quote! { wire_weaver_client::ww_self::Delivery::Reliable }),
        ),
    };
    let args = stream.args.to_vec();
    let (args_ser, args_list, _) = ser_args(api_bundle, ident, &args, model.no_alloc());
    let path_kind = path_kind(path_mode, gid_paths);
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
        Some(Batching::Timed) => match &time_ty {
            Some(tm) => quote! { .batched(wire_weaver_client::unpack_timed::<#item_ty, #tm>()) },
            None => {
                let e = format!(
                    "batched stream {ident}: its model has no Timed<T, Tm> item type, rebuild it with this wire_weaver"
                );
                quote! { compile_error!(#e) }
            }
        },
        Some(Batching::Untimed) => {
            quote! { .batched(wire_weaver_client::unpack_untimed::<#item_ty>()) }
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
        pub fn #ident(&self #maybe_index_arg, #args_list) -> wire_weaver_client::PreparedStreamOpen<#result_ty, #handle_ty> {
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
    api_bundle: &CodegenBundle,
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
    let status = if credit {
        let status = Ident::new(&format!("{ident}_status"), ident.span());
        let resume = Ident::new(&format!("{ident}_resume"), ident.span());
        let resume_blocking = Ident::new(&format!("{ident}_resume_blocking"), ident.span());
        let blocking_doc = format!("Blocking variant of [Self::{resume}].");
        quote! {
            /// Continue the credit sink session of `cursor` (saved from `Sink::cursor()`) after a restart of the host
            /// program: returns the sink with the position where the device is, at or after `cursor.pos`, user code
            /// continues its own data from there. `StreamError::SinkSessionLost` when the device no longer has the
            /// session (it rebooted).
            pub async fn #resume(&self #maybe_index_arg, cursor: wire_weaver_client::StreamCursor) -> Result<(wire_weaver_client::Sink<#ty_def>, u64), wire_weaver_client::StreamError> {
                #index_chain_push
                let path_kind = #path_kind;
                let mut sink = self.cmd.prepare_credit_sink(path_kind).await?;
                let pos = sink.resume(cursor).await?;
                Ok((sink, pos))
            }

            #[doc = #blocking_doc]
            pub fn #resume_blocking(&self #maybe_index_arg, cursor: wire_weaver_client::StreamCursor) -> Result<(wire_weaver_client::Sink<#ty_def>, u64), wire_weaver_client::StreamError> {
                #index_chain_push
                let path_kind = #path_kind;
                let mut sink = self.cmd.prepare_credit_sink_blocking(path_kind)?;
                let pos = sink.resume_blocking(cursor)?;
                Ok((sink, pos))
            }

            /// State of the credit sink on the device: open or not, session, position, bytes buffered and capacity.
            pub fn #status(&self #maybe_index_arg) -> wire_weaver_client::PreparedRead<wire_weaver_client::ww_client_server::stream::StreamStatus> {
                #index_chain_push
                let path_kind = #path_kind;
                self.cmd.prepare_read(path_kind)
            }
        }
    } else {
        quote! {}
    };
    quote! {
        #sync_accessors
        #async_accessors
        #status
    }
}

fn ser_args(
    api_bundle: &CodegenBundle,
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

        // a `#[flatten]` argument is the args struct itself (EVO-22)
        let args_value = match args {
            [arg] if arg.is_flatten() => quote! { #(#idents)* },
            _ => quote! { #args_struct_ident { #(#idents),* } },
        };
        let args_ser = if no_alloc {
            quote! {
                let mut args_scratch = [0u8; 128]; // TODO: Vec based writer
                let args = #args_value;
                let args_bytes = args.to_ww_bytes(&mut args_scratch).map(|b| b.to_vec()).map_err(|e| e.into());
            }
        } else {
            quote! {
                let args = #args_value;
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

fn connect_disconnect_methods(api_bundle: &CodegenBundle) -> TokenStream {
    let IntrospectTs {
        introspect_bytes,
        no_docs_hash,
        with_docs_hash,
    } = introspect_prepare(
        api_bundle,
        true,
        crate::codegen::api_server::IntrospectCompression::None,
    );
    // API crate name and version, same constant the device reports, and the wire version link setup decides on
    let root = &api_bundle.root;
    let crate_name = api_bundle.trait_module_path(root);
    let full_gid = Ident::new(
        format!("{}_FULL_GID", root.trait_name)
            .to_case(Case::Constant)
            .as_str(),
        Span::call_site(),
    );
    // the API root's wire version, what link setup decides on (EVO-25): a full client needs one
    let wire = match api_bundle.root_wire_version() {
        Some(wire) => {
            let (line, revision) = (wire.line.0, wire.revision.0);
            quote! { wire_weaver::ww_version::WireVersion::new(#line, #revision) }
        }
        None => {
            let msg = format!(
                "a client of `{}` needs the wire version of its API root, link setup decides on it (EVO-25): mark \
                 the trait `#[ww_api_root]` instead of `#[ww_trait]` and declare it in the crate's Cargo.toml, \
                 `[package.metadata.wire_weaver]` with `wire = \"w1.0\"` (EVO-31)",
                root.trait_name
            );
            quote! { compile_error!(#msg) }
        }
    };
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
                .client_version(#crate_name::#full_gid.make_owned(), #wire)
        }

        /// The wire line this client speaks, for a host bundling clients of several lines of this API
        /// (`ClientConfig::speaks`, EVO-28).
        pub fn client_line() -> wire_weaver_client::ClientLine {
            wire_weaver_client::ClientLine {
                version: #crate_name::#full_gid.make_owned(),
                wire: #wire,
                ww_self_bytes: Self::introspect_bytes(),
                api_hash: Self::api_hash().make_owned(),
            }
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

/// Accessor of a sub-level reached through a shared client (CLT-68), with the compile-time check that the shared
/// client was generated from the same trait as this line has.
fn shared_accessor(
    api_bundle: &CodegenBundle,
    item: &ApiItemOwned,
    shared: &SharedSubLevel,
    index_chain_push: TokenStream,
    maybe_index_arg: TokenStream,
) -> anyhow::Result<TokenStream> {
    let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind else {
        anyhow::bail!("not a trait");
    };
    let signature = shared_signature(api_bundle, Some(trait_idx.0))?;
    let client = &shared.client;
    let ident = Ident::new(&item.ident, Span::call_site());
    let line = match api_bundle.root_wire_version() {
        Some(w) => format!(" (w{}.{})", w.line.0, w.revision.0),
        None => String::new(),
    };
    let msg = format!(
        "`{}` was generated from another `{}` than `{}::{}`{line} has: generate a client per line for it, or bring \
         both to one version of the trait (CLT-68)",
        quote!(#client).to_string().replace(' ', ""),
        shared.trait_path(),
        api_bundle.root.trait_name,
        item.ident,
    );
    Ok(quote! {
        pub fn #ident(&self #maybe_index_arg) -> #client<'_> {
            const _: () = assert!(<#client<'static>>::WW_SIGNATURE == #signature, #msg);
            #index_chain_push
            #client {
                index_chain: index_chain.to_vec(),
                cmd: &self.cmd,
            }
        }
    })
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
