use syn::parse::{Parse, ParseBuffer};
use syn::LitStr;
use proc_macro2::Span;
use syn::Result;

use crate::ast::ResourceName;

impl Parse for ResourceName {
    fn parse(input: &ParseBuffer) -> Result<Self> {
        Ok(ResourceName::Plain(LitStr::new("abcd", Span::call_site())))
    }
}