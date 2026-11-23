//! The TOML form (INT-18): [ValueOwned] to and from [toml_edit] items, by the rules of the JSON form, for files
//! such as settings levels.
//!
//! | WireWeaver                     | TOML                                                                       |
//! |--------------------------------|----------------------------------------------------------------------------|
//! | integers                       | integer; beyond `i64` (and `u128`/`i128` that large) a decimal string      |
//! | `f32`, `f64`                   | float, `nan` and `inf` included                                            |
//! | struct with named fields       | table, keys in the order of the definition; a nested struct a sub-table    |
//! | unit struct, unit fields       | empty table `{}`                                                           |
//! | enum unit variant              | snake_case string, `"fast_start"` (the defined name is read too)           |
//! | enum variant with fields       | `{fast_start = fields}`                                                    |
//! | `Option<T>`                    | the key left out for `None`; `None` in a list has no form                  |
//! | quantities                     | text with the unit, `"0.3 s"`, `"16 kHz"`                                  |
//! | everything else                | as in JSON: lists, tuples, `Result`, hex bytes, ranges                     |
//!
//! [Codec::decode_onto] reads a level onto a base value, keeping every key the table doesn't name;
//! [Codec::write_onto] is its inverse for write-back, editing a parsed [toml_edit::DocumentMut] in place so
//! untouched lines and comments stay.
//!
//! Decoding onto a base value also reads two markers (Q22/Q23), a one-key inline table in place of a field's
//! ordinary value: `{ unset_to_none = true }` sets an `Option` field to `None` over any base value, where a left-out
//! key would keep it; `{ use_parent = true }` keeps the base's value outright, the same as leaving the key out, for
//! a level that wants to say so. `write_onto` writes the first when it must (`None` over a base that has a value,
//! where a left-out key would read back wrong) and never writes the second, but leaves an existing line of either
//! alone when the value still matches. No struct field may be named `unset_to_none` or `use_parent`, so a one-key
//! table of either name is never read as a struct's own value.

use crate::doc::{self, Doc, Form, Marker};
use crate::error::{ErrorKind, JsonError, JsonPath, Step};
use crate::value::{Codec, quantity_type, value_fields};
use toml_edit::{Array, InlineTable, Item, Table, TableLike, Value as T};
use ww_self::{
    ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, MAX_DECODE_DEPTH, TypeOwned,
    ValueOwned,
};

type Result<V> = std::result::Result<V, JsonError>;

/// The field definitions of a struct and its named values.
type StructParts<'v> = (&'v [FieldOwned], &'v [(String, ValueOwned)]);

impl<'a> Codec<'a> {
    /// `value` of type `ty` as a TOML item: a struct is a [Table], anything else a value.
    pub fn to_toml(&self, value: &ValueOwned, ty: &TypeOwned) -> Result<Item> {
        let path = self.path();
        let doc = self.encode_doc(value, ty, Form::Toml, &path)?;
        to_item(doc, &mut path.clone())
    }

    /// The TOML item `item` as a value of type `ty`.
    pub fn from_toml(&self, item: &Item, ty: &TypeOwned) -> Result<ValueOwned> {
        let mut path = self.path();
        let doc = item_doc(item, &mut path)?;
        self.decode_doc(&doc, ty, Form::Toml, &path)
    }

    /// `base`, a value of struct type `ty`, with the keys `table` holds set on it; every other field keeps its
    /// value from `base`. A key naming a nested struct sets only the keys its own table holds; any other key
    /// replaces the field. A caller's built-in level is the type's `Default` encoded to a value, and each level's
    /// table goes onto it in order. `table` is often a parsed [toml_edit::DocumentMut] (`&doc`).
    pub fn decode_onto(
        &self,
        base: &ValueOwned,
        table: &Table,
        ty: &TypeOwned,
    ) -> Result<ValueOwned> {
        let doc = table_doc(table, &mut self.path())?;
        self.onto_doc(base, &doc, ty, Form::Toml)
    }

    /// Edits `table` so that [Codec::decode_onto] of it onto `base` gives `value`, changing only what must change:
    /// a key whose value already decodes to the wanted one stays as written, comments and formatting with it, a
    /// changed key gets the new value with the old one's comment, and a field equal to `base` that the table doesn't
    /// name is not added. Nested structs are edited key by key. `table` is often a parsed
    /// [toml_edit::DocumentMut] (`&mut doc`).
    ///
    /// An `Option` field that is `None` where `base` has a value is written as `{ unset_to_none = true }`: TOML
    /// has no null, and a left-out key would read back as `base`'s value (Q22).
    pub fn write_onto(
        &self,
        table: &mut Table,
        base: &ValueOwned,
        value: &ValueOwned,
        ty: &TypeOwned,
    ) -> Result<()> {
        let mut path = self.path();
        self.write_struct(table, Some(base), value, ty, &mut path, 0)
    }

    fn write_struct(
        &self,
        table: &mut dyn TableLike,
        base: Option<&ValueOwned>,
        value: &ValueOwned,
        ty: &TypeOwned,
        path: &mut JsonPath,
        depth: usize,
    ) -> Result<()> {
        if depth >= MAX_DECODE_DEPTH {
            return Err(JsonError::new(path, ErrorKind::TooDeep(MAX_DECODE_DEPTH)));
        }
        let Some((defs, values)) = self.struct_parts(value, ty) else {
            return Err(JsonError::new(
                path,
                ErrorKind::Mismatch(format!("{value:?} is no struct with named fields")),
            ));
        };
        let base_values = base.and_then(|b| self.struct_parts(b, ty)).map(|(_, v)| v);
        for def in value_fields(defs) {
            let name = def.ident.clone().unwrap_or_default();
            let Some(new) = field(values, &name) else {
                continue;
            };
            let old = base_values.and_then(|b| field(b, &name));
            path.push(Step::Field(name.clone()));
            self.write_field(table, &name, old, new, &def.ty, path, depth)?;
            path.pop();
        }
        Ok(())
    }

    #[expect(clippy::too_many_arguments, reason = "the walk's state, one place")]
    fn write_field(
        &self,
        table: &mut dyn TableLike,
        name: &str,
        old: Option<&ValueOwned>,
        new: &ValueOwned,
        ty: &TypeOwned,
        path: &mut JsonPath,
        depth: usize,
    ) -> Result<()> {
        // a marker table is never a struct's own table, whatever the new value is (Q22/Q23)
        let current_marker = table
            .get(name)
            .and_then(|item| item_doc(item, &mut path.clone()).ok())
            .and_then(|doc| doc::marker(&doc));
        // a struct, or an option of one that is `Some`, is edited key by key, as `decode_onto` reads it
        let (inner_new, inner_old, inner_ty) = self.some_inner(new, old, ty);
        if current_marker.is_none() && self.struct_parts(inner_new, inner_ty).is_some() {
            if let Some(sub) = table.get_mut(name).and_then(Item::as_table_like_mut) {
                return self.write_struct(sub, inner_old, inner_new, inner_ty, path, depth + 1);
            }
            if table.get(name).is_none() {
                if old == Some(new) {
                    return Ok(());
                }
                let mut sub = Table::new();
                self.write_struct(&mut sub, inner_old, inner_new, inner_ty, path, depth + 1)?;
                table.insert(name, Item::Table(sub));
                return Ok(());
            }
        }
        if let Some(item) = table.get_mut(name) {
            let current = item_doc(item, &mut path.clone())
                .and_then(|doc| self.current_value(&doc, old, ty, path));
            if current.as_ref() == Ok(new) {
                return Ok(());
            }
        } else if old == Some(new) {
            return Ok(());
        }
        match self.encode_doc(new, ty, Form::Toml, path)? {
            // TOML has no null: a key left out would read back as `old`, so write the marker instead (Q22)
            Doc::Null if old.is_some_and(|o| *o != ValueOwned::Option(None)) => {
                self.set_item(table, name, marker_item(Marker::UnsetToNone));
                Ok(())
            }
            Doc::Null => {
                table.remove(name);
                Ok(())
            }
            doc => {
                let item = to_item(doc, &mut path.clone())?;
                self.set_item(table, name, item);
                Ok(())
            }
        }
    }

    /// `name` of `table` set to `item`, the old value's comment and spacing kept with it.
    fn set_item(&self, table: &mut dyn TableLike, name: &str, mut item: Item) {
        match table.get_mut(name) {
            Some(slot) => {
                if let (Some(was), Item::Value(now)) = (slot.as_value(), &mut item) {
                    *now.decor_mut() = was.decor().clone();
                }
                *slot = item;
            }
            None => {
                table.insert(name, item);
            }
        }
    }

    /// What `doc` currently decodes to at a field whose base value is `old`: a [Marker] resolves against `old`
    /// without needing `ty` ([Marker::UseParent] is `old` itself, [Marker::UnsetToNone] is `None`), anything else
    /// decodes by `ty` as usual.
    fn current_value(
        &self,
        doc: &Doc,
        old: Option<&ValueOwned>,
        ty: &TypeOwned,
        path: &JsonPath,
    ) -> Result<ValueOwned> {
        match doc::marker(doc) {
            Some(Marker::UseParent) => old.cloned().ok_or_else(|| {
                JsonError::new(
                    path,
                    ErrorKind::Unsupported(
                        "`{ use_parent = true }` has no base value to keep here".into(),
                    ),
                )
            }),
            Some(Marker::UnsetToNone) => Ok(ValueOwned::Option(None)),
            None => self.decode_doc(doc, ty, Form::Toml, path),
        }
    }

    /// For an `Option` type and a `Some` value: the inner value, the base's inner value when it is `Some` too, and
    /// the inner type; else the three as given.
    fn some_inner<'v>(
        &self,
        new: &'v ValueOwned,
        old: Option<&'v ValueOwned>,
        ty: &'v TypeOwned,
    ) -> (&'v ValueOwned, Option<&'v ValueOwned>, &'v TypeOwned)
    where
        'a: 'v,
    {
        let bundle: &'a ApiBundleOwned = self.bundle();
        match (ty.get_in_line(bundle), new) {
            (Ok(TypeOwned::Option { some_ty }), ValueOwned::Option(Some(inner))) => {
                let old = match old {
                    Some(ValueOwned::Option(Some(o))) => Some(o.as_ref()),
                    _ => None,
                };
                (inner, old, some_ty)
            }
            _ => (new, old, ty),
        }
    }

    /// The field definitions and named values of `value` when `ty` is a struct with named fields written as a
    /// table (no quantity).
    fn struct_parts<'v>(&self, value: &'v ValueOwned, ty: &'v TypeOwned) -> Option<StructParts<'v>>
    where
        'a: 'v,
    {
        let bundle: &'a ApiBundleOwned = self.bundle();
        let ty = ty.get_in_line(bundle).ok()?;
        if !self.is_raw() && quantity_type(ty, bundle) {
            return None;
        }
        match (ty, value) {
            (TypeOwned::Box(inner), _) => self.struct_parts(value, inner),
            (
                TypeOwned::Struct(s),
                ValueOwned::Struct {
                    fields: FieldsValueOwned::Named(values),
                },
            ) => match &s.fields {
                FieldsOwned::Named(defs) => Some((defs.as_slice(), values.as_slice())),
                _ => None,
            },
            _ => None,
        }
    }
}

fn field<'v>(values: &'v [(String, ValueOwned)], name: &str) -> Option<&'v ValueOwned> {
    values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// [Codec::to_toml] with the root `value`.
pub fn to_toml(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<Item> {
    Codec::new(bundle).to_toml(value, ty)
}

/// [Codec::from_toml] with the root `value`.
pub fn from_toml(item: &Item, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    Codec::new(bundle).from_toml(item, ty)
}

/// [Codec::decode_onto] with the root `value`.
pub fn decode_onto(
    base: &ValueOwned,
    table: &Table,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> Result<ValueOwned> {
    Codec::new(bundle).decode_onto(base, table, ty)
}

/// [Codec::write_onto] with the root `value`.
pub fn write_onto(
    table: &mut Table,
    base: &ValueOwned,
    value: &ValueOwned,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> Result<()> {
    Codec::new(bundle).write_onto(table, base, value, ty)
}

// ------------------------------------------------------------------------------------------------ TOML -> doc

fn unsupported(path: &JsonPath, what: &str) -> JsonError {
    JsonError::new(path, ErrorKind::Unsupported(what.into()))
}

/// `marker` as the one-key inline table it is written as, `{ unset_to_none = true }`.
fn marker_item(marker: Marker) -> Item {
    let mut table = InlineTable::new();
    table.insert(marker.key(), T::from(true));
    Item::Value(T::InlineTable(table))
}

fn item_doc(item: &Item, path: &mut JsonPath) -> Result<Doc> {
    match item {
        Item::None => Ok(Doc::Null),
        Item::Value(v) => value_doc(v, path),
        Item::Table(t) => table_doc(t, path),
        Item::ArrayOfTables(a) => {
            let mut items = Vec::with_capacity(a.len());
            for (idx, t) in a.iter().enumerate() {
                path.push(Step::Index(idx));
                items.push(table_doc(t, path)?);
                path.pop();
            }
            Ok(Doc::List(items))
        }
    }
}

fn table_doc(table: &dyn TableLike, path: &mut JsonPath) -> Result<Doc> {
    let mut members = Vec::with_capacity(table.len());
    for (key, item) in table.iter() {
        path.push(Step::Field(key.to_owned()));
        members.push((key.to_owned(), item_doc(item, path)?));
        path.pop();
    }
    Ok(Doc::Map(members))
}

fn value_doc(v: &T, path: &mut JsonPath) -> Result<Doc> {
    Ok(match v {
        T::String(s) => Doc::String(s.value().clone()),
        T::Integer(i) => Doc::Int(i128::from(*i.value())),
        T::Float(x) => Doc::Float(*x.value()),
        T::Boolean(b) => Doc::Bool(*b.value()),
        T::Datetime(_) => {
            return Err(unsupported(
                path,
                "a TOML date or time has no value in the API model: write it as a quantity or a string",
            ));
        }
        T::Array(a) => {
            let mut items = Vec::with_capacity(a.len());
            for (idx, v) in a.iter().enumerate() {
                path.push(Step::Index(idx));
                items.push(value_doc(v, path)?);
                path.pop();
            }
            Doc::List(items)
        }
        T::InlineTable(t) => table_doc(t, path)?,
    })
}

// ------------------------------------------------------------------------------------------------ doc -> TOML

/// A document as an item: a map is a [Table] (an implicit one when it holds only tables), a `null` member is left
/// out.
fn to_item(doc: Doc, path: &mut JsonPath) -> Result<Item> {
    match doc {
        Doc::Map(members) => {
            let mut table = Table::new();
            for (key, doc) in members {
                if doc == Doc::Null {
                    continue;
                }
                path.push(Step::Field(key.clone()));
                let item = to_item(doc, path)?;
                path.pop();
                table.insert(&key, item);
            }
            let only_tables = !table.is_empty() && table.iter().all(|(_, item)| item.is_table());
            table.set_implicit(only_tables);
            Ok(Item::Table(table))
        }
        Doc::Null => Err(unsupported(
            path,
            "TOML has no null: `None` here has no form",
        )),
        doc => to_value(doc, path).map(Item::Value),
    }
}

/// A document as a value: a map is an [InlineTable].
fn to_value(doc: Doc, path: &mut JsonPath) -> Result<T> {
    Ok(match doc {
        Doc::Null => {
            return Err(unsupported(
                path,
                "TOML has no null: `None` here has no form",
            ));
        }
        Doc::Bool(b) => T::from(b),
        Doc::Int(i) => match i64::try_from(i) {
            Ok(i) => T::from(i),
            Err(_) => T::from(i.to_string()),
        },
        Doc::Float(x) => T::from(x),
        Doc::String(s) => T::from(s),
        Doc::List(items) => {
            let mut array = Array::new();
            for (idx, doc) in items.into_iter().enumerate() {
                path.push(Step::Index(idx));
                array.push(to_value(doc, path)?);
                path.pop();
            }
            T::Array(array)
        }
        Doc::Map(members) => {
            let mut table = InlineTable::new();
            for (key, doc) in members {
                if doc == Doc::Null {
                    continue;
                }
                path.push(Step::Field(key.clone()));
                let value = to_value(doc, path)?;
                path.pop();
                table.insert(&key, value);
            }
            T::InlineTable(table)
        }
    })
}
