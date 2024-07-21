use std::str::FromStr;

use syn::{Expr, Lit, Meta};

use crate::ast::item::Repr;
use crate::ast::value::Value;
use crate::Version;

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
    errors: &mut Vec<SynConversionError>,
) -> Option<Value> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("default"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        errors.push(SynConversionError::WrongDefaultAttr(
            "Expected default = lit".into(),
        ));
        return None;
    };
    let Expr::Lit(expr_lit) = name_value.value else {
        errors.push(SynConversionError::WrongDefaultAttr(
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
            errors.push(SynConversionError::WrongDefaultAttr(format!(
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

pub(crate) fn take_final_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("finalx"))?;
    let _attr = attrs.remove(attr_idx);
    Some(())
}

pub(crate) fn take_repr_attr(
    attrs: &mut Vec<syn::Attribute>,
    errors: &mut Vec<SynConversionError>,
) -> Option<Repr> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("repr"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::List(meta_list) = attr.meta else {
        errors.push(SynConversionError::WrongReprAttr(
            "expected #[repr(u4/u8/u16/u32/vlu16n)]".into(),
        ));
        return None;
    };
    let repr = meta_list.tokens.to_string();
    let Ok(repr) = Repr::from_str(repr.as_str()) else {
        errors.push(SynConversionError::WrongReprAttr(
            "expected #[repr(u4/u8/u16/u32/vlu16n)]".into(),
        ));
        return None;
    };
    Some(repr)
}

pub(crate) fn collect_unknown_attributes(
    attrs: &mut Vec<syn::Attribute>,
    warnings: &mut Vec<SynConversionWarning>,
) {
    for a in attrs {
        warnings.push(SynConversionWarning::UnknownAttribute(format!(
            "{:?}",
            a.meta.path()
        )));
    }
}

#[derive(Debug)]
pub enum SynConversionWarning {
    UnknownAttribute(String),
    UnknownFileItem,
}

#[derive(Debug)]
pub enum SynConversionError {
    UnknownType,
    WrongDefaultAttr(String),
    WrongDiscriminant,
    WrongReprAttr(String),
}
