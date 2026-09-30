//! Python objects <-> [ValueOwned], guided by the types in an [ApiBundleOwned].
//!
//! | WireWeaver                        | Python                                                              |
//! |-----------------------------------|---------------------------------------------------------------------|
//! | `bool`                            | `bool`                                                              |
//! | integers, `UNib32`, `u4`, `nib`   | `int` (range is checked)                                            |
//! | `f32`, `f64`                      | `float` (`int` accepted)                                            |
//! | `String`, `&str`                  | `str`                                                               |
//! | `Vec<u8>`, `[u8; N]`              | `bytes` (`bytearray` or a list of ints accepted)                    |
//! | `Vec<T>`, `[T; N]`                | `list` (any iterable accepted)                                      |
//! | tuple                             | `tuple`                                                             |
//! | struct with named fields          | `dict` (dataclass, namedtuple or any object with attributes accepted)|
//! | tuple struct                      | `tuple`, a single field one is its value                            |
//! | unit struct                       | `None`                                                              |
//! | enum unit variant                 | `"Variant"`                                                         |
//! | enum variant with fields          | `{"Variant": fields}`, fields as for a struct                       |
//! | `Option<T>`                       | `None` or `T`                                                       |
//! | `Result<T, E>`                    | `{"Ok": T}` or `{"Err": E}`                                         |
//! | `Range`, `RangeInclusive`         | `(start, end)` (`range` with step 1 accepted)                       |

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PyRange, PyString, PyTuple,
};
use shrink_wrap::Nibble;
use wire_weaver_client::ww_numeric::{NumericAnyTypeOwned, NumericBaseType, NumericValue};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned,
};

pub(crate) fn to_value(
    obj: &Bound<'_, PyAny>,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> PyResult<ValueOwned> {
    let ty = resolve(ty, bundle)?;
    let expected = || {
        PyTypeError::new_err(format!(
            "expected {}, got {}: {}",
            type_name(ty, bundle),
            obj.get_type()
                .name()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            obj.repr().map(|r| r.to_string()).unwrap_or_default()
        ))
    };
    Ok(match ty {
        TypeOwned::Bool => {
            let b = obj.cast::<PyBool>().map_err(|_| expected())?;
            ValueOwned::Bool(b.is_true())
        }
        TypeOwned::NumericAny(any) => ValueOwned::Numeric(to_numeric(obj, numeric_base(any))?),
        TypeOwned::String => {
            let s = obj.cast::<PyString>().map_err(|_| expected())?;
            ValueOwned::String(s.to_str()?.to_string())
        }
        TypeOwned::Vec(inner) => ValueOwned::Vec(to_items(obj, inner, bundle).map_err(|e| {
            if e.is_instance_of::<PyTypeError>(obj.py()) && !is_iterable(obj) {
                expected()
            } else {
                e
            }
        })?),
        TypeOwned::Array { len, ty: inner } => {
            let items = to_items(obj, inner, bundle)?;
            if items.len() != len.0 as usize {
                return Err(PyValueError::new_err(format!(
                    "expected {} elements, got {}",
                    len.0,
                    items.len()
                )));
            }
            ValueOwned::Array(items)
        }
        TypeOwned::Tuple(types) => {
            let items = sequence(obj).ok_or_else(expected)?;
            if items.len() != types.len() {
                return Err(PyValueError::new_err(format!(
                    "expected a tuple of {} elements, got {}",
                    types.len(),
                    items.len()
                )));
            }
            let values = items
                .iter()
                .zip(types)
                .map(|(item, ty)| to_value(item, ty, bundle))
                .collect::<PyResult<_>>()?;
            ValueOwned::Tuple(values)
        }
        TypeOwned::Struct(item_struct) => ValueOwned::Struct {
            fields: to_fields(obj, &item_struct.fields, &item_struct.ident, bundle)?,
        },
        TypeOwned::Enum(item_enum) => {
            let (variant, payload) = if let Ok(name) = obj.cast::<PyString>() {
                (name.to_str()?.to_string(), None)
            } else if let Ok(dict) = obj.cast::<PyDict>()
                && dict.len() == 1
            {
                let (k, v) = dict.iter().next().expect("one item");
                let name = k.cast::<PyString>().map_err(|_| expected())?;
                (name.to_str()?.to_string(), Some(v))
            } else {
                return Err(expected());
            };
            let Some(def) = item_enum.variants.iter().find(|v| v.ident == variant) else {
                let names: Vec<_> = item_enum
                    .variants
                    .iter()
                    .map(|v| v.ident.as_str())
                    .collect();
                return Err(PyValueError::new_err(format!(
                    "{} has no variant '{variant}', expected one of: {}",
                    item_enum.ident,
                    names.join(", ")
                )));
            };
            let path = format!("{}::{}", item_enum.ident, def.ident);
            let fields = match (&def.fields, payload) {
                (FieldsOwned::Unit, None) => FieldsValueOwned::Unit,
                (_, Some(payload)) => to_fields(&payload, &def.fields, &path, bundle)?,
                (_, None) => {
                    return Err(PyValueError::new_err(format!(
                        "{path} has fields, pass {{\"{variant}\": fields}}"
                    )));
                }
            };
            ValueOwned::Enum { variant, fields }
        }
        TypeOwned::Option { some_ty } => {
            if obj.is_none() {
                ValueOwned::Option(None)
            } else {
                ValueOwned::Option(Some(Box::new(to_value(obj, some_ty, bundle)?)))
            }
        }
        TypeOwned::Result { ok_ty, err_ty } => {
            let dict = obj.cast::<PyDict>().map_err(|_| expected())?;
            if dict.len() != 1 {
                return Err(expected());
            }
            if let Some(v) = dict.get_item("Ok")? {
                ValueOwned::Result(Ok(Box::new(to_value(&v, ok_ty, bundle)?)))
            } else if let Some(v) = dict.get_item("Err")? {
                ValueOwned::Result(Err(Box::new(to_value(&v, err_ty, bundle)?)))
            } else {
                return Err(expected());
            }
        }
        TypeOwned::Box(inner) => to_value(obj, inner, bundle)?,
        TypeOwned::Range(base) => {
            let (start, end) = to_bounds(obj, base, true).ok_or_else(expected)??;
            ValueOwned::Range(start..end)
        }
        TypeOwned::RangeInclusive(base) => {
            let (start, end) = to_bounds(obj, base, false).ok_or_else(expected)??;
            ValueOwned::RangeInclusive(start..=end)
        }
        TypeOwned::Flag | TypeOwned::OutOfLine { .. } => {
            return Err(PyTypeError::new_err(format!("unsupported type {ty:?}")));
        }
    })
}

fn to_items(
    obj: &Bound<'_, PyAny>,
    inner: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> PyResult<Vec<ValueOwned>> {
    if is_u8(inner, bundle) {
        if let Ok(b) = obj.cast::<PyBytes>() {
            return Ok(bytes_value(b.as_bytes()));
        }
        if let Ok(b) = obj.cast::<PyByteArray>() {
            return Ok(bytes_value(&b.to_vec()));
        }
    }
    if obj.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err("expected a list, got str"));
    }
    obj.try_iter()?
        .map(|item| to_value(&item?, inner, bundle))
        .collect()
}

fn bytes_value(bytes: &[u8]) -> Vec<ValueOwned> {
    bytes
        .iter()
        .map(|b| ValueOwned::Numeric(NumericValue::U8(*b)))
        .collect()
}

fn to_fields(
    obj: &Bound<'_, PyAny>,
    defs: &FieldsOwned,
    path: &str,
    bundle: &ApiBundleOwned,
) -> PyResult<FieldsValueOwned> {
    Ok(match defs {
        FieldsOwned::Named(defs) => {
            let dict = as_dict(obj).ok_or_else(|| {
                PyTypeError::new_err(format!(
                    "{path}: expected a dict with fields {}",
                    field_names(defs)
                ))
            })?;
            let mut values = vec![];
            for (k, v) in dict.iter() {
                let name = k
                    .cast::<PyString>()
                    .map_err(|_| PyTypeError::new_err(format!("{path}: field names must be str")))?
                    .to_str()?
                    .to_string();
                let Some(def) = defs.iter().find(|d| d.ident.as_deref() == Some(&name)) else {
                    return Err(PyValueError::new_err(format!(
                        "{path} has no field '{name}', expected: {}",
                        field_names(defs)
                    )));
                };
                let value = to_value(&v, &def.ty, bundle)
                    .map_err(|e| prefix_err(obj.py(), e, &format!("{path}.{name}")))?;
                values.push((name, value));
            }
            // missing Option and default fields are filled in during serialization
            FieldsValueOwned::Named(values)
        }
        FieldsOwned::Unnamed(defs) => {
            let items = match sequence(obj) {
                Some(items) if defs.len() != 1 || obj.is_instance_of::<PyTuple>() => items,
                // single field: the value itself
                _ if defs.len() == 1 => vec![obj.clone()],
                _ => {
                    return Err(PyTypeError::new_err(format!(
                        "{path}: expected a tuple of {} fields",
                        defs.len()
                    )));
                }
            };
            if items.len() != defs.len() {
                return Err(PyValueError::new_err(format!(
                    "{path}: expected {} fields, got {}",
                    defs.len(),
                    items.len()
                )));
            }
            let mut values = vec![];
            for (idx, (item, def)) in items.iter().zip(defs).enumerate() {
                values.push(
                    to_value(item, &def.ty, bundle)
                        .map_err(|e| prefix_err(obj.py(), e, &format!("{path}.{idx}")))?,
                );
            }
            FieldsValueOwned::Unnamed(values)
        }
        FieldsOwned::Unit => {
            if !obj.is_none() {
                return Err(PyTypeError::new_err(format!(
                    "{path} has no fields, pass None"
                )));
            }
            FieldsValueOwned::Unit
        }
    })
}

pub(crate) fn prefix_err(py: Python<'_>, e: PyErr, path: &str) -> PyErr {
    let msg = format!("{path}: {}", e.value(py));
    if e.is_instance_of::<PyTypeError>(py) {
        PyTypeError::new_err(msg)
    } else if e.is_instance_of::<PyValueError>(py) {
        PyValueError::new_err(msg)
    } else {
        e
    }
}

/// dict, or dataclass / namedtuple / plain object turned into one.
fn as_dict<'py>(obj: &Bound<'py, PyAny>) -> Option<Bound<'py, PyDict>> {
    if let Ok(dict) = obj.cast::<PyDict>() {
        return Some(dict.clone());
    }
    if let Ok(d) = obj.call_method0("_asdict")
        && let Ok(d) = d.cast_into::<PyDict>()
    {
        return Some(d);
    }
    if obj.hasattr("__dataclass_fields__").unwrap_or(false) {
        let dataclasses = obj.py().import("dataclasses").ok()?;
        return dataclasses
            .call_method1("asdict", (obj,))
            .ok()?
            .cast_into::<PyDict>()
            .ok();
    }
    obj.getattr("__dict__").ok()?.cast_into::<PyDict>().ok()
}

fn sequence<'py>(obj: &Bound<'py, PyAny>) -> Option<Vec<Bound<'py, PyAny>>> {
    if let Ok(t) = obj.cast::<PyTuple>() {
        return Some(t.iter().collect());
    }
    if let Ok(l) = obj.cast::<PyList>() {
        return Some(l.iter().collect());
    }
    None
}

fn is_iterable(obj: &Bound<'_, PyAny>) -> bool {
    obj.try_iter().is_ok()
}

type Bounds = (NumericValue, NumericValue);

fn to_bounds(
    obj: &Bound<'_, PyAny>,
    base: &NumericBaseType,
    allow_range: bool,
) -> Option<PyResult<Bounds>> {
    if allow_range && let Ok(r) = obj.cast::<PyRange>() {
        let step: i64 = r.getattr("step").ok()?.extract().ok()?;
        if step != 1 {
            return Some(Err(PyValueError::new_err(
                "only ranges with step 1 are supported",
            )));
        }
        let start = r.getattr("start").ok()?;
        let stop = r.getattr("stop").ok()?;
        return Some(to_numeric(&start, base).and_then(|s| Ok((s, to_numeric(&stop, base)?))));
    }
    let items = sequence(obj)?;
    if items.len() != 2 {
        return None;
    }
    Some(to_numeric(&items[0], base).and_then(|s| Ok((s, to_numeric(&items[1], base)?))))
}

fn to_numeric(obj: &Bound<'_, PyAny>, base: &NumericBaseType) -> PyResult<NumericValue> {
    use NumericBaseType as B;
    use NumericValue as V;
    if matches!(base, B::F32 | B::F64) {
        if !(obj.is_instance_of::<PyFloat>() || obj.is_instance_of::<PyInt>())
            || obj.is_instance_of::<PyBool>()
        {
            return Err(PyTypeError::new_err(format!(
                "expected float, got {}",
                obj.repr()?
            )));
        }
        let v: f64 = obj.extract()?;
        return Ok(match base {
            B::F32 => V::F32(v as f32),
            _ => V::F64(v),
        });
    }
    if !obj.is_instance_of::<PyInt>() || obj.is_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err(format!(
            "expected int ({}), got {}",
            numeric_name(base),
            obj.repr()?
        )));
    }
    let out_of_range = || {
        PyValueError::new_err(format!(
            "{} is out of range for {}",
            obj.repr().map(|r| r.to_string()).unwrap_or_default(),
            numeric_name(base)
        ))
    };
    if matches!(base, B::U128) {
        return Ok(V::U128(obj.extract().map_err(|_| out_of_range())?));
    }
    let v: i128 = obj.extract().map_err(|_| out_of_range())?;
    macro_rules! int {
        ($t:ty) => {
            <$t>::try_from(v).map_err(|_| out_of_range())?
        };
    }
    Ok(match base {
        B::Nibble => V::Nibble(Nibble::new(int!(u8)).ok_or_else(out_of_range)?),
        B::U8 => V::U8(int!(u8)),
        B::U16 => V::U16(int!(u16)),
        B::U32 => V::U32(int!(u32)),
        B::UNib32 => V::UNib32(int!(u32)),
        B::U64 => V::U64(int!(u64)),
        B::I8 => V::I8(int!(i8)),
        B::I16 => V::I16(int!(i16)),
        B::I32 => V::I32(int!(i32)),
        B::I64 => V::I64(int!(i64)),
        B::I128 => V::I128(v),
        B::UB(bits) => {
            let bits = bits.0;
            if v < 0 || (bits < 64 && v >= 1i128 << bits) {
                return Err(out_of_range());
            }
            match bits {
                0..=8 => V::U8(v as u8),
                9..=16 => V::U16(v as u16),
                17..=32 => V::U32(v as u32),
                _ => V::U64(v as u64),
            }
        }
        B::IB(bits) => {
            let bits = bits.0;
            if v < -(1i128 << (bits - 1)) || v >= 1i128 << (bits - 1) {
                return Err(out_of_range());
            }
            match bits {
                0..=8 => V::I8(v as i8),
                9..=16 => V::I16(v as i16),
                17..=32 => V::I32(v as i32),
                _ => V::I64(v as i64),
            }
        }
        other => {
            return Err(PyTypeError::new_err(format!(
                "numeric type {other:?} is not supported yet"
            )));
        }
    })
}

pub(crate) fn to_py<'py>(
    py: Python<'py>,
    value: &ValueOwned,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> PyResult<Bound<'py, PyAny>> {
    let ty = resolve(ty, bundle)?;
    Ok(match (value, ty) {
        (ValueOwned::Bool(b), _) => PyBool::new(py, *b).to_owned().into_any(),
        (ValueOwned::Numeric(n), _) => numeric_to_py(py, n)?,
        (ValueOwned::String(s), _) => PyString::new(py, s).into_any(),
        (ValueOwned::Vec(items), TypeOwned::Vec(inner))
        | (ValueOwned::Array(items), TypeOwned::Array { ty: inner, .. }) => {
            if is_u8(inner, bundle) {
                let bytes: Vec<u8> = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Numeric(NumericValue::U8(b)) => Ok(*b),
                        other => Err(PyTypeError::new_err(format!("expected u8, got {other:?}"))),
                    })
                    .collect::<PyResult<_>>()?;
                PyBytes::new(py, &bytes).into_any()
            } else {
                let items = items
                    .iter()
                    .map(|v| to_py(py, v, inner, bundle))
                    .collect::<PyResult<Vec<_>>>()?;
                PyList::new(py, items)?.into_any()
            }
        }
        (ValueOwned::Tuple(items), TypeOwned::Tuple(types)) => {
            let items = items
                .iter()
                .zip(types)
                .map(|(v, ty)| to_py(py, v, ty, bundle))
                .collect::<PyResult<Vec<_>>>()?;
            PyTuple::new(py, items)?.into_any()
        }
        (ValueOwned::Struct { fields }, TypeOwned::Struct(item_struct)) => {
            fields_to_py(py, fields, &item_struct.fields, bundle)?
        }
        (ValueOwned::Enum { variant, fields }, TypeOwned::Enum(item_enum)) => {
            let def = item_enum
                .variants
                .iter()
                .find(|v| &v.ident == variant)
                .ok_or_else(|| PyValueError::new_err(format!("unknown variant {variant}")))?;
            if matches!(fields, FieldsValueOwned::Unit) {
                PyString::new(py, variant).into_any()
            } else {
                let dict = PyDict::new(py);
                dict.set_item(variant, fields_to_py(py, fields, &def.fields, bundle)?)?;
                dict.into_any()
            }
        }
        (ValueOwned::Option(v), TypeOwned::Option { some_ty }) => match v {
            Some(v) => to_py(py, v, some_ty, bundle)?,
            None => py.None().into_bound(py),
        },
        (ValueOwned::Result(r), TypeOwned::Result { ok_ty, err_ty }) => {
            let dict = PyDict::new(py);
            match r {
                Ok(v) => dict.set_item("Ok", to_py(py, v, ok_ty, bundle)?)?,
                Err(e) => dict.set_item("Err", to_py(py, e, err_ty, bundle)?)?,
            }
            dict.into_any()
        }
        (value, TypeOwned::Box(inner)) => to_py(py, value, inner, bundle)?,
        (ValueOwned::Range(r), _) => PyTuple::new(
            py,
            [numeric_to_py(py, &r.start)?, numeric_to_py(py, &r.end)?],
        )?
        .into_any(),
        (ValueOwned::RangeInclusive(r), _) => PyTuple::new(
            py,
            [numeric_to_py(py, r.start())?, numeric_to_py(py, r.end())?],
        )?
        .into_any(),
        (value, ty) => {
            return Err(PyTypeError::new_err(format!(
                "value {value:?} does not match type {}",
                type_name(ty, bundle)
            )));
        }
    })
}

fn fields_to_py<'py>(
    py: Python<'py>,
    fields: &FieldsValueOwned,
    defs: &FieldsOwned,
    bundle: &ApiBundleOwned,
) -> PyResult<Bound<'py, PyAny>> {
    let def_ty = |defs: &[FieldOwned], idx: usize| -> PyResult<TypeOwned> {
        defs.get(idx)
            .map(|d| d.ty.clone())
            .ok_or_else(|| PyValueError::new_err("more fields than defined"))
    };
    Ok(match (fields, defs) {
        (FieldsValueOwned::Named(values), FieldsOwned::Named(defs)) => {
            let dict = PyDict::new(py);
            for (idx, (name, value)) in values.iter().enumerate() {
                dict.set_item(name, to_py(py, value, &def_ty(defs, idx)?, bundle)?)?;
            }
            dict.into_any()
        }
        (FieldsValueOwned::Unnamed(values), FieldsOwned::Unnamed(defs)) => {
            let items = values
                .iter()
                .enumerate()
                .map(|(idx, v)| to_py(py, v, &def_ty(defs, idx)?, bundle))
                .collect::<PyResult<Vec<_>>>()?;
            if items.len() == 1 {
                items.into_iter().next().expect("one item")
            } else {
                PyTuple::new(py, items)?.into_any()
            }
        }
        (FieldsValueOwned::Unit, _) => py.None().into_bound(py),
        _ => return Err(PyTypeError::new_err("fields do not match their definition")),
    })
}

fn numeric_to_py<'py>(py: Python<'py>, n: &NumericValue) -> PyResult<Bound<'py, PyAny>> {
    use NumericValue as V;
    Ok(match n {
        V::Nibble(v) => v.value().into_pyobject(py)?.into_any(),
        V::U8(v) => v.into_pyobject(py)?.into_any(),
        V::U16(v) => v.into_pyobject(py)?.into_any(),
        V::U32(v) | V::UNib32(v) => v.into_pyobject(py)?.into_any(),
        V::U64(v) => v.into_pyobject(py)?.into_any(),
        V::U128(v) => v.into_pyobject(py)?.into_any(),
        V::I8(v) => v.into_pyobject(py)?.into_any(),
        V::I16(v) => v.into_pyobject(py)?.into_any(),
        V::I32(v) => v.into_pyobject(py)?.into_any(),
        V::I64(v) => v.into_pyobject(py)?.into_any(),
        V::I128(v) => v.into_pyobject(py)?.into_any(),
        V::F32(v) => (*v as f64).into_pyobject(py)?.into_any(),
        V::F64(v) => v.into_pyobject(py)?.into_any(),
        other => {
            return Err(PyTypeError::new_err(format!(
                "numeric value {other:?} is not supported yet"
            )));
        }
    })
}

fn resolve<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> PyResult<&'a TypeOwned> {
    ty.get_in_line(bundle)
        .map_err(|e| PyValueError::new_err(format!("{e:#}")))
}

fn is_u8(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    matches!(
        ty.get_in_line(bundle),
        Ok(TypeOwned::NumericAny(NumericAnyTypeOwned::Base(
            NumericBaseType::U8
        )))
    )
}

fn numeric_base(any: &NumericAnyTypeOwned) -> &NumericBaseType {
    match any {
        NumericAnyTypeOwned::Base(base)
        | NumericAnyTypeOwned::SubType { base, .. }
        | NumericAnyTypeOwned::ShiftScale { base, .. } => base,
    }
}

fn numeric_name(base: &NumericBaseType) -> String {
    match base {
        NumericBaseType::UB(bits) => format!("u{}", bits.0),
        NumericBaseType::IB(bits) => format!("i{}", bits.0),
        other => format!("{other:?}").to_lowercase(),
    }
}

pub(crate) fn type_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|_| format!("{ty:?}"))
}

fn field_names(defs: &[FieldOwned]) -> String {
    defs.iter()
        .filter_map(|d| d.ident.as_deref())
        .collect::<Vec<_>>()
        .join(", ")
}
