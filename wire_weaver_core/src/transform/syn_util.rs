use crate::ast::path::Path;
use crate::ast::value::Value;
use crate::ast::{Docs, Repr, Version};
use proc_macro2::TokenStream;
use quote::quote;
use shrink_wrap::ElementSize;
use syn::{Expr, Lit, LitStr, Meta};

/// Take `#[id = integer]` attribute and return the number
pub(crate) fn take_id_attr(_attrs: &mut Vec<syn::Attribute>) -> Option<u32> {
    None
}

/// Take `#[since = "X.Y.Z"]` attribute and return the Version
pub(crate) fn take_since_attr(_attrs: &mut Vec<syn::Attribute>) -> Option<Version> {
    None
}

/// Take `#[default = lit]` attribute and return Value containing provided literal
pub(crate) fn take_default_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<Value>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("default"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        return Err("Expected default = lit".into());
    };
    match name_value.value {
        // Expr::Array(_) => {}
        // Expr::Call(_) => {}
        Expr::Lit(expr_lit) => {
            match expr_lit.lit {
                Lit::Float(lit_float) => {
                    // TODO: Handle f32 and f64 properly
                    Ok(Some(Value::F32(lit_float.base10_parse().unwrap())))
                }
                Lit::Int(lit_int) => {
                    // lit_int.suffix()
                    // TODO: create default integer and do not emit suffix on it, emit suffix if it is present
                    Ok(Some(Value::I32(lit_int.base10_parse().unwrap())))
                }
                u => Err(format!("Not supported lit: {u:?}")), // Lit::Str(_) => {}
                                                               // Lit::ByteStr(_) => {}
                                                               // Lit::CStr(_) => {}
                                                               // Lit::Byte(_) => {}
                                                               // Lit::Char(_) => {}
                                                               // Lit::Int(_) => {}
                                                               // Lit::Bool(_) => {}
                                                               // Lit::Verbatim(_) => {}
            }
        }
        Expr::Path(expr_path) => {
            if expr_path.path.is_ident("None") {
                Ok(Some(Value::None))
            } else {
                Err(format!("Not supported path: {expr_path:?}"))
            }
        }
        // Expr::Range(_) => {}
        // Expr::Repeat(_) => {}
        // Expr::Struct(_) => {}
        // Expr::Tuple(_) => {}
        // Expr::Verbatim(_) => {}
        _ => Err("Expected default = lit/path/struct".into()),
    }
}

pub(crate) fn take_flag_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("flag"))?;
    let _attr = attrs.remove(attr_idx);
    Some(())
}

// pub(crate) fn take_final_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
//     let (attr_idx, _) = attrs
//         .iter()
//         .enumerate()
//         .find(|(_, a)| a.path().is_ident("final_evolution"))?;
//     let _attr = attrs.remove(attr_idx);
//     Some(())
// }

pub(crate) fn take_size_assumption(attrs: &mut Vec<syn::Attribute>) -> Option<ElementSize> {
    let mut is_final_structure = false;
    let mut is_self_describing = false;
    let mut is_sized = false;
    let attr_idx = attrs.iter().enumerate().find(|(_, a)| {
        if a.path().is_ident("final_structure") {
            is_final_structure = true;
            true
        } else if a.path().is_ident("self_describing") {
            is_self_describing = true;
            true
        } else if a.path().is_ident("sized") {
            is_sized = true;
            true
        } else {
            false
        }
    });
    if let Some((attr_idx, _)) = attr_idx {
        let _attr = attrs.remove(attr_idx);
        if is_final_structure {
            Some(ElementSize::UnsizedFinalStructure)
        } else if is_self_describing {
            Some(ElementSize::SelfDescribing)
        } else if is_sized {
            Some(ElementSize::Sized { size_bits: 0 })
        } else {
            None
        }
    } else {
        None
    }
}

pub(crate) fn collect_docs_attrs(attrs: &mut Vec<syn::Attribute>) -> Docs {
    let mut docs = Docs::empty();
    for attr in attrs.iter() {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(name_value) = attr.meta.clone() else {
            continue;
        };
        let Expr::Lit(expr_lit) = name_value.value else {
            continue;
        };
        if let Lit::Str(lit_str) = expr_lit.lit {
            docs.push(lit_str);
        }
    }
    attrs.retain(|a| !a.path().is_ident("doc"));
    docs
}

pub fn take_ww_repr_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Repr, String> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("ww_repr"))
        .ok_or("ww_repr attribute is required")?;
    let attr = attrs.remove(attr_idx);
    let Meta::List(meta_list) = attr.meta else {
        return Err("expected #[repr(u1..u32 / nib16)]".into());
    };
    let repr = meta_list.tokens.to_string();
    let Some(repr) = Repr::parse_str(repr.as_str()) else {
        return Err("expected #[repr(u1..u32 / nib16)]".into());
    };
    Ok(repr)
}

pub fn take_shrink_wrap_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<String>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("shrink_wrap"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::List(meta_list) = attr.meta else {
        return Err("expected #[shrink_wrap(no_alloc)]".into());
    };
    let config = meta_list.tokens.to_string();
    Ok(Some(config))
}

pub fn take_owned_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<LitStr>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("owned"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(named_value) = attr.meta else {
        return Err("expected #[owned = \"feature_name\"]".into());
    };
    let Expr::Lit(expr_lit) = named_value.value else {
        return Err("expected #[owned = \"feature_name\"]".into());
    };
    let Lit::Str(feature) = expr_lit.lit else {
        return Err("expected #[owned = \"feature_name\"]".into());
    };
    Ok(Some(feature))
}

pub(crate) fn take_derive_attr(attrs: &mut Vec<syn::Attribute>) -> Vec<Path> {
    let mut derive = vec![];
    for attr in attrs.iter() {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let Meta::List(meta_list) = attr.meta.clone() else {
            continue;
        };
        let derives = meta_list.tokens.to_string();
        derive.extend(
            derives
                .split(&[' ', ','])
                .filter(|s| !s.is_empty())
                .map(|s| Path::new_path(s)),
        );
    }
    attrs.retain(|a| !a.path().is_ident("derive"));
    derive
}

pub(crate) fn collect_unknown_attributes(attrs: &mut Vec<syn::Attribute>) {
    for a in attrs {
        // ignore #[shrink_warp(...)] in after #[derive(ShrinkWrap)]
        if a.path().is_ident("shrink_wrap") {
            continue;
        }
        println!("Unknown attribute: {:?}", a.meta.path());
    }
}

#[must_use]
pub(crate) fn trace_extended_key_val(_key: &str, _val: &str) -> TokenStream {
    // let msg = format!("{key} {val}");
    // let msg = LitStr::new(msg.as_str(), proc_macro2::Span::call_site());
    // quote! {
    //     #[cfg(feature = "tracing-extended")]
    //     tracing::trace!(#msg);
    //     #[cfg(feature = "defmt-extended")]
    //     defmt::trace!(#msg);
    // }
    quote! {}
}
