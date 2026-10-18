//! A corpus value (JSON in the form `wire_weaver_py` uses for Python values) -> [ValueOwned], guided by the type
//! from the API model: what a dynamic consumer is given to encode, and what it must decode.

use serde_json::Value as J;
use wire_weaver::shrink_wrap::{Nibble, UNib32, UVlq32, UVlq64};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType as B, NumericValue as N};
use ww_self::{ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned};

pub fn to_value(j: &J, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned, String> {
    let ty = ty.get_in_line(bundle).map_err(|e| format!("{e:#}"))?;
    let wrong = || format!("{j} is not a {}", type_name(ty, bundle));
    Ok(match ty {
        TypeOwned::Bool => ValueOwned::Bool(j.as_bool().ok_or_else(wrong)?),
        TypeOwned::NumericAny(any) => ValueOwned::Numeric(numeric(j, base(any))?),
        TypeOwned::String => ValueOwned::String(j.as_str().ok_or_else(wrong)?.into()),
        TypeOwned::MaxLen { ty, .. } => to_value(j, ty, bundle)?,
        TypeOwned::Vec(inner) | TypeOwned::Series(inner) => {
            ValueOwned::Vec(items(j, inner, bundle)?)
        }
        TypeOwned::Array { ty: inner, .. } => ValueOwned::Array(items(j, inner, bundle)?),
        TypeOwned::Tuple(types) => {
            let a = j
                .as_array()
                .filter(|a| a.len() == types.len())
                .ok_or_else(wrong)?;
            ValueOwned::Tuple(
                a.iter()
                    .zip(types)
                    .map(|(j, ty)| to_value(j, ty, bundle))
                    .collect::<Result<_, _>>()?,
            )
        }
        TypeOwned::Struct(s) => ValueOwned::Struct {
            fields: fields(j, &s.fields, bundle)?,
        },
        TypeOwned::Enum(e) => {
            let (name, payload) = match j {
                J::String(name) => (name.clone(), None),
                J::Object(o) if o.len() == 1 => {
                    let (k, v) = o.iter().next().expect("one");
                    (k.clone(), Some(v))
                }
                _ => return Err(wrong()),
            };
            let fields = match e.variants.iter().find(|v| v.ident == name) {
                Some(def) => match payload {
                    Some(p) => fields(p, &def.fields, bundle)?,
                    None if matches!(def.fields, FieldsOwned::Unit) => FieldsValueOwned::Unit,
                    None => return Err(format!("{}::{name} has fields", e.ident)),
                },
                // a variant from a newer version, as the dynamic decoder reads it
                None if name.starts_with("<unknown ") && payload.is_none() => {
                    FieldsValueOwned::Unit
                }
                None => return Err(format!("{} has no variant {name}", e.ident)),
            };
            ValueOwned::Enum {
                variant: name,
                fields,
            }
        }
        TypeOwned::Option { some_ty } => ValueOwned::Option(match j {
            J::Null => None,
            j => Some(Box::new(to_value(j, some_ty, bundle)?)),
        }),
        TypeOwned::Result { ok_ty, err_ty } => {
            let o = j.as_object().filter(|o| o.len() == 1).ok_or_else(wrong)?;
            ValueOwned::Result(match (o.get("Ok"), o.get("Err")) {
                (Some(v), _) => Ok(Box::new(to_value(v, ok_ty, bundle)?)),
                (_, Some(v)) => Err(Box::new(to_value(v, err_ty, bundle)?)),
                _ => return Err(wrong()),
            })
        }
        TypeOwned::Box(inner) => to_value(j, inner, bundle)?,
        TypeOwned::Range(b) | TypeOwned::RangeInclusive(b) => {
            let a = j.as_array().filter(|a| a.len() == 2).ok_or_else(wrong)?;
            let (start, end) = (numeric(&a[0], b)?, numeric(&a[1], b)?);
            if matches!(ty, TypeOwned::Range(_)) {
                ValueOwned::Range(start..end)
            } else {
                ValueOwned::RangeInclusive(start..=end)
            }
        }
        TypeOwned::Flag
        | TypeOwned::OutOfLine { .. }
        | TypeOwned::Instance { .. }
        | TypeOwned::Param(_) => return Err(format!("unexpected type {ty:?}")),
    })
}

fn items(j: &J, inner: &TypeOwned, bundle: &ApiBundleOwned) -> Result<Vec<ValueOwned>, String> {
    j.as_array()
        .ok_or_else(|| format!("{j} is not a list"))?
        .iter()
        .map(|j| to_value(j, inner, bundle))
        .collect()
}

/// Fields in the order of their definition, as a decoder gives them.
fn fields(j: &J, defs: &FieldsOwned, bundle: &ApiBundleOwned) -> Result<FieldsValueOwned, String> {
    Ok(match defs {
        FieldsOwned::Named(defs) => {
            let o = j
                .as_object()
                .ok_or_else(|| format!("{j} is not an object of fields"))?;
            let named: Vec<&FieldOwned> = defs.iter().filter(|d| !d.is_flag()).collect();
            if let Some(k) = o
                .keys()
                .find(|k| !named.iter().any(|d| d.ident.as_deref() == Some(k)))
            {
                return Err(format!("no field {k}"));
            }
            let mut values = vec![];
            for def in named {
                let name = def.ident.clone().unwrap_or_default();
                let Some(v) = o.get(&name) else {
                    return Err(format!("field {name} missing (give every field)"));
                };
                values.push((name, to_value(v, &def.ty, bundle)?));
            }
            FieldsValueOwned::Named(values)
        }
        FieldsOwned::Unnamed(defs) => {
            let items: Vec<&J> = match j {
                J::Array(a) if defs.len() != 1 => a.iter().collect(),
                j if defs.len() == 1 => vec![j],
                _ => return Err(format!("{j} is not a list of {} fields", defs.len())),
            };
            if items.len() != defs.len() {
                return Err(format!("{} fields, {} given", defs.len(), items.len()));
            }
            FieldsValueOwned::Unnamed(
                items
                    .into_iter()
                    .zip(defs)
                    .map(|(j, d)| to_value(j, &d.ty, bundle))
                    .collect::<Result<_, _>>()?,
            )
        }
        FieldsOwned::Unit if j.is_null() => FieldsValueOwned::Unit,
        FieldsOwned::Unit => return Err(format!("{j}: a unit has no fields, give null")),
    })
}

fn base(any: &NumericAnyTypeOwned) -> &B {
    match any {
        NumericAnyTypeOwned::Base(base)
        | NumericAnyTypeOwned::SubType { base, .. }
        | NumericAnyTypeOwned::ShiftScale { base, .. } => base,
    }
}

/// An integer of the corpus: a JSON number, or a string for one beyond 64 bits (JSON parsers lose those).
pub fn int(j: &J) -> Option<i128> {
    match j {
        J::Number(n) => n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)),
        J::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn numeric(j: &J, base: &B) -> Result<N, String> {
    if matches!(base, B::F32 | B::F64) {
        let v = j.as_f64().ok_or_else(|| format!("{j} is not a float"))?;
        return Ok(if matches!(base, B::F32) {
            N::F32(v as f32)
        } else {
            N::F64(v)
        });
    }
    if matches!(base, B::U128) {
        let v: u128 = match j {
            J::String(s) => s.parse().ok(),
            j => j.as_u64().map(u128::from),
        }
        .ok_or_else(|| format!("{j} is not a u128"))?;
        return Ok(N::U128(v));
    }
    let v = int(j).ok_or_else(|| format!("{j} is not an integer"))?;
    let range = || format!("{v} is out of range for {base:?}");
    macro_rules! int {
        ($t:ty) => {
            <$t>::try_from(v).map_err(|_| range())?
        };
    }
    Ok(match base {
        B::Nibble => N::Nibble(Nibble::new(int!(u8)).ok_or_else(range)?),
        B::U8 => N::U8(int!(u8)),
        B::U16 => N::U16(int!(u16)),
        B::U32 => N::U32(int!(u32)),
        B::UNib32 => N::UNib32(UNib32(int!(u32))),
        B::UVlq32 => N::UVlq32(UVlq32(int!(u32))),
        B::UVlq64 => N::UVlq64(UVlq64(int!(u64))),
        B::U64 => N::U64(int!(u64)),
        B::I8 => N::I8(int!(i8)),
        B::I16 => N::I16(int!(i16)),
        B::I32 => N::I32(int!(i32)),
        B::I64 => N::I64(int!(i64)),
        B::I128 => N::I128(v),
        // the value types the dynamic decoder gives bit-sized numbers
        B::UB(bits) => match bits.0 {
            0..=8 => N::U8(int!(u8)),
            9..=16 => N::U16(int!(u16)),
            17..=32 => N::U32(int!(u32)),
            _ => N::U64(int!(u64)),
        },
        B::IB(bits) => match bits.0 {
            0..=8 => N::I8(int!(i8)),
            9..=16 => N::I16(int!(i16)),
            17..=32 => N::I32(int!(i32)),
            _ => N::I64(int!(i64)),
        },
        other => return Err(format!("numeric type {other:?} has no corpus form")),
    })
}

pub fn type_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|_| format!("{ty:?}"))
}
