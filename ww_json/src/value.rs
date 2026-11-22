//! The value codec: [ValueOwned] <-> [serde_json::Value], guided by the type from the API model.
//!
//! | WireWeaver                       | JSON                                                                         |
//! |----------------------------------|------------------------------------------------------------------------------|
//! | `bool`                           | `true`, `false`                                                              |
//! | integers, `UNib32`, `u4`, `nib`  | number; beyond ±2⁵³ (and every `u128`/`i128` that large) a decimal string    |
//! | `f32`, `f64`                     | number; `"NaN"`, `"inf"`, `"-inf"` as strings                                |
//! | `String`                         | string                                                                       |
//! | `Vec<u8>`, `[u8; N]`             | lowercase hex string (a list of numbers accepted as input)                   |
//! | `Vec<T>`, `[T; N]`, `Series<T>`  | list                                                                         |
//! | tuple                            | list                                                                         |
//! | struct with named fields         | object, fields in the order of the definition                                |
//! | tuple struct                     | list, a single field one is its value                                        |
//! | unit struct                      | `null`                                                                       |
//! | enum unit variant                | `"Variant"`                                                                  |
//! | enum variant with fields         | `{"Variant": fields}`, fields as for a struct                                |
//! | enum variant of a newer version  | `"<unknown N>"` (read from a device, accepted back)                          |
//! | `Option<T>`                      | `null` or `T`                                                                |
//! | `Result<T, E>`                   | `{"Ok": T}` or `{"Err": E}`                                                  |
//! | `Range`, `RangeInclusive`        | `[start, end]`                                                               |
//! | `ww_si` quantity, `AnyQuantity`, `Fixed`, `Point`, `Coded` | text with the unit, `"5 ms"`; input also takes a quantity in any unit that converts exactly, or the stored number |
//! | `Index`, `IndexFrom1`, `Seq`     | the number, as for any integer                                               |
//!
//! Output is deterministic. Input is strict: an unknown field, a value of another kind and a number that does not
//! fit are errors naming the [JsonPath] of the failing value.

use crate::error::{ErrorKind, JsonError, JsonPath, Step};
use serde_json::{Map, Number, Value as J};
use shrink_wrap::{INib32, Nibble, UNib32, UVlq32, UVlq64};
use wire_weaver_client::quantity::{
    DynCount, DynQuantity, coded_of, fixed_of, is_any_quantity, is_quantity, parse_point_in,
    parse_quantity_in, stored_number, unit_of,
};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType as B, NumericValue as N};
use ww_self::{
    ApiBundleOwned, ArgumentOwned, FieldOwned, FieldsOwned, FieldsValueOwned, MAX_DECODE_DEPTH,
    TypeOwned, ValueOwned,
};

/// 2⁵³: integers up to it are exact in a JSON number of the usual double.
pub const EXACT_INT: i128 = 1 << 53;

type Result<T> = std::result::Result<T, JsonError>;

/// The codec for the types of one API bundle (a [prepared](ww_self::generics::prepare) one).
///
/// ```
/// # use ww_json::Codec;
/// # fn f(bundle: &ww_self::ApiBundleOwned, ty: &ww_self::TypeOwned) {
/// let codec = Codec::new(bundle).root("args");
/// let value = codec.from_json(&serde_json::json!({"timeout": "5 ms"}), ty);
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct Codec<'a> {
    bundle: &'a ApiBundleOwned,
    root: &'a str,
    raw: bool,
}

impl<'a> Codec<'a> {
    /// A codec for `bundle`, errors rooted at `value`.
    pub fn new(bundle: &'a ApiBundleOwned) -> Self {
        Codec {
            bundle,
            root: "value",
            raw: false,
        }
    }

    /// The structural form: quantities, `Fixed`, `Point`, `Index`, `Seq` and the like as the structs they are on the
    /// wire (`{"value": 5000}`, `{"prefix": "Milli", "value": {"U32": 3300}}`), byte lists as lists of numbers.
    /// What the conformance corpus is written in, so every consumer reads the same values (TEST-23).
    pub fn raw(mut self) -> Self {
        self.raw = true;
        self
    }

    /// The name errors start their path with (`args`, `items`).
    pub fn root(mut self, root: &'a str) -> Self {
        self.root = root;
        self
    }

    /// `value` of type `ty` as JSON.
    pub fn to_json(&self, value: &ValueOwned, ty: &TypeOwned) -> Result<J> {
        Ser {
            bundle: self.bundle,
            path: JsonPath::new(self.root),
            raw: self.raw,
        }
        .value(value, ty, 0)
    }

    /// The JSON `json` as a value of type `ty`.
    pub fn from_json(&self, json: &J, ty: &TypeOwned) -> Result<ValueOwned> {
        De {
            bundle: self.bundle,
            path: JsonPath::new(self.root),
            raw: self.raw,
        }
        .value(json, ty, 0)
    }

    /// The arguments of a method or stream function as one JSON object, by name, in the order of the definition
    /// (a `#[flatten]` argument's fields in its place).
    pub fn args_to_json(&self, values: &[ValueOwned], args: &[ArgumentOwned]) -> Result<J> {
        let mut ser = Ser {
            bundle: self.bundle,
            path: JsonPath::new(self.root),
            raw: self.raw,
        };
        let fields = ser.fields_of(args)?;
        if values.len() > fields.len() {
            return Err(JsonError::new(
                &ser.path,
                ErrorKind::Length {
                    expected: format!("at most {} arguments", fields.len()),
                    got: values.len(),
                },
            ));
        }
        let mut object = Map::new();
        for (field, value) in fields.iter().zip(values) {
            let name = field.ident.clone().unwrap_or_default();
            ser.path.push(Step::Field(name.clone()));
            object.insert(name, ser.value(value, &field.ty, 1)?);
            ser.path.pop();
        }
        Ok(J::Object(object))
    }

    /// The JSON object `json` of arguments as their values, in the order of the definition. An argument with a
    /// default (an `Option`, `Vec` or `String` one at the end, or one with `#[default]`) may be left out; an
    /// `Option` one in the middle is `None` when left out.
    pub fn args_from_json(&self, json: &J, args: &[ArgumentOwned]) -> Result<Vec<ValueOwned>> {
        let mut de = De {
            bundle: self.bundle,
            path: JsonPath::new(self.root),
            raw: self.raw,
        };
        let fields = de.fields_of(args)?;
        let Some(object) = json.as_object() else {
            return Err(de.wrong(json, "an object of arguments"));
        };
        let names: Vec<String> = fields.iter().filter_map(|f| f.ident.clone()).collect();
        if let Some(unknown) = object.keys().find(|k| !names.contains(k)) {
            return Err(JsonError::new(
                &de.path,
                ErrorKind::UnknownField {
                    name: unknown.clone(),
                    expected: names,
                },
            ));
        }
        de.named_values(object, &fields, true, 1)
    }
}

/// [Codec::to_json] with the root `value`.
pub fn to_json(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<J> {
    Codec::new(bundle).to_json(value, ty)
}

/// [Codec::from_json] with the root `value`.
pub fn from_json(json: &J, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    Codec::new(bundle).from_json(json, ty)
}

/// [Codec::args_to_json] with the root `args`.
pub fn args_to_json(
    values: &[ValueOwned],
    args: &[ArgumentOwned],
    bundle: &ApiBundleOwned,
) -> Result<J> {
    Codec::new(bundle).root("args").args_to_json(values, args)
}

/// [Codec::args_from_json] with the root `args`.
pub fn args_from_json(
    json: &J,
    args: &[ArgumentOwned],
    bundle: &ApiBundleOwned,
) -> Result<Vec<ValueOwned>> {
    Codec::new(bundle).root("args").args_from_json(json, args)
}

/// The kind of a JSON value, in words.
fn kind_of(j: &J) -> &'static str {
    match j {
        J::Null => "null",
        J::Bool(_) => "a boolean",
        J::Number(_) => "a number",
        J::String(_) => "a string",
        J::Array(_) => "a list",
        J::Object(_) => "an object",
    }
}

fn type_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|_| format!("{ty:?}"))
}

fn numeric_base(any: &NumericAnyTypeOwned) -> &B {
    match any {
        NumericAnyTypeOwned::Base(base)
        | NumericAnyTypeOwned::SubType { base, .. }
        | NumericAnyTypeOwned::ShiftScale { base, .. } => base,
    }
}

/// `u8`, `ub4`, `f32`.
pub(crate) fn base_name(base: &B) -> String {
    ww_self::expr::base_name(base)
}

/// Whether `ty` is exactly `u8`: a list of them is bytes.
pub(crate) fn is_u8(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    matches!(
        ty.get_in_line(bundle),
        Ok(TypeOwned::NumericAny(NumericAnyTypeOwned::Base(B::U8)))
    )
}

/// What a quantity type is written as: the text of a quantity.
fn quantity_type(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    is_quantity(ty, bundle) || coded_of(ty, bundle).is_some()
}

/// The JSON number for an integer, a string beyond ±2⁵³.
fn int_json(i: i128) -> J {
    if (-EXACT_INT..=EXACT_INT).contains(&i) {
        J::Number(Number::from(i as i64))
    } else {
        J::String(i.to_string())
    }
}

fn float_json(x: f64, shortest: String) -> J {
    if !x.is_finite() {
        return J::String(if x.is_nan() {
            "NaN".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        });
    }
    // an f32 by its shortest decimal text, so 0.1f32 reads 0.1 and not 0.10000000149011612
    let x = shortest.parse::<f64>().unwrap_or(x);
    Number::from_f64(x).map_or_else(|| J::String(shortest), J::Number)
}

/// A number as JSON, `None` for a variant the JSON form doesn't cover.
fn numeric_json(n: &N) -> Option<J> {
    Some(match n {
        N::F32(x) => float_json(f64::from(*x), x.to_string()),
        N::F64(x) => float_json(*x, x.to_string()),
        N::U128(x) => match i128::try_from(*x) {
            Ok(i) => int_json(i),
            Err(_) => J::String(x.to_string()),
        },
        n => int_json(n.to_i128()?),
    })
}

/// Hex of `bytes`, lowercase, no separators.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        _ = write!(s, "{b:02x}");
        s
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: String = text
        .trim()
        .trim_start_matches("0x")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    if !digits.len().is_multiple_of(2) || !digits.is_ascii() {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok())
        .collect()
}

/// The fields of a definition that are values: the relocated presence flags are not.
fn value_fields(defs: &[FieldOwned]) -> impl Iterator<Item = &FieldOwned> {
    defs.iter().filter(|d| !d.is_flag())
}

fn field_names(defs: &[FieldOwned]) -> Vec<String> {
    value_fields(defs).filter_map(|d| d.ident.clone()).collect()
}

// ------------------------------------------------------------------------------------------------ value -> JSON

struct Ser<'a> {
    bundle: &'a ApiBundleOwned,
    path: JsonPath,
    raw: bool,
}

impl Ser<'_> {
    fn err(&self, kind: ErrorKind) -> JsonError {
        JsonError::new(&self.path, kind)
    }

    fn mismatch(&self, value: &ValueOwned, ty: &TypeOwned) -> JsonError {
        self.err(ErrorKind::Mismatch(format!(
            "{value:?} is no {}",
            type_name(ty, self.bundle)
        )))
    }

    fn fields_of(&self, args: &[ArgumentOwned]) -> Result<Vec<FieldOwned>> {
        ArgumentOwned::as_fields(args, self.bundle)
            .map_err(|e| self.err(ErrorKind::Model(format!("{e:#}"))))
    }

    fn value(&mut self, value: &ValueOwned, ty: &TypeOwned, depth: usize) -> Result<J> {
        if depth >= MAX_DECODE_DEPTH {
            return Err(self.err(ErrorKind::TooDeep(MAX_DECODE_DEPTH)));
        }
        let bundle = self.bundle;
        let ty = ty
            .get_in_line(bundle)
            .map_err(|e| self.err(ErrorKind::Model(format!("{e:#}"))))?;
        if !self.raw && quantity_type(ty, bundle) {
            if let Some(q) = DynQuantity::from_value(value, ty, bundle) {
                return Ok(quantity_json(&q));
            }
            return Err(self.err(ErrorKind::Quantity(format!(
                "{value:?} is no value of {}",
                type_name(ty, bundle)
            ))));
        }
        if !self.raw
            && let Some(count) = DynCount::from_value(value, ty, bundle)
        {
            return numeric_json(&count.value)
                .ok_or_else(|| self.err(ErrorKind::Unsupported(format!("{count}"))));
        }
        match (value, ty) {
            (_, TypeOwned::MaxLen { ty: inner, .. }) | (_, TypeOwned::Box(inner)) => {
                self.value(value, inner, depth + 1)
            }
            (ValueOwned::Bool(b), TypeOwned::Bool) => Ok(J::Bool(*b)),
            (ValueOwned::Numeric(n), TypeOwned::NumericAny(_)) => numeric_json(n)
                .ok_or_else(|| self.err(ErrorKind::Unsupported(format!("number {n:?}")))),
            (ValueOwned::String(s), TypeOwned::String) => Ok(J::String(s.clone())),
            (ValueOwned::Vec(items), TypeOwned::Vec(inner) | TypeOwned::Series(inner))
            | (ValueOwned::Array(items), TypeOwned::Array { ty: inner, .. }) => {
                if !self.raw
                    && is_u8(inner, bundle)
                    && !matches!(ty, TypeOwned::Series(_))
                    && let Some(bytes) = u8s(items)
                {
                    return Ok(J::String(hex(&bytes)));
                }
                self.list(items, |_| inner, depth)
            }
            (ValueOwned::Tuple(items), TypeOwned::Tuple(types)) => {
                if items.len() != types.len() {
                    return Err(self.mismatch(value, ty));
                }
                self.list(items, |i| &types[i], depth)
            }
            (ValueOwned::Struct { fields }, TypeOwned::Struct(s)) => {
                self.fields(fields, &s.fields, depth)
            }
            (ValueOwned::Enum { variant, fields }, TypeOwned::Enum(e)) => {
                match e.variants.iter().find(|v| &v.ident == variant) {
                    Some(def) => {
                        if matches!(fields, FieldsValueOwned::Unit) {
                            return Ok(J::String(variant.clone()));
                        }
                        self.path.push(Step::Field(variant.clone()));
                        let inner = self.fields(fields, &def.fields, depth);
                        self.path.pop();
                        let mut object = Map::new();
                        object.insert(variant.clone(), inner?);
                        Ok(J::Object(object))
                    }
                    // a variant of a newer version, its fields skipped (SW-16): its name, as the decoder gives it
                    None if variant.starts_with("<unknown ")
                        && matches!(fields, FieldsValueOwned::Unit) =>
                    {
                        Ok(J::String(variant.clone()))
                    }
                    None => Err(self.err(ErrorKind::UnknownVariant {
                        enum_name: e.ident.clone(),
                        name: variant.clone(),
                        expected: e.variants.iter().map(|v| v.ident.clone()).collect(),
                    })),
                }
            }
            (ValueOwned::Option(v), TypeOwned::Option { some_ty }) => match v {
                Some(v) => self.value(v, some_ty, depth + 1),
                None => Ok(J::Null),
            },
            (ValueOwned::Result(r), TypeOwned::Result { ok_ty, err_ty }) => {
                let (name, v, ty) = match r {
                    Ok(v) => ("Ok", v, ok_ty),
                    Err(v) => ("Err", v, err_ty),
                };
                self.path.push(Step::Field(name.into()));
                let inner = self.value(v, ty, depth + 1);
                self.path.pop();
                let mut object = Map::new();
                object.insert(name.into(), inner?);
                Ok(J::Object(object))
            }
            (ValueOwned::Range(r), TypeOwned::Range(_)) => self.bounds(&r.start, &r.end),
            (ValueOwned::RangeInclusive(r), TypeOwned::RangeInclusive(_)) => {
                self.bounds(r.start(), r.end())
            }
            (ValueOwned::Opaque(_), _) => Err(self.err(ErrorKind::Unsupported(
                "a value of a type this host has no definition of has no JSON form".into(),
            ))),
            (value, ty) => Err(self.mismatch(value, ty)),
        }
    }

    fn bounds(&self, start: &N, end: &N) -> Result<J> {
        let both = numeric_json(start).zip(numeric_json(end));
        both.map(|(a, b)| J::Array(vec![a, b]))
            .ok_or_else(|| self.err(ErrorKind::Unsupported("a range of this number type".into())))
    }

    fn list<'t>(
        &mut self,
        items: &[ValueOwned],
        ty: impl Fn(usize) -> &'t TypeOwned,
        depth: usize,
    ) -> Result<J> {
        let mut out = Vec::with_capacity(items.len());
        for (idx, item) in items.iter().enumerate() {
            self.path.push(Step::Index(idx));
            out.push(self.value(item, ty(idx), depth + 1)?);
            self.path.pop();
        }
        Ok(J::Array(out))
    }

    fn fields(&mut self, values: &FieldsValueOwned, defs: &FieldsOwned, depth: usize) -> Result<J> {
        match (values, defs) {
            (FieldsValueOwned::Named(values), FieldsOwned::Named(defs)) => {
                let mut object = Map::new();
                // in the order of the definition, whatever order the value came in
                for def in value_fields(defs) {
                    let name = def.ident.clone().unwrap_or_default();
                    let Some((_, value)) = values.iter().find(|(n, _)| *n == name) else {
                        continue;
                    };
                    self.path.push(Step::Field(name.clone()));
                    object.insert(name, self.value(value, &def.ty, depth + 1)?);
                    self.path.pop();
                }
                Ok(J::Object(object))
            }
            (FieldsValueOwned::Unnamed(values), FieldsOwned::Unnamed(defs)) => {
                if values.len() != defs.len() {
                    return Err(self.err(ErrorKind::Mismatch(format!(
                        "{} fields where {} are defined",
                        values.len(),
                        defs.len()
                    ))));
                }
                if let ([value], [def]) = (values.as_slice(), defs.as_slice()) {
                    return self.value(value, &def.ty, depth + 1);
                }
                self.list(values, |i| &defs[i].ty, depth)
            }
            (FieldsValueOwned::Unit, _) => Ok(J::Null),
            _ => Err(self.err(ErrorKind::Mismatch(
                "fields do not match their definition".into(),
            ))),
        }
    }
}

/// The bytes of a list of `u8` values.
fn u8s(items: &[ValueOwned]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| match v {
            ValueOwned::Numeric(N::U8(b)) => Some(*b),
            _ => None,
        })
        .collect()
}

/// A quantity as text, a value that is no finite number as the plain number (`"NaN"`).
fn quantity_json(q: &DynQuantity) -> J {
    if q.origin.is_none() && q.value.is_float() && !q.value.to_f64().is_finite() {
        return numeric_json(&q.value).unwrap_or(J::Null);
    }
    J::String(q.to_string())
}

// ------------------------------------------------------------------------------------------------ JSON -> value

struct De<'a> {
    bundle: &'a ApiBundleOwned,
    path: JsonPath,
    raw: bool,
}

impl De<'_> {
    fn err(&self, kind: ErrorKind) -> JsonError {
        JsonError::new(&self.path, kind)
    }

    fn wrong(&self, j: &J, expected: &str) -> JsonError {
        self.err(ErrorKind::WrongType {
            expected: expected.into(),
            found: kind_of(j),
        })
    }

    fn range(&self, value: impl std::fmt::Display, base: &B) -> JsonError {
        self.err(ErrorKind::OutOfRange {
            value: value.to_string(),
            ty: base_name(base),
        })
    }

    fn fields_of(&self, args: &[ArgumentOwned]) -> Result<Vec<FieldOwned>> {
        ArgumentOwned::as_fields(args, self.bundle)
            .map_err(|e| self.err(ErrorKind::Model(format!("{e:#}"))))
    }

    fn value(&mut self, j: &J, ty: &TypeOwned, depth: usize) -> Result<ValueOwned> {
        if depth >= MAX_DECODE_DEPTH {
            return Err(self.err(ErrorKind::TooDeep(MAX_DECODE_DEPTH)));
        }
        let bundle = self.bundle;
        let ty = ty
            .get_in_line(bundle)
            .map_err(|e| self.err(ErrorKind::Model(format!("{e:#}"))))?;
        if !self.raw && quantity_type(ty, bundle) {
            return self.quantity(j, ty);
        }
        if !self.raw
            && let Some(text) = stored_text(j)
            && let Some(value) = stored_number(&text, ty, bundle)
        {
            return value.map_err(|e| self.err(ErrorKind::Quantity(e)));
        }
        let name = || type_name(ty, bundle);
        Ok(match ty {
            TypeOwned::Bool => {
                ValueOwned::Bool(j.as_bool().ok_or_else(|| self.wrong(j, "a boolean"))?)
            }
            TypeOwned::NumericAny(any) => ValueOwned::Numeric(self.numeric(j, numeric_base(any))?),
            TypeOwned::String => ValueOwned::String(
                j.as_str()
                    .ok_or_else(|| self.wrong(j, "a string"))?
                    .to_string(),
            ),
            TypeOwned::MaxLen { max_len, ty: inner } => {
                let value = self.value(j, inner, depth + 1)?;
                let len = match &value {
                    ValueOwned::String(s) => s.len(),
                    ValueOwned::Vec(items) | ValueOwned::Array(items) => items.len(),
                    _ => 0,
                };
                let max = max_len
                    .fixed()
                    .map_err(|e| self.err(ErrorKind::Model(e.to_string())))?;
                if len > max as usize {
                    return Err(self.err(ErrorKind::Length {
                        expected: format!("at most {max} elements (bytes for a string)"),
                        got: len,
                    }));
                }
                value
            }
            TypeOwned::Vec(inner) | TypeOwned::Series(inner) => {
                ValueOwned::Vec(self.items(j, inner, ty, None, depth)?)
            }
            TypeOwned::Array { len, ty: inner } => {
                let len = len
                    .fixed()
                    .map_err(|e| self.err(ErrorKind::Model(e.to_string())))?;
                ValueOwned::Array(self.items(j, inner, ty, Some(len as usize), depth)?)
            }
            TypeOwned::Tuple(types) => {
                let a = j
                    .as_array()
                    .ok_or_else(|| self.wrong(j, &format!("a list of {} elements", types.len())))?;
                if a.len() != types.len() {
                    return Err(self.err(ErrorKind::Length {
                        expected: format!("a list of {} elements", types.len()),
                        got: a.len(),
                    }));
                }
                let mut values = Vec::with_capacity(a.len());
                for (idx, (j, ty)) in a.iter().zip(types).enumerate() {
                    self.path.push(Step::Index(idx));
                    values.push(self.value(j, ty, depth + 1)?);
                    self.path.pop();
                }
                ValueOwned::Tuple(values)
            }
            TypeOwned::Struct(s) => ValueOwned::Struct {
                fields: self.fields(j, &s.fields, depth)?,
            },
            TypeOwned::Enum(e) => {
                let (variant, payload) = match j {
                    J::String(name) => (name.clone(), None),
                    J::Object(o) if o.len() == 1 => {
                        let (k, v) = o.iter().next().expect("one member");
                        (k.clone(), Some(v))
                    }
                    _ => {
                        return Err(self.wrong(
                            j,
                            &format!(
                                "a variant name or an object with one variant of {}",
                                e.ident
                            ),
                        ));
                    }
                };
                let Some(def) = e.variants.iter().find(|v| v.ident == variant) else {
                    // a variant from a newer version, as the dynamic decoder reads it
                    if variant.starts_with("<unknown ") && payload.is_none() {
                        return Ok(ValueOwned::Enum {
                            variant,
                            fields: FieldsValueOwned::Unit,
                        });
                    }
                    return Err(self.err(ErrorKind::UnknownVariant {
                        enum_name: e.ident.clone(),
                        name: variant,
                        expected: e.variants.iter().map(|v| v.ident.clone()).collect(),
                    }));
                };
                let fields = match (&def.fields, payload) {
                    (FieldsOwned::Unit, None) => FieldsValueOwned::Unit,
                    (FieldsOwned::Unit, Some(p)) if p.is_null() => FieldsValueOwned::Unit,
                    (_, Some(payload)) => {
                        self.path.push(Step::Field(variant.clone()));
                        let fields = self.fields(payload, &def.fields, depth);
                        self.path.pop();
                        fields?
                    }
                    (_, None) => {
                        return Err(self.err(ErrorKind::WrongType {
                            expected: format!(
                                "{{\"{variant}\": fields}}, {}::{variant} has fields",
                                e.ident
                            ),
                            found: "a string",
                        }));
                    }
                };
                ValueOwned::Enum { variant, fields }
            }
            TypeOwned::Option { some_ty } => ValueOwned::Option(match j {
                J::Null => None,
                j => Some(Box::new(self.value(j, some_ty, depth + 1)?)),
            }),
            TypeOwned::Result { ok_ty, err_ty } => {
                let o = j
                    .as_object()
                    .filter(|o| o.len() == 1)
                    .ok_or_else(|| self.wrong(j, "an object with `Ok` or `Err`"))?;
                let (key, v) = o.iter().next().expect("one member");
                let ty = match key.as_str() {
                    "Ok" => ok_ty,
                    "Err" => err_ty,
                    _ => {
                        return Err(self.err(ErrorKind::UnknownField {
                            name: key.clone(),
                            expected: vec!["Ok".into(), "Err".into()],
                        }));
                    }
                };
                self.path.push(Step::Field(key.clone()));
                let inner = self.value(v, ty, depth + 1);
                self.path.pop();
                let inner = Box::new(inner?);
                ValueOwned::Result(if key == "Ok" { Ok(inner) } else { Err(inner) })
            }
            TypeOwned::Box(inner) => self.value(j, inner, depth + 1)?,
            TypeOwned::Range(b) | TypeOwned::RangeInclusive(b) => {
                let a = j
                    .as_array()
                    .ok_or_else(|| self.wrong(j, "a list of [start, end]"))?;
                if a.len() != 2 {
                    return Err(self.err(ErrorKind::Length {
                        expected: "a list of [start, end]".into(),
                        got: a.len(),
                    }));
                }
                self.path.push(Step::Index(0));
                let start = self.numeric(&a[0], b);
                self.path.pop();
                self.path.push(Step::Index(1));
                let end = self.numeric(&a[1], b);
                self.path.pop();
                let (start, end) = (start?, end?);
                if matches!(ty, TypeOwned::Range(_)) {
                    ValueOwned::Range(start..end)
                } else {
                    ValueOwned::RangeInclusive(start..=end)
                }
            }
            TypeOwned::Flag
            | TypeOwned::OutOfLine { .. }
            | TypeOwned::Instance { .. }
            | TypeOwned::Param(_) => {
                return Err(self.err(ErrorKind::Unsupported(format!(
                    "type {} is not a value of the model",
                    name()
                ))));
            }
        })
    }

    fn items(
        &mut self,
        j: &J,
        inner: &TypeOwned,
        list_ty: &TypeOwned,
        len: Option<usize>,
        depth: usize,
    ) -> Result<Vec<ValueOwned>> {
        let bundle = self.bundle;
        if is_u8(inner, bundle)
            && !matches!(list_ty, TypeOwned::Series(_))
            && let J::String(text) = j
        {
            let bytes = unhex(text).ok_or_else(|| {
                self.err(ErrorKind::WrongType {
                    expected: "a hex string of whole bytes (`dead01`)".into(),
                    found: "a string",
                })
            })?;
            self.check_len(len, bytes.len())?;
            return Ok(bytes
                .into_iter()
                .map(|b| ValueOwned::Numeric(N::U8(b)))
                .collect());
        }
        let a = j.as_array().ok_or_else(|| {
            let what = match (len, is_u8(inner, bundle)) {
                (_, true) => "a hex string or a list of bytes".to_string(),
                (Some(len), _) => format!("a list of {len} elements"),
                (None, _) => "a list".to_string(),
            };
            self.wrong(j, &what)
        })?;
        self.check_len(len, a.len())?;
        let mut out = Vec::with_capacity(a.len());
        for (idx, j) in a.iter().enumerate() {
            self.path.push(Step::Index(idx));
            out.push(self.value(j, inner, depth + 1)?);
            self.path.pop();
        }
        Ok(out)
    }

    fn check_len(&self, expected: Option<usize>, got: usize) -> Result<()> {
        match expected {
            Some(len) if len != got => Err(self.err(ErrorKind::Length {
                expected: format!("{len} elements"),
                got,
            })),
            _ => Ok(()),
        }
    }

    fn fields(&mut self, j: &J, defs: &FieldsOwned, depth: usize) -> Result<FieldsValueOwned> {
        Ok(match defs {
            FieldsOwned::Named(defs) => {
                let o = j
                    .as_object()
                    .ok_or_else(|| self.wrong(j, "an object of fields"))?;
                let names = field_names(defs);
                if let Some(unknown) = o.keys().find(|k| !names.contains(k)) {
                    return Err(self.err(ErrorKind::UnknownField {
                        name: unknown.clone(),
                        expected: names,
                    }));
                }
                let fields: Vec<FieldOwned> = value_fields(defs).cloned().collect();
                FieldsValueOwned::Named(self.named_values_pairs(o, &fields, false, depth + 1)?)
            }
            FieldsOwned::Unnamed(defs) => {
                let items: Vec<&J> = match j {
                    J::Array(a) if defs.len() != 1 => a.iter().collect(),
                    j if defs.len() == 1 => vec![j],
                    _ => return Err(self.wrong(j, &format!("a list of {} fields", defs.len()))),
                };
                if items.len() != defs.len() {
                    return Err(self.err(ErrorKind::Length {
                        expected: format!("{} fields", defs.len()),
                        got: items.len(),
                    }));
                }
                let mut values = Vec::with_capacity(defs.len());
                for (idx, (j, def)) in items.into_iter().zip(defs).enumerate() {
                    if defs.len() != 1 {
                        self.path.push(Step::Index(idx));
                    }
                    values.push(self.value(j, &def.ty, depth + 1)?);
                    if defs.len() != 1 {
                        self.path.pop();
                    }
                }
                FieldsValueOwned::Unnamed(values)
            }
            FieldsOwned::Unit if j.is_null() => FieldsValueOwned::Unit,
            FieldsOwned::Unit => return Err(self.wrong(j, "null (a unit has no fields)")),
        })
    }

    /// The values of `fields` from `o`, in the order of the definition. `args`: the arguments of a call, where a
    /// trailing one with a default may be left out (arguments are positional on the wire).
    fn named_values(
        &mut self,
        o: &Map<String, J>,
        fields: &[FieldOwned],
        args: bool,
        depth: usize,
    ) -> Result<Vec<ValueOwned>> {
        Ok(self
            .named_values_pairs(o, fields, args, depth)?
            .into_iter()
            .map(|(_, v)| v)
            .collect())
    }

    fn named_values_pairs(
        &mut self,
        o: &Map<String, J>,
        fields: &[FieldOwned],
        args: bool,
        depth: usize,
    ) -> Result<Vec<(String, ValueOwned)>> {
        let name_of = |def: &FieldOwned| def.ident.clone().unwrap_or_default();
        let last_given = fields.iter().rposition(|d| o.contains_key(&name_of(d)));
        let mut values = vec![];
        for (idx, def) in fields.iter().enumerate() {
            let name = name_of(def);
            let Some(j) = o.get(&name) else {
                let is_option = matches!(
                    def.ty.get_in_line(self.bundle),
                    Ok(TypeOwned::Option { .. })
                );
                let after_given = last_given.is_none_or(|last| idx > last);
                if args && after_given && (def.default.is_some() || is_option) {
                    // a trailing argument with a default: the device reads it
                } else if is_option {
                    values.push((name, ValueOwned::Option(None)));
                } else if def.default.is_some() && !args {
                    // a struct's default is filled in during serialization
                } else {
                    return Err(self.err(ErrorKind::MissingField { name }));
                }
                continue;
            };
            self.path.push(Step::Field(name.clone()));
            let value = self.value(j, &def.ty, depth);
            self.path.pop();
            values.push((name, value?));
        }
        Ok(values)
    }

    fn numeric(&self, j: &J, base: &B) -> Result<N> {
        if matches!(base, B::F32 | B::F64) {
            let v = match j {
                J::Number(n) => n.as_f64(),
                J::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            }
            .ok_or_else(|| self.wrong(j, &format!("a number of {}", base_name(base))))?;
            return Ok(if matches!(base, B::F32) {
                N::F32(v as f32)
            } else {
                N::F64(v)
            });
        }
        if matches!(base, B::U128) {
            let v = match j {
                J::String(s) => s.trim().parse::<u128>().ok(),
                J::Number(n) => n.as_u64().map(u128::from),
                _ => None,
            }
            .ok_or_else(|| self.wrong(j, "an integer of u128 (a string beyond 2^53)"))?;
            return Ok(N::U128(v));
        }
        let v = int_of(j).ok_or_else(|| {
            self.wrong(
                j,
                &format!("an integer of {} (a string beyond 2^53)", base_name(base)),
            )
        })?;
        let range = || self.range(v, base);
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
            B::INib32 => N::INib32(INib32(int!(i32))),
            B::I64 => N::I64(int!(i64)),
            B::I128 => N::I128(v),
            // the value types the dynamic decoder gives bit-sized numbers
            B::UB(bits) => {
                let bits = bits.0;
                if v < 0 || (bits < 64 && v >= 1i128 << bits) {
                    return Err(range());
                }
                match bits {
                    0..=8 => N::U8(int!(u8)),
                    9..=16 => N::U16(int!(u16)),
                    17..=32 => N::U32(int!(u32)),
                    _ => N::U64(int!(u64)),
                }
            }
            B::IB(bits) => {
                let bits = bits.0;
                if bits == 0 || v < -(1i128 << (bits - 1)) || v >= 1i128 << (bits - 1) {
                    return Err(range());
                }
                match bits {
                    0..=8 => N::I8(int!(i8)),
                    9..=16 => N::I16(int!(i16)),
                    17..=32 => N::I32(int!(i32)),
                    _ => N::I64(int!(i64)),
                }
            }
            other => {
                return Err(self.err(ErrorKind::Unsupported(format!(
                    "number type {} has no JSON form",
                    base_name(other)
                ))));
            }
        })
    }

    /// A quantity: text with a unit, or the stored number.
    fn quantity(&self, j: &J, ty: &TypeOwned) -> Result<ValueOwned> {
        let bundle = self.bundle;
        let quantity = |e: String| self.err(ErrorKind::Quantity(e));
        let expected = || {
            self.wrong(
                j,
                &format!(
                    "a quantity as text with its unit (`5 ms`) or a number, for {}",
                    type_name(ty, bundle)
                ),
            )
        };
        let (J::Number(_) | J::String(_)) = j else {
            return Err(expected());
        };
        // a number alone: what the type stores, or for a `Quantity<U>` the number in the unit itself
        if let Some(text) = stored_text(j) {
            if let Some(value) = stored_number(&text, ty, bundle) {
                return value.map_err(quantity);
            }
            if let Some(unit) = unit_of(ty, bundle) {
                return parse_quantity_in(&text, Some(&unit), None)
                    .and_then(|q| q.to_value(ty, bundle))
                    .map_err(quantity);
            }
        }
        let text = match j {
            J::String(text) => text.as_str(),
            number => {
                return Err(quantity(format!(
                    "`{number}` is a number alone, which has no unit: write its symbol (`{number} V`)"
                )));
            }
        };
        if let Some(point) = parse_point_in(text, ty, bundle) {
            return point.map_err(quantity);
        }
        let fixed = fixed_of(ty, bundle);
        let coded = coded_of(ty, bundle);
        let unit = unit_of(ty, bundle)
            .or_else(|| fixed.as_ref().map(|f| f.unit.clone()))
            .or_else(|| coded.as_ref().map(|c| c.linear.unit.clone()));
        let scale = fixed
            .as_ref()
            .map(|f| f.prefix.clone())
            .or_else(|| coded.as_ref().map(|c| c.linear.prefix.clone()));
        if unit.is_none() && !is_any_quantity(ty, bundle) {
            return Err(expected());
        }
        parse_quantity_in(text, unit.as_ref(), scale.as_ref())
            .and_then(|q| q.to_value(ty, bundle))
            .map_err(quantity)
    }
}

/// A JSON number or a string that is only a number, as the text the stored-number parsers read.
fn stored_text(j: &J) -> Option<String> {
    match j {
        J::Number(n) => Some(n.to_string()),
        J::String(s) => {
            let t = s.trim();
            let plain = t.parse::<f64>().is_ok() && !t.is_empty();
            plain.then(|| t.to_string())
        }
        _ => None,
    }
}

/// An integer of JSON: a number, a number that is whole (`5.0`), or a decimal string.
fn int_of(j: &J) -> Option<i128> {
    match j {
        J::Number(n) => n
            .as_i64()
            .map(i128::from)
            .or(n.as_u64().map(i128::from))
            .or_else(|| {
                let x = n.as_f64()?;
                (x.fract() == 0.0 && x.abs() <= EXACT_INT as f64).then_some(x as i128)
            }),
        J::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex(&[0xde, 0xad, 0x01]), "dead01");
        assert_eq!(unhex("de ad_01"), Some(vec![0xde, 0xad, 0x01]));
        assert_eq!(unhex("0xDEAD"), Some(vec![0xde, 0xad]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn big_integers_are_strings() {
        assert_eq!(int_json(5), serde_json::json!(5));
        assert_eq!(int_json(EXACT_INT), serde_json::json!(9007199254740992i64));
        assert_eq!(
            int_json(EXACT_INT + 1),
            serde_json::json!("9007199254740993")
        );
        assert_eq!(
            int_json(-EXACT_INT - 1),
            serde_json::json!("-9007199254740993")
        );
    }

    #[test]
    fn floats() {
        assert_eq!(numeric_json(&N::F32(0.1)), Some(serde_json::json!(0.1)));
        assert_eq!(
            numeric_json(&N::F64(f64::NAN)),
            Some(serde_json::json!("NaN"))
        );
        assert_eq!(
            numeric_json(&N::F64(f64::NEG_INFINITY)),
            Some(serde_json::json!("-inf"))
        );
    }
}
