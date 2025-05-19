use syn::{Expr, Lit, Meta};

use crate::ast::path::Path;
use crate::ast::value::Value;
use crate::ast::{Repr, Version};
use crate::transform::{Messages, SynConversionError, SynConversionWarning};

/// Take `#[id = integer]` attribute and return the number
pub(crate) fn take_id_attr(_attrs: &mut Vec<syn::Attribute>) -> Option<u32> {
    None
}

/// Take `#[since = "X.Y.Z"]` attribute and return the Version
pub(crate) fn take_since_attr(_attrs: &mut Vec<syn::Attribute>) -> Option<Version> {
    None
}

/// Take `#[default = lit]` attribute and return Value containing provided literal
pub(crate) fn take_default_attr(
    attrs: &mut Vec<syn::Attribute>,
    messages: &mut Messages,
) -> Option<Value> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("default"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        messages.push_conversion_error(SynConversionError::WrongDefaultAttr(
            "Expected default = lit".into(),
        ));
        return None;
    };
    let Expr::Lit(expr_lit) = name_value.value else {
        messages.push_conversion_error(SynConversionError::WrongDefaultAttr(
            "Expected default = lit".into(),
        ));
        return None;
    };
    match expr_lit.lit {
        Lit::Float(lit_float) => {
            // TODO: Handle f32 and f64 properly
            Some(Value::F32(lit_float.base10_parse().unwrap()))
        }
        u => {
            messages.push_conversion_error(SynConversionError::WrongDefaultAttr(format!(
                "Not supported lit: {u:?}"
            )));
            None
        } // Lit::Str(_) => {}
          // Lit::ByteStr(_) => {}
          // Lit::CStr(_) => {}
          // Lit::Byte(_) => {}
          // Lit::Char(_) => {}
          // Lit::Int(_) => {}
          // Lit::Bool(_) => {}
          // Lit::Verbatim(_) => {}
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

pub(crate) fn take_final_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("final_evolution"))?;
    let _attr = attrs.remove(attr_idx);
    Some(())
}

pub(crate) fn collect_docs_attrs(attrs: &mut Vec<syn::Attribute>) -> Vec<String> {
    let mut docs = vec![];
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
            docs.push(lit_str.value());
        }
    }
    attrs.retain(|a| !a.path().is_ident("doc"));
    docs
}

pub fn take_ww_repr_attr(attrs: &mut Vec<syn::Attribute>, messages: &mut Messages) -> Option<Repr> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("ww_repr"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::List(meta_list) = attr.meta else {
        messages.push_conversion_error(SynConversionError::WrongReprAttr(
            "expected #[repr(u1..u32 / nib16)]".into(),
        ));
        return None;
    };
    let repr = meta_list.tokens.to_string();
    let Some(repr) = Repr::parse_str(repr.as_str()) else {
        messages.push_conversion_error(SynConversionError::WrongReprAttr(
            "expected #[repr(u1..u32 / nib16)]".into(),
        ));
        return None;
    };
    Some(repr)
}

pub fn take_shrink_wrap_attr(
    attrs: &mut Vec<syn::Attribute>,
    messages: &mut Messages,
) -> Option<String> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("shrink_wrap"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::List(meta_list) = attr.meta else {
        messages.push_conversion_error(SynConversionError::WrongReprAttr(
            "expected #[shrink_wrap(no_alloc)]".into(),
        ));
        return None;
    };
    let config = meta_list.tokens.to_string();
    Some(config)
}

pub(crate) fn take_derive_attr(
    attrs: &mut Vec<syn::Attribute>,
    _messages: &mut Messages,
) -> Vec<Path> {
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

pub(crate) fn collect_unknown_attributes(attrs: &mut Vec<syn::Attribute>, messages: &mut Messages) {
    for a in attrs {
        // ignore #[shrink_warp(...)] in after #[derive(ShrinkWrap)]
        if a.path().is_ident("shrink_wrap") {
            continue;
        }
        messages.push_conversion_warning(SynConversionWarning::UnknownAttribute(format!(
            "{:?}",
            a.meta.path()
        )));
    }
}
