use darling::FromMeta;
use darling::ast::NestedMeta;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Result, Token};
use wire_weaver_core::ast::trait_macro_args::ImplTraitLocation;

pub(crate) struct ApiArgs {
    pub(crate) location: ImplTraitLocation,
    _colon_colon: Token![::],
    pub(crate) trait_name: Ident,
    _for: Token![for],
    pub(crate) context_ident: Ident,
    _comma: Token![,],
    pub(crate) ext: ImplExtArgs,
}

#[derive(Debug, FromMeta)]
pub(crate) struct ImplExtArgs {
    #[darling(default)]
    pub(crate) client: String,

    #[darling(default)]
    pub(crate) server: bool,

    pub(crate) no_alloc: bool,
    pub(crate) use_async: bool,

    #[darling(default)]
    pub(crate) method_model: String,

    #[darling(default)]
    pub(crate) property_model: String,

    #[darling(default)]
    pub(crate) debug_to_file: String,

    #[darling(default)]
    pub(crate) introspect: bool,
}

impl Parse for ApiArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let location = input.parse()?;
        if !matches!(location, ImplTraitLocation::SameFile) {
            let _: Token![::] = input.parse()?;
        }
        Ok(ApiArgs {
            location,
            _colon_colon: Default::default(),
            trait_name: input.parse()?,
            _for: input.parse()?,
            context_ident: input.parse()?,
            _comma: input.parse()?,
            ext: input.parse()?,
        })
    }
}

impl Parse for ImplExtArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let ts: proc_macro2::TokenStream = input.parse()?;
        let attr_args = NestedMeta::parse_meta_list(ts)?;
        let ext_args = match ImplExtArgs::from_list(&attr_args) {
            Ok(v) => v,
            Err(e) => return Err(e.into()),
        };
        Ok(ext_args)
    }
}
