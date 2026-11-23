//! JSON Schema (draft 2020-12) of the JSON forms of the codec (INT-15): what [crate::Codec] accepts and writes, for
//! every argument list, result, property and stream item of an API model.
//!
//! | Type | Schema |
//! |---|---|
//! | `bool` | `{"type": "boolean"}` |
//! | integers up to ±2⁵³ | `{"type": "integer", "minimum": .., "maximum": ..}` of the type's range and bounds |
//! | wider integers | an integer within ±2⁵³ or a decimal string (`pattern`), the range in `description` and `x-range` |
//! | `f32`, `f64` | a number or `"NaN"`, `"inf"`, `"-inf"` |
//! | `String`, `MaxLen<String, N>` | `{"type": "string", "maxLength": N}` |
//! | `Vec<u8>`, `[u8; N]` | a hex string or a list of bytes |
//! | `Vec<T>`, `Series<T>`, `[T; N]` | `{"type": "array", "items": T}`, with `minItems`, `maxItems` |
//! | tuple, tuple struct | `prefixItems`, a single field one is its value |
//! | struct | `{"type": "object", "properties": .., "required": .., "additionalProperties": false}` |
//! | enum | `oneOf` the variants: `{"const": "Unit"}` or `{"Variant": fields}` |
//! | `Option<T>` | `anyOf` `null` and `T` |
//! | `Result<T, E>` | `oneOf` `{"Ok": T}` and `{"Err": E}` |
//! | `ww_si` quantity, `Fixed`, `Point`, `AnyQuantity`, `Coded` | `{"type": "string"}` with `x-unit`, `x-min`, `x-max` and a description |
//! | `Index`, `IndexFrom1`, `Seq` | an integer |
//!
//! Struct and enum definitions are `$defs` referenced with `$ref` (also what makes a recursive type finite); doc
//! comments are `description`s. Every schema is self-contained: it carries the `$defs` it refers to. A schema may
//! be stricter than the codec in forms the codec is lenient about (a whole float for an integer, a number for a
//! quantity), never looser.

use serde_json::{Map, Value as J, json};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType as B, NumericValue};
use ww_quantity::{coded_of, fixed_of, is_any_quantity, point_of, unit_of};
use ww_self::units::{Bound, NumBounds, Wrapper, wrapper_of};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ArgumentOwned, FieldOwned, FieldsOwned,
    MAX_DECODE_DEPTH, TypeOwned,
};

use crate::doc::EXACT_INT;
use crate::value::{base_name, is_u8};

/// The dialect every schema declares.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Why a type has no schema: it doesn't resolve in the model, or has no JSON form.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SchemaError {
    /// The model doesn't resolve the type (a missing definition).
    #[error("{0}")]
    Model(String),
    /// A type the JSON form doesn't cover.
    #[error("{0}")]
    Unsupported(String),
    /// Nested deeper than the dynamic decoder reads.
    #[error("nested deeper than {0} levels")]
    TooDeep(usize),
}

type Result<T> = std::result::Result<T, SchemaError>;

/// The schemas of one API item: its arguments as an object, the result of a call, the value of a property or the item
/// of a stream. Every one is a complete schema document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemSchemas {
    /// The arguments of a method or stream function: an object by name.
    pub arguments: Option<J>,
    /// The result of a method or stream function: its return type, `None` for a call that returns nothing.
    pub result: Option<J>,
    /// The value of a property, or the item of a stream, sink or stream function.
    pub value: Option<J>,
    /// The error of a write to a property.
    pub write_error: Option<J>,
    /// The doc comment of the item, joined.
    pub description: Option<String>,
}

/// The schemas of `item`, `None` for a resource that holds none (a trait, a key companion's operations).
pub fn item_schemas(item: &ApiItemOwned, bundle: &ApiBundleOwned) -> Result<ItemSchemas> {
    let mut schemas = ItemSchemas {
        description: docs(&item.docs),
        ..ItemSchemas::default()
    };
    let ty = |ty: &TypeOwned| type_schema(ty, bundle);
    match &item.kind {
        ApiItemKindOwned::Method {
            args, return_ty, ..
        } => {
            schemas.arguments = Some(args_schema(args, bundle)?);
            schemas.result = return_ty.as_ref().map(ty).transpose()?;
        }
        ApiItemKindOwned::Property {
            ty: value,
            write_err_ty,
            ..
        } => {
            schemas.value = Some(ty(value)?);
            schemas.write_error = write_err_ty.as_ref().map(ty).transpose()?;
        }
        ApiItemKindOwned::Stream { ty: value, .. }
        | ApiItemKindOwned::FlowSink { ty: value, .. } => {
            schemas.value = Some(ty(value)?);
        }
        ApiItemKindOwned::StreamFn {
            ty: value,
            args,
            return_ty,
            ..
        } => {
            schemas.arguments = Some(args_schema(args, bundle)?);
            schemas.result = return_ty.as_ref().map(ty).transpose()?;
            schemas.value = Some(ty(value)?);
        }
        ApiItemKindOwned::Keys { key, .. } => schemas.value = Some(ty(key)?),
        ApiItemKindOwned::Trait { .. } => {}
    }
    Ok(schemas)
}

/// The schema of a value of type `ty`, a complete document with the `$defs` it refers to.
pub fn type_schema(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<J> {
    let mut b = Builder::new(bundle);
    let mut schema = b.ty(ty, 0)?;
    Ok(b.finish(&mut schema))
}

/// The schema of the arguments of a call: one object by name, with the required ones listed (those without a default
/// and not an `Option`), a `#[flatten]` argument's fields in its place.
pub fn args_schema(args: &[ArgumentOwned], bundle: &ApiBundleOwned) -> Result<J> {
    let fields =
        ArgumentOwned::as_fields(args, bundle).map_err(|e| SchemaError::Model(format!("{e:#}")))?;
    let mut b = Builder::new(bundle);
    let mut schema = b.object(&fields.iter().collect::<Vec<_>>(), 0)?;
    Ok(b.finish(&mut schema))
}

/// Joined doc lines, `None` for no docs.
fn docs(lines: &[String]) -> Option<String> {
    let text = lines.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

struct Builder<'a> {
    bundle: &'a ApiBundleOwned,
    defs: Map<String, J>,
    /// `type_idx` -> its key in `$defs`.
    keys: std::collections::HashMap<u32, String>,
}

impl<'a> Builder<'a> {
    fn new(bundle: &'a ApiBundleOwned) -> Self {
        Builder {
            bundle,
            defs: Map::new(),
            keys: Default::default(),
        }
    }

    /// `schema` as a document: the dialect and the `$defs`.
    fn finish(&mut self, schema: &mut J) -> J {
        let mut doc = Map::new();
        doc.insert("$schema".into(), J::String(DIALECT.into()));
        if let J::Object(o) = schema {
            for (k, v) in std::mem::take(o) {
                doc.insert(k, v);
            }
        }
        if !self.defs.is_empty() {
            doc.insert("$defs".into(), J::Object(std::mem::take(&mut self.defs)));
        }
        J::Object(doc)
    }

    fn model(&self, e: impl std::fmt::Display) -> SchemaError {
        SchemaError::Model(e.to_string())
    }

    /// A `$ref` to the definition of the struct or enum at `type_idx`, built first time.
    fn reference(&mut self, type_idx: u32, depth: usize) -> Result<J> {
        if let Some(key) = self.keys.get(&type_idx) {
            return Ok(json!({"$ref": format!("#/$defs/{key}")}));
        }
        let (ty, _) = self
            .bundle
            .get_ty(type_idx)
            .map_err(|e| self.model(format!("{e:#}")))?;
        let name = match ty {
            TypeOwned::Struct(s) => s.ident.clone(),
            TypeOwned::Enum(e) => e.ident.clone(),
            other => other
                .human_name(false, self.bundle)
                .unwrap_or_else(|_| "Type".into()),
        };
        // a readable name; a second definition of the same name (another crate's, another instance) gets its index
        let clean: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let key = if self.keys.values().any(|k| *k == clean) {
            format!("{clean}_{type_idx}")
        } else {
            clean
        };
        self.keys.insert(type_idx, key.clone());
        // reserve the key first: a recursive type refers to itself while it is built
        self.defs.insert(key.clone(), J::Null);
        let mut schema = self.definition(ty, depth)?;
        if let (J::Object(o), Some(title)) = (&mut schema, Some(&name)) {
            o.insert("title".into(), J::String(title.clone()));
        }
        self.defs.insert(key.clone(), schema);
        Ok(json!({"$ref": format!("#/$defs/{key}")}))
    }

    fn ty(&mut self, ty: &TypeOwned, depth: usize) -> Result<J> {
        if depth >= MAX_DECODE_DEPTH {
            return Err(SchemaError::TooDeep(MAX_DECODE_DEPTH));
        }
        // a struct or enum of the model is a definition; the quantity wrappers are structs but not defs
        if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = ty {
            let resolved = ty
                .get_in_line(self.bundle)
                .map_err(|e| self.model(format!("{e:#}")))?;
            if let Some(schema) = self.special(resolved)? {
                return Ok(schema);
            }
            if matches!(resolved, TypeOwned::Struct(_) | TypeOwned::Enum(_)) {
                return self.reference(type_idx.0, depth + 1);
            }
        }
        let ty = ty
            .get_in_line(self.bundle)
            .map_err(|e| self.model(format!("{e:#}")))?;
        if let Some(schema) = self.special(ty)? {
            return Ok(schema);
        }
        self.definition(ty, depth)
    }

    /// Quantities and counts: wrappers of numbers with their own text.
    fn special(&mut self, ty: &TypeOwned) -> Result<Option<J>> {
        let bundle = self.bundle;
        if let Some((origin, coord)) = point_of(ty, bundle) {
            let symbol = origin.symbol.clone();
            let coord = fixed_of(&coord, bundle);
            let unit = coord.as_ref().map(|c| c.unit.symbol()).unwrap_or_default();
            let description = format!(
                "A position in {}, written as text with its symbol (`25 {symbol}`); also accepted in {}, converted exactly (`298.15 {unit}`). A plain number is not accepted.",
                origin.id.name,
                unit_text(&unit)
            );
            return Ok(Some(json!({
                "type": "string",
                "description": description,
                "x-quantity": "point",
                "x-origin": origin.id.name,
                "x-unit": symbol,
            })));
        }
        if let Some(coded) = coded_of(ty, bundle) {
            let unit = coded.linear.symbol();
            return Ok(Some(json!({
                "type": "string",
                "description": format!("The linear value of a code, written as text with its unit (in {}); a plain number is the code itself.", unit_text(&unit)),
                "x-quantity": "coded",
                "x-unit": unit,
            })));
        }
        if let Some(fixed) = fixed_of(ty, bundle) {
            let symbol = fixed.symbol();
            let mut schema = Map::new();
            schema.insert("type".into(), "string".into());
            let bounds = &fixed.storage.bounds;
            let show = |b: &Bound| format!("{b} {symbol}");
            let range = match (&bounds.min, &bounds.max) {
                (None, None) => String::new(),
                (min, max) => format!(
                    ", from {} to {}",
                    min.as_ref()
                        .map_or("the lowest the storage holds".into(), show),
                    max.as_ref()
                        .map_or("the highest the storage holds".into(), show)
                ),
            };
            let description = format!(
                "A quantity of {}, written as text with its unit: `5 {symbol}`, in whole {symbol} (stored as {}){range}. Any unit and prefix that converts exactly is accepted too. A plain number is taken as {symbol}.",
                fixed.unit.name(),
                fixed.storage.name(),
            );
            if let Some(min) = &bounds.min {
                schema.insert("x-min".into(), show(min).into());
            }
            if let Some(max) = &bounds.max {
                schema.insert("x-max".into(), show(max).into());
            }
            schema.insert("description".into(), description.into());
            schema.insert("x-quantity".into(), "fixed".into());
            schema.insert("x-unit".into(), symbol.into());
            schema.insert("x-storage".into(), fixed.storage.name().into());
            return Ok(Some(J::Object(schema)));
        }
        if let Some(unit) = unit_of(ty, bundle) {
            let symbol = unit.symbol();
            return Ok(Some(json!({
                "type": "string",
                "description": format!("A quantity in {} ({symbol}), written as text with its unit and any prefix: `5 {symbol}`. A plain number is in {symbol} itself.", unit.name()),
                "x-quantity": "quantity",
                "x-unit": symbol,
            })));
        }
        if is_any_quantity(ty, bundle) {
            return Ok(Some(json!({
                "type": "string",
                "description": "A quantity with the unit it is in, written as text with its unit symbol: `3.3 V`, `5 mA`. A plain number has no unit and is not accepted.",
                "x-quantity": "any",
                "x-unit": "any",
            })));
        }
        match wrapper_of(ty, bundle) {
            Some(Wrapper::Index {
                storage, bounds, ..
            }) => Ok(Some(self.count(
                storage,
                Some(bounds),
                0,
                "A position in a table, counted from 0",
            )?)),
            Some(Wrapper::IndexFrom1 {
                storage, bounds, ..
            }) => Ok(Some(self.count(
                storage,
                Some(bounds),
                1,
                "A number counted from 1, never 0",
            )?)),
            Some(Wrapper::Seq { storage, .. }) => Ok(Some(self.count(
                storage,
                None,
                0,
                "A sequence number, wrapping at the top of its storage",
            )?)),
            _ => Ok(None),
        }
    }

    fn count(
        &mut self,
        storage: &TypeOwned,
        bounds: Option<&TypeOwned>,
        floor: i128,
        what: &str,
    ) -> Result<J> {
        let base = ww_self::units::storage_of(storage, self.bundle)
            .map_err(|e| self.model(format!("{e:#}")))?;
        let bounds = match bounds {
            Some(b) => ww_self::units::bounds_of(b, self.bundle)
                .map_err(|e| self.model(format!("{e:#}")))?,
            None => NumBounds::default(),
        };
        let (mut min, mut max) = int_range(&base).unwrap_or((0, i128::MAX));
        min = min.max(floor);
        if let Some(Bound::Int(m)) = bounds.min {
            min = min.max(m);
        }
        if let Some(Bound::Int(m)) = bounds.max {
            max = max.min(m);
        }
        let mut schema = int_schema(min, max);
        if let J::Object(o) = &mut schema {
            o.insert("description".into(), format!("{what}, as a number.").into());
        }
        Ok(schema)
    }

    /// The schema of a resolved type that is no wrapper: a definition when it is a struct or enum.
    fn definition(&mut self, ty: &TypeOwned, depth: usize) -> Result<J> {
        let depth = depth + 1;
        Ok(match ty {
            TypeOwned::Bool => json!({"type": "boolean"}),
            TypeOwned::NumericAny(any) => numeric_schema(any),
            TypeOwned::String => json!({"type": "string"}),
            TypeOwned::MaxLen { max_len, ty: inner } => {
                let max = max_len.fixed().map_err(|e| self.model(e))?;
                let mut schema = self.ty(inner, depth)?;
                limit(&mut schema, max as usize);
                schema
            }
            TypeOwned::Vec(inner) | TypeOwned::Series(inner) => {
                self.list(inner, None, matches!(ty, TypeOwned::Series(_)), depth)?
            }
            TypeOwned::Array { len, ty: inner } => {
                let len = len.fixed().map_err(|e| self.model(e))?;
                self.list(inner, Some(len as usize), false, depth)?
            }
            TypeOwned::Tuple(types) => {
                let items = types
                    .iter()
                    .map(|t| self.ty(t, depth))
                    .collect::<Result<Vec<_>>>()?;
                tuple(items)
            }
            TypeOwned::Struct(s) => {
                let mut schema = match &s.fields {
                    FieldsOwned::Named(fields) => {
                        self.object(&fields.iter().collect::<Vec<_>>(), depth)?
                    }
                    FieldsOwned::Unnamed(fields) => self.unnamed(fields, depth)?,
                    FieldsOwned::Unit => json!({"type": "null"}),
                };
                describe(&mut schema, docs(&s.docs));
                schema
            }
            TypeOwned::Enum(e) => {
                let mut branches = vec![];
                for variant in &e.variants {
                    let mut branch = match &variant.fields {
                        FieldsOwned::Unit => json!({"const": variant.ident}),
                        fields => {
                            let inner = match fields {
                                FieldsOwned::Named(f) => {
                                    self.object(&f.iter().collect::<Vec<_>>(), depth)?
                                }
                                FieldsOwned::Unnamed(f) => self.unnamed(f, depth)?,
                                FieldsOwned::Unit => unreachable!("matched above"),
                            };
                            json!({
                                "type": "object",
                                "properties": {variant.ident.clone(): inner},
                                "required": [variant.ident],
                                "additionalProperties": false,
                            })
                        }
                    };
                    describe(&mut branch, docs(&variant.docs));
                    branches.push(branch);
                }
                // a variant of a newer version, as a device sends it
                if !matches!(e.future_version, Some(ww_self::FutureVersionOwned::Off)) {
                    branches.push(json!({
                        "type": "string",
                        "pattern": "^<unknown [0-9]+>$",
                        "description": "A variant from a newer version of the API, as a device sends it.",
                    }));
                }
                let mut schema = json!({"oneOf": branches});
                describe(&mut schema, docs(&e.docs));
                schema
            }
            TypeOwned::Option { some_ty } => {
                let inner = self.ty(some_ty, depth)?;
                json!({"anyOf": [{"type": "null"}, inner]})
            }
            TypeOwned::Result { ok_ty, err_ty } => {
                let ok = self.ty(ok_ty, depth)?;
                let err = self.ty(err_ty, depth)?;
                json!({"oneOf": [
                    {"type": "object", "properties": {"Ok": ok}, "required": ["Ok"], "additionalProperties": false},
                    {"type": "object", "properties": {"Err": err}, "required": ["Err"], "additionalProperties": false},
                ]})
            }
            TypeOwned::Box(inner) => self.ty(inner, depth)?,
            TypeOwned::Range(base) | TypeOwned::RangeInclusive(base) => {
                let n = numeric_schema(&NumericAnyTypeOwned::Base((**base).clone()));
                tuple(vec![n.clone(), n])
            }
            TypeOwned::Flag
            | TypeOwned::OutOfLine { .. }
            | TypeOwned::Instance { .. }
            | TypeOwned::Param(_) => {
                return Err(SchemaError::Unsupported(format!(
                    "type {} is not a value of the model",
                    ty.human_name(false, self.bundle).unwrap_or_default()
                )));
            }
        })
    }

    fn list(
        &mut self,
        inner: &TypeOwned,
        len: Option<usize>,
        series: bool,
        depth: usize,
    ) -> Result<J> {
        let item = self.ty(inner, depth)?;
        let mut schema = if is_u8(inner, self.bundle) && !series {
            let (pattern, items) = (
                "^([0-9a-fA-F]{2})*$",
                json!({"type": "integer", "minimum": 0, "maximum": 255}),
            );
            let mut hex = json!({"type": "string", "pattern": pattern});
            let mut list = json!({"type": "array", "items": items});
            if let Some(len) = len {
                hex["minLength"] = (2 * len).into();
                hex["maxLength"] = (2 * len).into();
                list["minItems"] = len.into();
                list["maxItems"] = len.into();
            }
            json!({
                "anyOf": [hex, list],
                "description": "Bytes: a hex string (`00ff10`), or a list of numbers 0 to 255.",
            })
        } else {
            let mut list = json!({"type": "array", "items": item});
            if let Some(len) = len {
                list["minItems"] = len.into();
                list["maxItems"] = len.into();
            }
            list
        };
        if let (Some(len), true) = (len, schema.get("anyOf").is_some()) {
            schema["description"] = format!(
                "{len} bytes: a hex string of {} digits, or a list of {len} numbers 0 to 255.",
                2 * len
            )
            .into();
        }
        Ok(schema)
    }

    /// An object of named fields: `required` are those without a default and not an `Option`.
    fn object(&mut self, fields: &[&FieldOwned], depth: usize) -> Result<J> {
        let mut properties = Map::new();
        let mut required = vec![];
        for field in fields.iter().filter(|f| !f.is_flag()) {
            let name = field.ident.clone().unwrap_or_default();
            let mut schema = self.ty(&field.ty, depth)?;
            describe(&mut schema, docs(&field.docs));
            if field.default.is_none()
                && !matches!(
                    field.ty.get_in_line(self.bundle),
                    Ok(TypeOwned::Option { .. })
                )
            {
                required.push(J::String(name.clone()));
            }
            properties.insert(name, schema);
        }
        Ok(json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        }))
    }

    /// The fields of a tuple struct or variant: the value itself for one, a list for more.
    fn unnamed(&mut self, fields: &[FieldOwned], depth: usize) -> Result<J> {
        let mut items = vec![];
        for field in fields {
            let mut schema = self.ty(&field.ty, depth)?;
            describe(&mut schema, docs(&field.docs));
            items.push(schema);
        }
        Ok(match items.len() {
            1 => items.remove(0),
            _ => tuple(items),
        })
    }
}

/// `schema`'s description, `text` before what it says.
fn describe(schema: &mut J, text: Option<String>) {
    let (Some(text), J::Object(o)) = (text, schema) else {
        return;
    };
    let text = match o.get("description").and_then(J::as_str) {
        Some(existing) => format!("{text} {existing}"),
        None => text,
    };
    o.insert("description".into(), J::String(text));
}

/// A list of `items` of fixed length, each at its place.
fn tuple(items: Vec<J>) -> J {
    json!({
        "type": "array",
        "prefixItems": items,
        "minItems": items.len(),
        "maxItems": items.len(),
        "items": false,
    })
}

/// `schema` limited to at most `max` elements (bytes for a string): `maxLength` / `maxItems`, nested in the bytes form.
fn limit(schema: &mut J, max: usize) {
    let J::Object(o) = schema else {
        return;
    };
    match o.get("type").and_then(J::as_str) {
        Some("string") => {
            // `maxLength` counts characters: a string of at most `max` bytes has at most `max` of them
            o.insert("maxLength".into(), max.into());
            let text = format!("At most {max} bytes of UTF-8.");
            let d = o
                .get("description")
                .and_then(J::as_str)
                .map_or(text.clone(), |d| format!("{d} {text}"));
            o.insert("description".into(), d.into());
        }
        Some("array") => {
            o.insert("maxItems".into(), max.into());
        }
        _ => {
            if let Some(J::Array(branches)) = o.get_mut("anyOf") {
                // bytes: a hex string and a list
                for branch in branches {
                    match branch.get("type").and_then(J::as_str) {
                        Some("string") => branch["maxLength"] = (2 * max).into(),
                        Some("array") => branch["maxItems"] = max.into(),
                        _ => {}
                    }
                }
                o.insert(
                    "description".into(),
                    format!("At most {max} bytes: a hex string of at most {} digits, or a list of at most {max} numbers 0 to 255.", 2 * max).into(),
                );
            }
        }
    }
}

/// The unit's words for descriptions: `ms` as is.
fn unit_text(symbol: &str) -> String {
    symbol.to_string()
}

fn numeric_schema(any: &NumericAnyTypeOwned) -> J {
    let base = match any {
        NumericAnyTypeOwned::Base(base)
        | NumericAnyTypeOwned::SubType { base, .. }
        | NumericAnyTypeOwned::ShiftScale { base, .. } => base,
    };
    if matches!(base, B::F32 | B::F64) {
        return json!({
            "anyOf": [
                {"type": "number"},
                {"enum": ["NaN", "inf", "-inf"]},
            ],
            "description": format!("A {}: a number, or \"NaN\", \"inf\", \"-inf\" as a string.", base_name(base)),
        });
    }
    let Some((mut min, mut max)) = int_range(base) else {
        return json!({"type": "integer"});
    };
    if let NumericAnyTypeOwned::SubType {
        kind: ww_numeric::SubTypeKindOwned::ValidRange { start, end },
        ..
    } = any
        && let (Some(a), Some(b)) = (to_int(start), to_int(end))
    {
        min = min.max(a);
        max = max.min(b);
    }
    int_schema(min, max)
}

fn to_int(n: &NumericValue) -> Option<i128> {
    n.to_i128()
}

/// The range of an integer type, `None` for a float or a type of no fixed width.
fn int_range(base: &B) -> Option<(i128, i128)> {
    Some(match base {
        B::Nibble => (0, 15),
        B::U8 => (0, u8::MAX.into()),
        B::U16 => (0, u16::MAX.into()),
        B::U32 | B::UNib32 | B::UVlq32 => (0, u32::MAX.into()),
        B::U64 | B::UVlq64 => (0, u64::MAX.into()),
        B::U128 => (0, i128::MAX),
        B::I8 => (i8::MIN.into(), i8::MAX.into()),
        B::I16 => (i16::MIN.into(), i16::MAX.into()),
        B::I32 | B::INib32 => (i32::MIN.into(), i32::MAX.into()),
        B::I64 => (i64::MIN.into(), i64::MAX.into()),
        B::I128 => (i128::MIN, i128::MAX),
        B::UB(bits) => (0, (1i128 << bits.0.min(64)) - 1),
        B::IB(bits) => {
            let bits = bits.0.clamp(1, 64);
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        }
        _ => return None,
    })
}

/// An integer in `min..=max`: a JSON number up to ±2⁵³, beyond that also a decimal string.
fn int_schema(min: i128, max: i128) -> J {
    let to_json = |i: i128| -> J {
        i64::try_from(i)
            .map(J::from)
            .or_else(|_| u64::try_from(i).map(J::from))
            .unwrap_or_else(|_| J::String(i.to_string()))
    };
    if min >= -EXACT_INT && max <= EXACT_INT {
        return json!({"type": "integer", "minimum": min as i64, "maximum": max as i64});
    }
    // JSON numbers beyond 2^53 are not exact in most parsers: the number form stops there, a string goes on
    let (lo, hi) = (min.max(-EXACT_INT), max.min(EXACT_INT));
    let _ = to_json;
    json!({
        "anyOf": [
            {"type": "integer", "minimum": lo as i64, "maximum": hi as i64},
            {"type": "string", "pattern": "^-?[0-9]+$"},
        ],
        "description": format!("An integer from {min} to {max}: a number up to ±2^53, a decimal string beyond."),
        "x-range": [min.to_string(), max.to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_ranges() {
        assert_eq!(
            int_schema(0, 255),
            json!({"type": "integer", "minimum": 0, "maximum": 255})
        );
        let wide = int_schema(0, u64::MAX.into());
        assert_eq!(wide["anyOf"][1]["pattern"], "^-?[0-9]+$");
        assert_eq!(wide["anyOf"][0]["maximum"], 9007199254740992i64);
        assert_eq!(wide["x-range"][1], "18446744073709551615");
    }

    #[test]
    fn docs_are_joined() {
        assert_eq!(
            docs(&[" A supply rail,".into(), " 0 V to 3.3 V".into()]),
            Some("A supply rail, 0 V to 3.3 V".into())
        );
        assert_eq!(docs(&[]), None);
    }

    #[test]
    fn tuple_has_fixed_length() {
        let t = tuple(vec![json!({"type": "integer"}), json!({"type": "boolean"})]);
        assert_eq!(t["minItems"], 2);
        assert_eq!(t["items"], false);
    }
}
