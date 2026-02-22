use proc_macro2::{Span, TokenStream};
use quote::TokenStreamExt;
use shrink_wrap_core::ast::{ItemEnum, ItemStruct};
use shrink_wrap_core::transform::take_owned_attr;
use syn::{File, Item};

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
    let no_alloc = has_lifetimes(&item);

    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let ww_item_enum = ItemEnum::from_syn(item_enum)?;
            ts.append_all(ww_item_enum.def_rust(no_alloc));
            ts.append_all(ww_item_enum.serdes_rust(no_alloc, false));
            if let Some(feature) = &generate_owned {
                let enum_owned = ww_item_enum.to_owned(feature.clone());
                ts.append_all(enum_owned.def_rust(false));
                ts.append_all(enum_owned.serdes_rust(false, false));
            }
        }
        Item::Struct(item_struct) => {
            let ww_item_struct = ItemStruct::from_syn(item_struct)?;
            ts.append_all(ww_item_struct.def_rust(no_alloc));
            ts.append_all(ww_item_struct.serdes_rust(no_alloc, false));
            if let Some(feature) = &generate_owned {
                let struct_owned = ww_item_struct.to_owned(feature.clone());
                ts.append_all(struct_owned.def_rust(false));
                ts.append_all(struct_owned.serdes_rust(false, false));
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
            let ww_item_enum = ItemEnum::from_syn(item_enum)?;
            let no_alloc = !item_enum.generics.params.is_empty();
            ts.append_all(ww_item_enum.serdes_rust(no_alloc, false));
        }
        Item::Struct(item_struct) => {
            let ww_item_struct = ItemStruct::from_syn(item_struct)?;
            let no_alloc = !item_struct.generics.params.is_empty();
            ts.append_all(ww_item_struct.serdes_rust(no_alloc, false));
        }
        _ => {}
    }
    Ok(ts)
}

fn has_lifetimes(item: &Item) -> bool {
    match item {
        Item::Enum(item_enum) => item_enum.generics.lifetimes().next().is_some(),
        Item::Struct(item_struct) => item_struct.generics.lifetimes().next().is_some(),
        _ => false,
    }
}
