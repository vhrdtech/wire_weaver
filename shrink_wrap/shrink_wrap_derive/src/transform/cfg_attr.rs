//! `#[cfg_attr(pred, attr)]` on fields and enum variants of a `derive_shrink_wrap` item, where `attr` is one the
//! derive reads (`default = ..`, `flag`, `since = ..`, ...).
//!
//! An attribute macro gets its item before rustc expands the `cfg_attr`s inside it, and can't evaluate a `cfg`
//! itself. So the derive reads such an attribute as if `pred` holds, and the generated code fails to compile where
//! it doesn't: the bytes never depend on a `cfg` the derive couldn't see. This is the form a crate with
//! `shrink_wrap` optional needs (`exact_units`): `#[cfg_attr(feature = "shrink_wrap", derive_shrink_wrap(..))]` on
//! the type and `#[cfg_attr(feature = "shrink_wrap", default = 0)]` on a field, the same feature, so it always holds
//! where the derive runs. `wire(W)` goes the same way, read by [super::syn_util::take_wire_attr] (DER-19).

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::punctuated::Punctuated;
use syn::{Attribute, Fields, Item, Meta, Token};

/// Attributes the derive reads on a field or an enum variant (except `wire`, see the module docs).
const READ: &[&str] = &[
    "default",
    "flag",
    "id",
    "series",
    "slot",
    "since",
    "common",
    "display",
    "future_version",
];

/// Expands the `cfg_attr`s on the fields and variants of `item` that hold an attribute the derive reads, and
/// returns their predicates, each once.
pub(crate) fn expand_read_cfg_attrs(item: &mut Item) -> Result<Vec<Meta>, String> {
    let mut preds = vec![];
    match item {
        Item::Struct(item_struct) => fields(&mut item_struct.fields, &mut preds)?,
        Item::Enum(item_enum) => {
            for variant in &mut item_enum.variants {
                attrs(&mut variant.attrs, &mut preds)?;
                fields(&mut variant.fields, &mut preds)?;
            }
        }
        _ => {}
    }
    Ok(preds)
}

/// A compile error per predicate of [expand_read_cfg_attrs], where it doesn't hold.
pub(crate) fn cfg_guards(type_name: &str, preds: &[Meta]) -> TokenStream {
    let mut ts = TokenStream::new();
    for pred in preds {
        let message = format!(
            "{type_name}: #[cfg_attr({}, ..)] on a field or variant is off while the type is serialized, its bytes \
             would change with a cfg. Put the derive under the same cfg, or drop the cfg_attr",
            pred.to_token_stream()
        );
        ts.append_all(quote! {
            #[cfg(not(#pred))]
            compile_error!(#message);
        });
    }
    ts
}

fn fields(fields: &mut Fields, preds: &mut Vec<Meta>) -> Result<(), String> {
    for field in fields.iter_mut() {
        attrs(&mut field.attrs, preds)?;
    }
    Ok(())
}

fn attrs(attrs: &mut Vec<Attribute>, preds: &mut Vec<Meta>) -> Result<(), String> {
    let mut expanded = Vec::with_capacity(attrs.len());
    for attr in attrs.drain(..) {
        let Some((pred, inner)) = read_cfg_attr(&attr)? else {
            expanded.push(attr);
            continue;
        };
        if !preds.contains(&pred) {
            preds.push(pred);
        }
        expanded.extend(inner.into_iter().map(|meta| Attribute {
            pound_token: attr.pound_token,
            style: attr.style,
            bracket_token: attr.bracket_token,
            meta,
        }));
    }
    *attrs = expanded;
    Ok(())
}

/// `pred` and the attributes of a `#[cfg_attr(pred, ..)]` holding attributes the derive reads, `None` for any other
/// attribute.
fn read_cfg_attr(attr: &Attribute) -> Result<Option<(Meta, Vec<Meta>)>, String> {
    if !attr.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Ok(metas) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return Ok(None);
    };
    let mut metas = metas.into_iter();
    let Some(pred) = metas.next() else {
        return Ok(None);
    };
    let inner: Vec<Meta> = metas.collect();
    let is_read = |meta: &Meta| READ.iter().any(|name| meta.path().is_ident(name));
    if !inner.iter().any(is_read) {
        return Ok(None);
    }
    if !inner.iter().all(is_read) {
        return Err(format!(
            "#[cfg_attr({}, ..)]: the derive's own attributes ({}) go in a cfg_attr of their own",
            pred.to_token_stream(),
            READ.join(", ")
        ));
    }
    Ok(Some((pred, inner)))
}
