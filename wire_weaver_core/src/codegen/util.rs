use crate::codegen::CodegenBundle;
use crate::handler_model::{HandlerKind, HandlerModel};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Path, PathArguments, PathSegment};
use ww_self::ApiLevelOwned;

pub fn maybe_quote(condition: bool, tokens_if_true: TokenStream) -> TokenStream {
    if condition {
        tokens_if_true
    } else {
        TokenStream::new()
    }
}

pub fn maybe_quote_cl<F: FnMut() -> TokenStream>(
    condition: bool,
    mut call_if_true: F,
) -> TokenStream {
    if condition {
        call_if_true()
    } else {
        TokenStream::new()
    }
}

pub fn add_prefix(prefix: Option<&String>, ident: &Ident) -> Ident {
    match prefix {
        Some(prefix) => Ident::new(format!("{}_{}", prefix, ident).as_str(), ident.span()),
        None => ident.clone(),
    }
}

/// Step `step` of an item of `kind` (`method`, `property`, ..), a constant of `ww_client_server::err_seq::<kind>`,
/// or `KIND_NOT_TAKEN` (SRV-22).
pub(crate) fn item_err_seq(kind: &str, step: &str) -> TokenStream {
    let step = Ident::new(step, Span::call_site());
    if kind.is_empty() {
        quote! { ww_client_server::err_seq::#step }
    } else {
        let kind = Ident::new(kind, Span::call_site());
        quote! { ww_client_server::err_seq::#kind::#step }
    }
}

/// A level or request site, a constant of `ww_client_server::err_seq` (SRV-22): `LEVEL_BAD_PATH`, ..
pub(crate) fn fixed_err_seq(name: &str) -> TokenStream {
    let name = Ident::new(name, Span::call_site());
    quote! { ww_client_server::err_seq::#name }
}

pub(crate) fn mod_name(api_level: &ApiLevelOwned, api_bundle: &CodegenBundle) -> Ident {
    let crate_name = api_level.crate_name(api_bundle).unwrap();
    Ident::new(
        format!(
            "{}_{}",
            crate_name,
            api_level.trait_name.to_case(Case::Snake)
        )
        .as_str(),
        Span::call_site(),
    )
}

pub(crate) fn source_marker(api_level: &ApiLevelOwned, api_bundle: &CodegenBundle) -> TokenStream {
    let crate_name = api_bundle.crate_path(api_level.crate_name(api_bundle).unwrap());
    let trait_full_gid = Ident::new(
        &format!("{}_FULL_GID", api_level.trait_name).to_case(Case::Constant),
        Span::call_site(),
    );
    quote! {
        #[allow(unused_imports)]
        use #crate_name::#trait_full_gid as _SOURCE_MARKER;
    }
}

pub(crate) fn str_to_path(path: &str) -> Path {
    Path {
        leading_colon: None,
        segments: path
            .split("::")
            .map(|s| PathSegment {
                ident: Ident::new(s, Span::call_site()),
                arguments: PathArguments::None,
            })
            .collect(),
    }
}

/// How the generated server calls user handlers (SRV-21): `async fn` ones with `.await`, plain `fn` ones without,
/// as `handler_model` (or `use_async` for all) says, through the checks in `wire_weaver::handler`.
#[derive(Copy, Clone)]
pub(crate) struct Handlers<'a> {
    pub(crate) model: &'a HandlerModel,
    pub(crate) use_async: bool,
}

impl Handlers<'_> {
    /// Whether the handlers of the item at the dotted `path` are `async fn`.
    pub(crate) fn is_async(&self, path: &str) -> bool {
        self.model.pick(path, self.use_async) == HandlerKind::Async
    }

    /// `call` (`self.get_x(cx)`) of a handler of the item at `path`, awaited if it's async.
    pub(crate) fn call(&self, path: &str, call: TokenStream) -> TokenStream {
        if self.is_async(path) {
            quote! { wire_weaver::handler::r#async(#call).await }
        } else {
            quote! { wire_weaver::handler::sync(#call) }
        }
    }
}

/// Dotted path of an item, as `handler_model` and `omit` see it: `device.settings.vbus_mode`.
pub(crate) fn dotted(level_path: &[String], ident: &str) -> String {
    let mut s = String::new();
    for name in level_path {
        s.push_str(name);
        s.push('.');
    }
    s.push_str(ident);
    s
}
