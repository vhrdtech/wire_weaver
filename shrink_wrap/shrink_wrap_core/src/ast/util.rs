use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::LitStr;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Clone, Debug)]
pub struct Cfg(pub LitStr);

impl ToTokens for Cfg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature = &self.0;
        tokens.extend(quote! { #[cfg(feature = #feature)] });
    }
}

#[derive(Clone, Debug)]
pub struct CfgAttrDefmt(pub LitStr);

impl ToTokens for CfgAttrDefmt {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature = &self.0;
        tokens.extend(quote! { #[cfg_attr(feature = #feature, derive(defmt::Format))] });
    }
}
