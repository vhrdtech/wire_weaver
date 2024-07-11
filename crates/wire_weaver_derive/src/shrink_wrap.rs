use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, TokenStreamExt};
use syn::ItemStruct;

pub fn shrink_wrap(item: proc_macro::TokenStream) -> TokenStream {
    let item: TokenStream = item.into();
    // eprintln!("{item:?}");
    let Some(TokenTree::Ident(kind)) = item.clone().into_iter().next() else {
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
    } else {
        panic!("only structs and enums are supported");
    }
    ts
}
