//! `ww_error::CommonError` (STD-16) in dynamic values: recognised by its type, so that every tool shows it the same
//! for every API, and messages from the doc lines of enum variants.

use crate::{
    ApiBundleOwned, FieldsOwned, FieldsValueOwned, TypeLocationOwned, TypeOwned, ValueOwned,
};
use ww_numeric::NumericValue;

/// Crate and name of the type every API's common errors have.
pub const COMMON_ERROR: (&str, &str) = ("ww_error", "CommonError");

impl TypeOwned {
    /// Whether this is `ww_error::CommonError`, of any version, also when its definition was left out
    /// (known from snapshots).
    pub fn is_common_error(&self, bundle: &ApiBundleOwned) -> bool {
        let TypeOwned::OutOfLine { type_idx } = self else {
            return false;
        };
        let (crate_idx, name) = match bundle.types.get(type_idx.0 as usize) {
            Some(TypeLocationOwned::InLine {
                ty: TypeOwned::Enum(item_enum),
                crate_idx,
            }) => (crate_idx, item_enum.ident.as_str()),
            Some(TypeLocationOwned::SkippedFullVersion {
                crate_idx,
                type_name,
                ..
            }) => (crate_idx, type_name.as_str()),
            _ => return false,
        };
        bundle.crate_name(crate_idx.0).ok() == Some(COMMON_ERROR.0) && name == COMMON_ERROR.1
    }
}

impl ValueOwned {
    /// The `CommonError` in an error value of type `ty`, with its type: the value itself when `ty` is
    /// `CommonError`, the field of its `#[common] Common(CommonError)` variant when it is an API's error enum holding
    /// one, `None` otherwise.
    pub fn common_error<'v>(
        &'v self,
        ty: &'v TypeOwned,
        bundle: &'v ApiBundleOwned,
    ) -> Option<(&'v ValueOwned, &'v TypeOwned)> {
        if ty.is_common_error(bundle) {
            return Some((self, ty));
        }
        let (
            TypeOwned::Enum(item_enum),
            ValueOwned::Enum {
                variant,
                fields: FieldsValueOwned::Unnamed(values),
            },
        ) = (ty.get_in_line(bundle).ok()?, self)
        else {
            return None;
        };
        let def = item_enum.variants.iter().find(|v| &v.ident == variant)?;
        match (&def.fields, values.as_slice()) {
            (FieldsOwned::Unnamed(fields), [value])
                if fields.len() == 1 && fields[0].ty.is_common_error(bundle) =>
            {
                Some((value, &fields[0].ty))
            }
            _ => None,
        }
    }

    /// The first doc line of the variant of an enum value of type `ty`, without the final period, with `{field}`
    /// (and `{field:#x}` for integers) filled in from the value: `In use by USART2` for
    /// `/// In use by {by}.`. A placeholder whose field is an empty string is dropped together with the `: ` or
    /// ` on ` before it (`Refused` for `/// Refused: {reason}.`). `None` for other values and undocumented variants.
    pub fn doc_message(&self, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<String> {
        let (TypeOwned::Enum(item_enum), ValueOwned::Enum { variant, fields }) =
            (ty.get_in_line(bundle).ok()?, self)
        else {
            return None;
        };
        let def = item_enum.variants.iter().find(|v| &v.ident == variant)?;
        let line = def.docs.first()?.trim();
        let line = line.strip_suffix('.').unwrap_or(line);
        let named = match fields {
            FieldsValueOwned::Named(named) => named.as_slice(),
            _ => &[],
        };
        Some(fill(line, named))
    }
}

/// `{name}` and `{name:#x}` in `template` replaced with the value of field `name`, unknown names kept as written.
fn fill(template: &str, fields: &[(String, ValueOwned)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        let placeholder = &rest[start + 1..start + len];
        let (name, hex) = match placeholder.strip_suffix(":#x") {
            Some(name) => (name, true),
            None => (placeholder, false),
        };
        let value = fields.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        let before = &rest[..start];
        match value.map(|v| format_value(v, hex)) {
            Some(text) if text.is_empty() => {
                let before = before
                    .strip_suffix(": ")
                    .or_else(|| before.strip_suffix(" on "))
                    .or_else(|| before.strip_suffix(" by "))
                    .unwrap_or(before);
                out.push_str(before);
            }
            Some(text) => {
                out.push_str(before);
                out.push_str(&text);
            }
            None => out.push_str(&rest[..start + len + 1]),
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn format_value(value: &ValueOwned, hex: bool) -> String {
    match value {
        ValueOwned::String(s) => s.clone(),
        ValueOwned::Bool(b) => b.to_string(),
        ValueOwned::Numeric(n) => match (integer(n), hex) {
            (Some(i), true) => format!("{i:#x}"),
            (Some(i), false) => i.to_string(),
            (None, _) => format!("{n:?}"),
        },
        other => format!("{other:?}"),
    }
}

fn integer(n: &NumericValue) -> Option<i128> {
    Some(match *n {
        NumericValue::U8(v) => v.into(),
        NumericValue::U16(v) => v.into(),
        NumericValue::U32(v) | NumericValue::UNib32(v) => v.into(),
        NumericValue::U64(v) => v.into(),
        NumericValue::I8(v) => v.into(),
        NumericValue::I16(v) => v.into(),
        NumericValue::I32(v) => v.into(),
        NumericValue::I64(v) => v.into(),
        NumericValue::I128(v) => v,
        NumericValue::U128(v) => i128::try_from(v).ok()?,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> ValueOwned {
        ValueOwned::String(v.into())
    }

    #[test]
    fn fills_placeholders() {
        let fields = vec![
            ("by".to_string(), s("USART2")),
            ("code".into(), ValueOwned::Numeric(NumericValue::U32(42))),
        ];
        assert_eq!(fill("In use by {by}", &fields), "In use by USART2");
        assert_eq!(
            fill("Hardware fault {code:#x}", &fields),
            "Hardware fault 0x2a"
        );
        assert_eq!(
            fill("Fault {code} of {missing}", &fields),
            "Fault 42 of {missing}"
        );
        let empty = vec![("reason".to_string(), s("")), ("rail".into(), s(""))];
        assert_eq!(fill("Refused: {reason}", &empty), "Refused");
        assert_eq!(fill("No power on {rail}", &empty), "No power");
    }
}
