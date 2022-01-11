use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, TokenStreamExt, ToTokens};
use crate::ast_wrappers::{CGTypename};
use crate::rust::docs::CGDocs;
use parser::ast::item_enum::{EnumItems, ItemEnum, EnumItemName, EnumItemKind};
use crate::rust::item_tuple::CGTupleFields;
use std::marker::PhantomData;

pub struct CGItemEnum<'i, 'c> {
    pub docs: CGDocs<'i, 'c>,
    pub typename: CGTypename<'i, 'c>,
    pub items: &'c EnumItems<'i>,
}

impl<'i, 'c> CGItemEnum<'i, 'c> {
    pub fn new(item_enum: &'c ItemEnum<'i>) -> Self {
        Self {
            docs: CGDocs { inner: &item_enum.docs },
            typename: CGTypename { inner: &item_enum.typename },
            items: &item_enum.items
        }
    }
}

impl<'i, 'c> ToTokens for CGItemEnum<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let docs = &self.docs;
        let typename = &self.typename;
        let items = self.items.items.iter().map(
            |i| CGEnumItem {
                docs: CGDocs { inner: &i.docs },
                name: CGEnumItemName { inner: &i.name },
                kind: CGEnumItemKind { inner: &i.kind }
            }
        );
        tokens.append_all(quote! {
            #docs
            #[derive(Copy, Clone, Eq, PartialEq, Debug)]
            enum #typename {
                #(#items),*
            }
        });
    }
}

pub struct CGEnumItem<'i, 'c> {
    pub docs: CGDocs<'i, 'c>,
    pub name: CGEnumItemName<'i, 'c>,
    pub kind: CGEnumItemKind<'i, 'c>
}

impl<'i, 'c> ToTokens for CGEnumItem<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let docs = &self.docs;
        let name = &self.name;
        let kind = &self.kind;
        tokens.append_all(quote! {
            #docs
            #name
            #kind
        });
    }
}

pub struct CGEnumItemName<'i, 'c> {
    pub inner: &'c EnumItemName<'i>
}

impl<'i, 'c> ToTokens for CGEnumItemName<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(self.inner.name, Span::call_site()));
    }
}

pub struct CGEnumItemKind<'i, 'c> {
    pub inner: &'c Option<EnumItemKind<'i>>
}

impl<'i, 'c> ToTokens for CGEnumItemKind<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if let Some(kind) = self.inner {
            match kind {
                EnumItemKind::Tuple(fields) => {
                    let fields = CGTupleFields { inner: &fields, _p: &PhantomData };
                    tokens.append_all(quote! {
                        #fields
                    });
                }
                EnumItemKind::Struct => {
                    todo!()
                }
                EnumItemKind::Discriminant(_expression) => {
                    todo!()
                }
            }
        }
    }
}