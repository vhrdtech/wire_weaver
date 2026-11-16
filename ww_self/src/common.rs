//! Messages of error values in dynamic values: the doc lines of enum variants, filled in from the value.

use crate::{ApiBundleOwned, FieldsValueOwned, TypeOwned, ValueOwned};
use ww_numeric::NumericValue;

impl ValueOwned {
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
    n.to_i128()
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
