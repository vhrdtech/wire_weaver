use proc_macro2::{Ident, Span, TokenStream, TokenTree};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{ItemEnum, ItemStruct, Meta};

pub fn shrink_wrap(item: proc_macro::TokenStream) -> TokenStream {
    let item: TokenStream = item.into();
    // eprintln!("{item:?}");
    let Some(TokenTree::Ident(kind)) = item
        .clone()
        .into_iter()
        .skip_while(|tt| !matches!(tt, TokenTree::Ident(_)))
        .next()
    else {
        panic!("struct or enum expected");
    };
    let kind = kind.to_string();
    let mut ts = TokenStream::new();
    if kind == "struct" {
        let item_struct: ItemStruct = syn::parse2(item).unwrap();
        // eprintln!("{item_struct:?}");
        let (item_struct, warnings) =
            wire_weaver_core::ast::item::ItemStruct::from_syn(item_struct).unwrap();
        if !warnings.is_empty() {
            eprintln!("{warnings:?}");
        }
        ts.append_all(wire_weaver_core::codegen::item::struct_serdes(
            &item_struct,
            true,
        ));
    } else if kind == "enum" {
        let item_enum: ItemEnum = syn::parse2(item).unwrap();
        let Some(repr_attr) = item_enum
            .attrs
            .iter()
            .find(|a| a.path().get_ident() == Some(&Ident::new("repr", Span::call_site())))
        else {
            panic!("enum must be repr(u16)");
        };
        let Meta::List(ref repr_attr) = repr_attr.meta else {
            panic!("enum must be repr(u16)")
        };
        if repr_attr.tokens.to_string() != "u16" {
            panic!("enum must be repr(u16)");
        }

        let (item_enum, warnings) =
            wire_weaver_core::ast::item::ItemEnum::from_syn(item_enum).unwrap();
        if !warnings.is_empty() {
            eprintln!("{warnings:?}");
        }
        ts.append_all(wire_weaver_core::codegen::item::enum_discriminant(
            &item_enum,
        ));
        ts.append_all(wire_weaver_core::codegen::item::enum_serdes(
            &item_enum, true,
        ));
    } else {
        panic!("only structs and enums are supported");
    }
    ts
}
