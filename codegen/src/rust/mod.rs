pub mod ast_wrappers;

use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, TokenStreamExt, ToTokens};
use crate::ast_wrappers::{CGTypename};
use crate::rust::ast_wrappers::{CGDocs, CGItemEnum, CGEnumItem, CGEnumItemName};

impl<'i, 'c> ToTokens for CGDocs<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let lines = &self.inner.lines;
        tokens.append_all(quote! {
            #(#[doc = #lines])*
        });
    }
}

impl<'i, 'c> ToTokens for CGEnumItemName<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(self.inner.name, Span::call_site()));
    }
}


impl<'i, 'c> ToTokens for CGEnumItem<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let docs = &self.docs;
        let name = &self.name;
        tokens.append_all(quote! {
            #docs
            #name
        });
    }
}

impl<'i, 'c> ToTokens for CGItemEnum<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let docs = &self.docs;
        let typename = &self.typename;
        let items = self.items.items.iter().map(
            |i| CGEnumItem {
                docs: CGDocs { inner: &i.docs },
                name: CGEnumItemName { inner: &i.name }
            }
        );
        tokens.append_all(quote::quote! {
            #docs
            #[derive(Copy, Clone, Eq, PartialEq, Debug)]
            struct #typename {
                #(#items),*
            }
        });
    }
}