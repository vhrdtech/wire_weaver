use darling::FromMeta;
use darling::ast::NestedMeta;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Result, Token};

pub(crate) struct ImplArgs {
    pub(crate) trait_source: LitStr,
    _colon_colon: Token![::],
    pub(crate) trait_name: Ident,
    _for: Token![for],
    pub(crate) context_ident: Ident,
    _comma: Token![,],
    // pub(crate) tail: proc_macro2::TokenStream,
    pub(crate) ext: ImplExtArgs,
}

#[derive(Debug, FromMeta)]
pub(crate) struct ImplExtArgs {
    #[darling(default)]
    pub(crate) client: bool,
    #[darling(default)]
    pub(crate) raw_client: bool,
    #[darling(default)]
    pub(crate) server: bool,
    pub(crate) no_alloc: bool,
    pub(crate) use_async: bool,
    #[darling(default)]
    pub(crate) debug_to_file: String,
    #[darling(default)]
    pub(crate) derive: String,
    #[darling(default)]
    pub(crate) method_model: String,
    #[darling(default)]
    pub(crate) property_model: String,
}

impl Parse for ImplArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(ImplArgs {
            trait_source: input.parse()?,
            _colon_colon: input.parse()?,
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
