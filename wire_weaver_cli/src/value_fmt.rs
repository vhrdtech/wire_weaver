//! Short, Rust-like text of a dynamically decoded value, for const values in `ww introspect` and updates in
//! `ww observe`. With the type known it is a Rust expression `ww_self::expr` reads back (quantities aside).

use wire_weaver_client::quantity::DynQuantity;
use wire_weaver_client::ww_self::{
    ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned,
};

/// `{ name: "b200", version: { major: 0, .. }, commit: 0xa1b2c3 }`: struct and enum names are not in the value,
/// byte vectors and arrays are shown as hex, `ww_si` quantities with their unit (`3300 mV`, CLT-31). `ty` is the
/// value's type in `bundle` (`None`: not known); where the value doesn't match it, the rest is written without types.
pub(crate) fn fmt_value(
    value: &ValueOwned,
    ty: Option<&TypeOwned>,
    bundle: &ApiBundleOwned,
) -> String {
    Fmt { bundle }.value(value, ty)
}

struct Fmt<'a> {
    bundle: &'a ApiBundleOwned,
}

impl Fmt<'_> {
    fn value(&self, value: &ValueOwned, ty: Option<&TypeOwned>) -> String {
        let ty = ty.and_then(|ty| ty.get_in_line(self.bundle).ok());
        if let Some(ty) = ty
            && let Some(q) = DynQuantity::from_value(value, ty, self.bundle)
        {
            return q.to_string();
        }
        if let Some(TypeOwned::MaxLen { ty: inner, .. } | TypeOwned::Box(inner)) = ty {
            return self.value(value, Some(inner));
        }
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
                _ => {
                    let inner = match ty {
                        Some(
                            TypeOwned::Vec(inner)
                            | TypeOwned::Series(inner)
                            | TypeOwned::Array { ty: inner, .. },
                        ) => Some(&**inner),
                        _ => None,
                    };
                    format!("[{}]", self.list(items, |_| inner))
                }
            },
            ValueOwned::Tuple(items) => {
                let types = match ty {
                    Some(TypeOwned::Tuple(types)) => Some(types),
                    _ => None,
                };
                format!(
                    "({})",
                    self.list(items, |idx| types.and_then(|t| t.get(idx)))
                )
            }
            // with its name when known, so the text reads back as a Rust expression (`ww explain --value`)
            ValueOwned::Struct { fields } => match ty {
                Some(TypeOwned::Struct(s)) => {
                    let fields = self.fields(fields, Some(&s.fields));
                    match fields.starts_with('{') {
                        true => format!("{} {fields}", s.ident),
                        false => format!("{}{fields}", s.ident),
                    }
                }
                _ => self.fields(fields, None),
            },
            ValueOwned::Enum { variant, fields } => match fields {
                FieldsValueOwned::Unit => variant.clone(),
                fields => {
                    let defs = match ty {
                        Some(TypeOwned::Enum(e)) => e
                            .variants
                            .iter()
                            .find(|v| &v.ident == variant)
                            .map(|v| &v.fields),
                        _ => None,
                    };
                    format!("{variant}{}", self.fields(fields, defs))
                }
            },
            ValueOwned::Option(None) => "None".into(),
            ValueOwned::Option(Some(v)) => {
                let inner = match ty {
                    Some(TypeOwned::Option { some_ty }) => Some(&**some_ty),
                    _ => None,
                };
                format!("Some({})", self.value(v, inner))
            }
            ValueOwned::Result(r) => {
                let (ok_ty, err_ty) = match ty {
                    Some(TypeOwned::Result { ok_ty, err_ty }) => (Some(&**ok_ty), Some(&**err_ty)),
                    _ => (None, None),
                };
                match r {
                    Ok(v) => format!("Ok({})", self.value(v, ok_ty)),
                    Err(v) => format!("Err({})", self.value(v, err_ty)),
                }
            }
            ValueOwned::Range(r) => format!(
                "{}..{}",
                self.value(&ValueOwned::Numeric(r.start), None),
                self.value(&ValueOwned::Numeric(r.end), None)
            ),
            ValueOwned::RangeInclusive(r) => format!(
                "{}..={}",
                self.value(&ValueOwned::Numeric(*r.start()), None),
                self.value(&ValueOwned::Numeric(*r.end()), None)
            ),
        }
    }

    fn list<'t>(
        &self,
        items: &[ValueOwned],
        ty: impl Fn(usize) -> Option<&'t TypeOwned>,
    ) -> String {
        items
            .iter()
            .enumerate()
            .map(|(idx, v)| self.value(v, ty(idx)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn fields(&self, fields: &FieldsValueOwned, defs: Option<&FieldsOwned>) -> String {
        // relocated flags are not in the value
        let defs: Vec<&FieldOwned> = match defs {
            Some(FieldsOwned::Named(defs) | FieldsOwned::Unnamed(defs)) => {
                defs.iter().filter(|d| !d.is_flag()).collect()
            }
            _ => vec![],
        };
        let ty = |idx: usize| defs.get(idx).map(|d| &d.ty);
        match fields {
            FieldsValueOwned::Named(named) => {
                let fields: Vec<String> = named
                    .iter()
                    .enumerate()
                    .map(|(idx, (name, v))| format!("{name}: {}", self.value(v, ty(idx))))
                    .collect();
                format!("{{ {} }}", fields.join(", "))
            }
            FieldsValueOwned::Unnamed(items) => format!("({})", self.list(items, ty)),
            FieldsValueOwned::Unit => String::new(),
        }
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
    use shrink_wrap::SerializeShrinkWrapOwned;
    use std::path::Path;
    use wire_weaver_client::ww_self::ApiItemKindOwned;
    use ww_numeric::NumericValue;

    fn quantities_bundle() -> ApiBundleOwned {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/quantities_api");
        wire_weaver_core::load(&path, Some("Quantities".into()), false).unwrap()
    }

    /// The value of `property` written by its typed `value`, as `ww` shows it.
    fn typed(property: &str, value: impl SerializeShrinkWrapOwned) -> String {
        let mut bundle = quantities_bundle();
        wire_weaver_client::ww_self::generics::prepare(&mut bundle).unwrap();
        let item = bundle
            .root
            .items
            .iter()
            .find(|i| i.ident == property)
            .unwrap();
        let ApiItemKindOwned::Property { ty, .. } = &item.kind else {
            panic!("property");
        };
        let bytes = value.to_ww_bytes_owned().unwrap();
        let value = ValueOwned::des_shrink_wrap_dyn(&bytes, ty, &bundle).unwrap();
        fmt_value(&value, Some(ty), &bundle)
    }

    #[test]
    fn quantities_with_their_units() {
        use ww_si::units::{AngularVelocity, Per, Torque};
        assert_eq!(typed("voltage", ww_si::Volt::milli(3300u16)), "3300 mV");
        assert_eq!(
            typed(
                "damping",
                ww_si::Quantity::<Per<Torque, AngularVelocity>>::from_f32(1.2e-4)
            ),
            "0.00012 N·m/(rad/s)"
        );
    }

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
            fmt_value(&value, None, &quantities_bundle()),
            r#"{ name: "b200", id: 0xa102, kind: Chip, rev: None }"#
        );
    }
}
