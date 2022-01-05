use parser::ast::item::Docs;
use crate::ast_wrappers::CGTypename;

use proc_macro2::{TokenStream, };
use quote::{quote, TokenStreamExt, ToTokens};
use parser::ast::item_enum::{ItemEnum, EnumItems, EnumItemName};

pub struct CGDocs<'i, 'c> {
    pub inner: &'c Docs<'i>
}

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

pub struct CGEnumItem<'i, 'c> {
    pub docs: CGDocs<'i, 'c>,
    pub name: CGEnumItemName<'i, 'c>,
    // pub kind:
}

pub struct CGEnumItemName<'i, 'c> {
    pub inner: &'c EnumItemName<'i>
}