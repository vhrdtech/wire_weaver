use convert_case::Casing;
use proc_macro2::TokenStream;
use quote::TokenStreamExt;

use crate::ast::api::{ApiItemKind, ApiLevel};
use crate::ast::ident::Ident;
use crate::ast::{Field, ItemStruct};

pub fn args_structs(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let mut defs = TokenStream::new();
    for item in &api_level.items {
        if let ApiItemKind::Method { ident, args } = &item.kind {
            let mut fields = vec![];
            for (id, arg) in args.iter().enumerate() {
                fields.push(Field {
                    docs: vec![],
                    id: id as u32,
                    ident: arg.ident.clone(),
                    ty: arg.ty.clone(),
                    since: None,
                    default: None,
                });
            }
            if fields.is_empty() {
                continue;
            }

            let ident = format!("{}_args", ident.sym).to_case(convert_case::Case::Pascal);
            let item_struct = ItemStruct {
                docs: vec![],
                derive: vec![],
                is_final: false,
                ident: Ident::new(ident),
                fields,
            };
            defs.append_all(super::item_struct::struct_def(&item_struct, no_alloc));
            defs.append_all(super::item_struct::struct_serdes(&item_struct, no_alloc));
        }
    }
    defs
}
