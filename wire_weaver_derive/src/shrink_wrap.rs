use proc_macro2::{Span, TokenStream};
use quote::TokenStreamExt;
use syn::{File, Item};
use wire_weaver_core::codegen::item_enum::{enum_def, enum_serdes};
use wire_weaver_core::codegen::item_struct::{struct_def, struct_serdes};
use wire_weaver_core::transform::syn_util::take_owned_attr;
use wire_weaver_core::transform::transform_enum::transform_item_enum;
use wire_weaver_core::transform::transform_struct::transform_item_struct;

// TODO: move owned = "" to derive_shrink_warp attribute macro args?
pub fn shrink_wrap_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let file = match syn::parse2::<File>(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    shrink_wrap_attr_inner(file)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

pub fn shrink_wrap_derive(item: TokenStream) -> TokenStream {
    let file = match syn::parse2::<File>(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    shrink_wrap_derive_inner(file)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

fn shrink_wrap_attr_inner(mut file: File) -> Result<TokenStream, String> {
    let Some(mut item) = file.items.pop() else {
        return Err("Expected one item (enum or struct)".into());
    };
    let attrs = match &mut item {
        Item::Enum(item_enum) => &mut item_enum.attrs,
        Item::Struct(item_struct) => &mut item_struct.attrs,
        _ => return Err("Expected enum or struct".into()),
    };
    let generate_owned = take_owned_attr(attrs)?;

    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let ww_item_enum = transform_item_enum(item_enum)?;
            // TODO: use generics presence as no_alloc indicator as well here?
            let no_alloc = ww_item_enum.potential_lifetimes();
            ts.append_all(enum_def(&ww_item_enum, no_alloc));
            ts.append_all(enum_serdes(&ww_item_enum, no_alloc));
            if let Some(feature) = &generate_owned {
                let enum_owned = ww_item_enum.to_owned(feature.clone());
                ts.append_all(enum_def(&enum_owned, false));
                ts.append_all(enum_serdes(&enum_owned, false));
            }
        }
        Item::Struct(item_struct) => {
            let ww_item_struct = transform_item_struct(item_struct)?;
            let no_alloc = ww_item_struct.potential_lifetimes();
            ts.append_all(struct_def(&ww_item_struct, no_alloc));
            ts.append_all(struct_serdes(&ww_item_struct, no_alloc));
            if let Some(feature) = &generate_owned {
                let struct_owned = ww_item_struct.to_owned(feature.clone());
                ts.append_all(struct_def(&struct_owned, false));
                ts.append_all(struct_serdes(&struct_owned, false));
            }
        }
        _ => {}
    }
    Ok(ts)
}

fn shrink_wrap_derive_inner(mut file: File) -> Result<TokenStream, String> {
    let Some(item) = file.items.pop() else {
        return Err("Expected one item (enum or struct)".into());
    };

    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let ww_item_enum = transform_item_enum(item_enum)?;
            let no_alloc = !item_enum.generics.params.is_empty();
            ts.append_all(enum_serdes(&ww_item_enum, no_alloc));
        }
        Item::Struct(item_struct) => {
            let ww_item_struct = transform_item_struct(item_struct)?;
            let no_alloc = !item_struct.generics.params.is_empty();
            ts.append_all(struct_serdes(&ww_item_struct, no_alloc));
        }
        _ => {}
    }
    Ok(ts)
}
