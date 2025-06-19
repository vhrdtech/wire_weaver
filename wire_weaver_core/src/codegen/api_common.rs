use convert_case::Casing;
use proc_macro2::{Ident, Span, TokenStream};
use quote::TokenStreamExt;

use crate::ast::api::{ApiItemKind, ApiLevel};
use crate::ast::{Docs, Field, ItemStruct, Type};
use crate::transform::create_flags;

pub fn args_structs(api_level: &ApiLevel, no_alloc: bool) -> TokenStream {
    let mut defs = TokenStream::new();
    for item in &api_level.items {
        if let ApiItemKind::Method {
            ident,
            args,
            return_type,
        } = &item.kind
        {
            if let Some(ty) = return_type {
                output_struct(&mut defs, ident, ty, no_alloc);
            }

            let mut fields = vec![];
            for (id, arg) in args.iter().enumerate() {
                fields.push(Field {
                    docs: Docs::empty(),
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
            create_flags(&mut fields, &[]);

            let ident = Ident::new(
                format!("{}_args", ident)
                    .to_case(convert_case::Case::Pascal)
                    .as_str(),
                ident.span(),
            );
            let item_struct = ItemStruct {
                docs: Docs::empty(),
                derive: vec![],
                ident,
                fields,
                cfg: None,
                size_assumption: None,
            };
            defs.append_all(super::item_struct::struct_def(&item_struct, no_alloc));
            defs.append_all(super::item_struct::struct_serdes(&item_struct, no_alloc));
        }
    }
    defs
}

fn output_struct(defs: &mut TokenStream, method_ident: &Ident, return_type: &Type, no_alloc: bool) {
    if matches!(return_type, Type::External(_, _)) {
        return;
    }
    let ident = Ident::new(
        format!("{}_output", method_ident)
            .to_case(convert_case::Case::Pascal)
            .as_str(),
        method_ident.span(),
    );
    let mut item_struct = ItemStruct {
        docs: Docs::empty(),
        derive: vec![],
        ident,
        fields: vec![Field {
            docs: Docs::empty(),
            id: 0,
            ident: Ident::new("output", Span::call_site()),
            ty: return_type.clone(),
            since: None,
            default: None,
        }],
        cfg: None,
        size_assumption: None,
    };
    create_flags(&mut item_struct.fields, &[]);
    defs.append_all(super::item_struct::struct_def(&item_struct, no_alloc));
    defs.append_all(super::item_struct::struct_serdes(&item_struct, no_alloc));
}
