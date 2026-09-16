use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::LitStr;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Version {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) patch: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Cfg(pub(crate) LitStr);

impl ToTokens for Cfg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature = &self.0;
        tokens.extend(quote! { #[cfg(feature = #feature)] });
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CfgAttrDefmt(pub(crate) LitStr);

impl ToTokens for CfgAttrDefmt {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature = &self.0;
        tokens.extend(quote! { #[cfg_attr(feature = #feature, derive(defmt::Format))] });
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CfgAttrSerde(pub(crate) LitStr);

impl ToTokens for CfgAttrSerde {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature = &self.0;
        tokens.extend(quote! { #[cfg_attr(feature = #feature, derive(serde::Deserialize, serde::Serialize))] });
    }
}
