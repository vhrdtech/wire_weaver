//! Short, Rust-like text of a dynamically decoded value, for const values in `ww introspect`.

use wire_weaver_client::ww_self::{FieldsValueOwned, ValueOwned};

/// `{ name: "b200", version: { major: 0, .. }, commit: 0xa1b2c3 }`: struct and enum names are not in the value,
/// byte vectors and arrays are shown as hex.
pub(crate) fn fmt_value(value: &ValueOwned) -> String {
    match value {
        ValueOwned::Bool(b) => b.to_string(),
        ValueOwned::Numeric(n) => match n.to_i128() {
            Some(i) => i.to_string(),
            None => n.to_f64().to_string(),
        },
        ValueOwned::String(s) => format!("{s:?}"),
        ValueOwned::Vec(items) | ValueOwned::Array(items) => match bytes(items) {
            Some(bytes) if !bytes.is_empty() => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                format!("0x{hex}")
            }
            _ => format!("[{}]", list(items)),
        },
        ValueOwned::Tuple(items) => format!("({})", list(items)),
        ValueOwned::Struct { fields } => fields_text(fields),
        ValueOwned::Enum { variant, fields } => match fields {
            FieldsValueOwned::Unit => variant.clone(),
            fields => format!("{variant}{}", fields_text(fields)),
        },
        ValueOwned::Option(None) => "None".into(),
        ValueOwned::Option(Some(v)) => format!("Some({})", fmt_value(v)),
        ValueOwned::Result(Ok(v)) => format!("Ok({})", fmt_value(v)),
        ValueOwned::Result(Err(v)) => format!("Err({})", fmt_value(v)),
        ValueOwned::Range(r) => format!(
            "{}..{}",
            fmt_value(&ValueOwned::Numeric(r.start)),
            fmt_value(&ValueOwned::Numeric(r.end))
        ),
        ValueOwned::RangeInclusive(r) => format!(
            "{}..={}",
            fmt_value(&ValueOwned::Numeric(*r.start())),
            fmt_value(&ValueOwned::Numeric(*r.end()))
        ),
    }
}

fn list(items: &[ValueOwned]) -> String {
    items.iter().map(fmt_value).collect::<Vec<_>>().join(", ")
}

fn fields_text(fields: &FieldsValueOwned) -> String {
    match fields {
        FieldsValueOwned::Named(named) => {
            let fields: Vec<String> = named
                .iter()
                .map(|(name, v)| format!("{name}: {}", fmt_value(v)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        FieldsValueOwned::Unnamed(items) => format!("({})", list(items)),
        FieldsValueOwned::Unit => String::new(),
    }
}

/// The items as bytes, if all of them are `u8`.
fn bytes(items: &[ValueOwned]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| match v {
            ValueOwned::Numeric(ww_numeric::NumericValue::U8(b)) => Some(*b),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_numeric::NumericValue;

    #[test]
    fn formats_structs_enums_and_bytes() {
        let value = ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![
                ("name".into(), ValueOwned::String("b200".into())),
                (
                    "id".into(),
                    ValueOwned::Vec(vec![
                        ValueOwned::Numeric(NumericValue::U8(0xa1)),
                        ValueOwned::Numeric(NumericValue::U8(0x02)),
                    ]),
                ),
                (
                    "kind".into(),
                    ValueOwned::Enum {
                        variant: "Chip".into(),
                        fields: FieldsValueOwned::Unit,
                    },
                ),
                ("rev".into(), ValueOwned::Option(None)),
            ]),
        };
        assert_eq!(
            fmt_value(&value),
            r#"{ name: "b200", id: 0xa102, kind: Chip, rev: None }"#
        );
    }
}
