//! Dynamic decoder of [Series](TypeOwned::Series) (SW-60, CLT-29): a `Series<T>` read with `T` known only from the
//! type tree (introspection data), with the same rules as the `SeriesColumns` impls `#[derive_shrink_wrap(..,
//! series)]` generates (`docs/serdes/series.md`), and the column codecs of `shrink_wrap` itself
//! ([shrink_wrap::series::dynamic]).
//!
//! It reads the way the reference decoder of the design does: group by group in column order, each group over its
//! subsequence of items with the restart flag of each, which come from the columns read before it (the tags of the
//! enums and `Option`s above it and of the enum fields declared earlier beside it). The result is a tree of
//! [SeriesColumn]s, one leaf column per leaf field, which [DecodedSeries::into_items] turns into items (what
//! [ValueOwned::des_shrink_wrap_dyn] gives for a `Series`: a [ValueOwned::Vec]), or which is used as it is (Python's
//! numpy columns).
//!
//! The bytes are untrusted: malformed data is an error, never a panic. An item can take no bits at all (a constant
//! column, a run, an empty tuple), so the work and the memory are bounded by the decode's [Budget], not by the
//! bytes: each group takes one value per item in it, a value copied to several items (a run of strings, an opaque
//! field, a default) takes its size once per copy.

use crate::value::{
    Budget, VARIANTS_PER_VALUE, des_inner, numeric_base, signed_value, unsigned_value,
};
use crate::{ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned};
use anyhow::{Result, anyhow, bail};
use shrink_wrap::series::dynamic::{
    RunKind, read_count, read_float, read_numeric, read_numeric128, read_opaque_run, read_run,
    read_struct_head,
};
use shrink_wrap::series::{Level, StructExtent};
use shrink_wrap::{BufReader, Nibble, UNib32, UVlq32, UVlq64};
use std::collections::HashMap;
use ww_numeric::{NumericBaseType, NumericValue};

/// A `Series` read column by column: its item count and the columns of `T` over all items.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedSeries {
    pub len: usize,
    pub root: SeriesColumn,
}

/// The columns of one group over its items (all items of the `Series` at the top, the items in a variant or
/// `Some` below an enum or `Option`), in item order.
#[derive(Clone, Debug, PartialEq)]
pub enum SeriesColumn {
    /// The group has no items: nothing was written.
    Empty,
    /// A leaf: a number, float, bool, string or byte list, or a field marked `#[series(opaque)]`. One value per
    /// item, as [ValueOwned::des_shrink_wrap_dyn] reads it in plain form.
    Leaf(Vec<ValueOwned>),
    /// A field an older writer's struct doesn't have: its `#[default]` for each of `len` items.
    Default { value: ValueOwned, len: usize },
    /// A struct, tuple or array: the columns of each field over the same items.
    Fields {
        kind: FieldsKind,
        len: usize,
        /// Field name, or the position for unnamed fields, tuple and array elements.
        fields: Vec<(String, SeriesColumn)>,
    },
    /// An enum: the variant of each item (an index into `variants`), and the fields of each variant that has
    /// items over those items (a variant no item is in is not listed: the type can have many more variants than
    /// the `Series` has items).
    Enum {
        variant_of: Vec<u32>,
        variants: Vec<VariantColumns>,
    },
    /// An `Option`: whether each item is `Some`, and the columns of the `Some` items.
    Option {
        some: Vec<bool>,
        inner: Box<SeriesColumn>,
    },
}

/// The columns of one enum variant.
#[derive(Clone, Debug, PartialEq)]
pub struct VariantColumns {
    pub ident: String,
    /// Position of the variant in the enum's definition ([ItemEnumOwned::variants](crate::ItemEnumOwned::variants)).
    pub variant_idx: u32,
    /// [SeriesColumn::Fields] of the items in this variant ([FieldsKind::Unit] and no fields for a unit variant).
    pub fields: SeriesColumn,
}

/// What a [SeriesColumn::Fields] makes of its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldsKind {
    /// Struct (or variant) with named fields.
    Named,
    /// Tuple struct (or variant).
    Unnamed,
    /// Unit struct (or variant).
    Unit,
    Tuple,
    Array,
}

impl SeriesColumn {
    /// Number of items the column has.
    pub fn len(&self) -> usize {
        match self {
            SeriesColumn::Empty => 0,
            SeriesColumn::Leaf(values) => values.len(),
            SeriesColumn::Default { len, .. } | SeriesColumn::Fields { len, .. } => *len,
            SeriesColumn::Enum { variant_of, .. } => variant_of.len(),
            SeriesColumn::Option { some, .. } => some.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl DecodedSeries {
    /// Reads a `Series` of `T` = `item_ty` at `rd` (the count, the group, the padding to the next byte).
    fn read(
        rd: &mut BufReader,
        item_ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
        b: &mut Budget,
    ) -> Result<Self> {
        let len = read_count(rd)?;
        let root = if len == 0 {
            SeriesColumn::Empty
        } else {
            b.take(len)?;
            let restarts = vec![false; len];
            let mut dec = Decoder { api_bundle, b };
            dec.group(rd, item_ty, &restarts, Level::Outermost)?.0
        };
        rd.align_byte();
        Ok(DecodedSeries { len, root })
    }

    /// The items, as plain values: what decoding a `Vec<T>` with the same items gives.
    pub fn into_items(self) -> Vec<ValueOwned> {
        self.root.into_items()
    }
}

impl SeriesColumn {
    /// The items of the column as plain values.
    pub fn into_items(self) -> Vec<ValueOwned> {
        match self {
            SeriesColumn::Empty => vec![],
            SeriesColumn::Leaf(values) => values,
            SeriesColumn::Default { value, len } => vec![value; len],
            SeriesColumn::Fields { kind, len, fields } => fields_into_items(kind, len, fields)
                .into_iter()
                .map(|fields| match (kind, fields) {
                    (FieldsKind::Tuple, FieldsValueOwned::Unnamed(values)) => {
                        ValueOwned::Tuple(values)
                    }
                    (FieldsKind::Array, FieldsValueOwned::Unnamed(values)) => {
                        ValueOwned::Array(values)
                    }
                    (_, fields) => ValueOwned::Struct { fields },
                })
                .collect(),
            SeriesColumn::Enum {
                variant_of,
                variants,
            } => {
                let mut per_variant: Vec<(String, std::vec::IntoIter<FieldsValueOwned>)> = variants
                    .into_iter()
                    .map(|v| {
                        let fields = match v.fields {
                            SeriesColumn::Fields { kind, len, fields } => {
                                fields_into_items(kind, len, fields)
                            }
                            _ => vec![],
                        };
                        (v.ident, fields.into_iter())
                    })
                    .collect();
                variant_of
                    .into_iter()
                    .map(|idx| {
                        let (ident, fields) = &mut per_variant[idx as usize];
                        ValueOwned::Enum {
                            variant: ident.clone(),
                            fields: fields.next().unwrap_or(FieldsValueOwned::Unit),
                        }
                    })
                    .collect()
            }
            SeriesColumn::Option { some, inner } => {
                let mut inner = inner.into_items().into_iter();
                some.into_iter()
                    .map(|is_some| {
                        ValueOwned::Option(if is_some {
                            inner.next().map(Box::new)
                        } else {
                            None
                        })
                    })
                    .collect()
            }
        }
    }

    /// The columns [DecodedSeries::read] would give for `items` (all of type `item_ty`), built straight from
    /// already-decoded values instead of `Series` bytes: for code that has items, not a `Series` to read (a
    /// batched stream's items, API-41, already unwrapped one by one by [crate::ValueOwned::des_shrink_wrap_dyn]
    /// through `DynBatching::decode`). No [Budget]: there is no untrusted data to bound, and every item already
    /// decoded successfully under `item_ty`, so every field a non-flag [FieldOwned] names is there (an older
    /// writer's missing trailing field was already filled with its default by that decode).
    pub fn from_items(
        items: &[ValueOwned],
        item_ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<SeriesColumn> {
        let m = items.len();
        if m == 0 {
            return Ok(SeriesColumn::Empty);
        }
        let ty = item_ty.get_in_line(api_bundle)?;
        match ty {
            TypeOwned::Struct(item_struct) => {
                let (kind, defs) = fields_of(&item_struct.fields);
                let fields_list: Vec<&FieldsValueOwned> = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Struct { fields } => Ok(fields),
                        _ => Err(items_mismatch(item_ty)),
                    })
                    .collect::<Result<_>>()?;
                let fields = fields_from_items(&fields_list, defs, api_bundle)?;
                Ok(SeriesColumn::Fields {
                    kind,
                    len: m,
                    fields,
                })
            }
            TypeOwned::Tuple(types) => {
                let fields = types
                    .iter()
                    .enumerate()
                    .map(|(j, ty)| {
                        let column_items = elements_at(items, j, item_ty)?;
                        Ok((
                            j.to_string(),
                            SeriesColumn::from_items(&column_items, ty, api_bundle)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(SeriesColumn::Fields {
                    kind: FieldsKind::Tuple,
                    len: m,
                    fields,
                })
            }
            TypeOwned::Array { len, ty } => {
                let len = len.fixed()? as usize;
                let fields = (0..len)
                    .map(|j| {
                        let column_items = elements_at(items, j, item_ty)?;
                        Ok((
                            j.to_string(),
                            SeriesColumn::from_items(&column_items, ty, api_bundle)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(SeriesColumn::Fields {
                    kind: FieldsKind::Array,
                    len: m,
                    fields,
                })
            }
            TypeOwned::Enum(item_enum) => {
                let mut slot_of: HashMap<&str, u32> = HashMap::new();
                let mut variant_of: Vec<u32> = Vec::with_capacity(m);
                let mut groups: Vec<(&str, Vec<&FieldsValueOwned>)> = vec![];
                for v in items {
                    let ValueOwned::Enum { variant, fields } = v else {
                        return Err(items_mismatch(item_ty));
                    };
                    let slot = *slot_of.entry(variant.as_str()).or_insert_with(|| {
                        groups.push((variant.as_str(), vec![]));
                        (groups.len() - 1) as u32
                    });
                    groups[slot as usize].1.push(fields);
                    variant_of.push(slot);
                }
                let variants = groups
                    .into_iter()
                    .map(|(ident, fields_list)| {
                        let (variant_idx, def) = item_enum
                            .variants
                            .iter()
                            .enumerate()
                            .find(|(_, d)| d.ident == ident)
                            .ok_or_else(|| items_mismatch(item_ty))?;
                        let (kind, defs) = fields_of(&def.fields);
                        let len = fields_list.len();
                        let fields = fields_from_items(&fields_list, defs, api_bundle)?;
                        Ok(VariantColumns {
                            ident: ident.to_string(),
                            variant_idx: variant_idx as u32,
                            fields: SeriesColumn::Fields { kind, len, fields },
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(SeriesColumn::Enum {
                    variant_of,
                    variants,
                })
            }
            TypeOwned::Option { some_ty } => {
                let mut some = Vec::with_capacity(m);
                let mut inner_items = Vec::with_capacity(m);
                for v in items {
                    let ValueOwned::Option(opt) = v else {
                        return Err(items_mismatch(item_ty));
                    };
                    match opt {
                        Some(value) => {
                            some.push(true);
                            inner_items.push((**value).clone());
                        }
                        None => some.push(false),
                    }
                }
                let inner = SeriesColumn::from_items(&inner_items, some_ty, api_bundle)?;
                Ok(SeriesColumn::Option {
                    some,
                    inner: Box::new(inner),
                })
            }
            // numbers, floats, bools, strings, byte lists and #[series(opaque)] fields: one value per item, as
            // the plain decoder already gave them
            _ => Ok(SeriesColumn::Leaf(items.to_vec())),
        }
    }
}

/// The `j`-th element of every item's `Tuple` or `Array` value.
fn elements_at(items: &[ValueOwned], j: usize, item_ty: &TypeOwned) -> Result<Vec<ValueOwned>> {
    items
        .iter()
        .map(|v| match v {
            ValueOwned::Tuple(values) | ValueOwned::Array(values) => values
                .get(j)
                .cloned()
                .ok_or_else(|| items_mismatch(item_ty)),
            _ => Err(items_mismatch(item_ty)),
        })
        .collect()
}

/// Pivots the same-shaped `fields_list` (one [FieldsValueOwned] per item of a struct, variant, or [Self::Empty]
/// context) into one [SeriesColumn] per non-flag field of `defs`, by position (as [FieldsValueOwned] already
/// leaves flags out, same as `defs` once filtered).
fn fields_from_items(
    fields_list: &[&FieldsValueOwned],
    defs: &[FieldOwned],
    api_bundle: &ApiBundleOwned,
) -> Result<Vec<(String, SeriesColumn)>> {
    defs.iter()
        .enumerate()
        .map(|(j, def)| {
            let name = def.ident.clone().unwrap_or_else(|| j.to_string());
            let column_items: Vec<ValueOwned> = fields_list
                .iter()
                .map(|fields| field_at(fields, j))
                .collect::<Result<_>>()?;
            let column = SeriesColumn::from_items(&column_items, &def.ty, api_bundle)?;
            Ok((name, column))
        })
        .collect()
}

/// The `j`-th non-flag field's value of a struct or variant's fields.
fn field_at(fields: &FieldsValueOwned, j: usize) -> Result<ValueOwned> {
    match fields {
        FieldsValueOwned::Named(values) => values
            .get(j)
            .map(|(_, v)| v.clone())
            .ok_or(shrink_wrap::Error::MalformedSeries.into()),
        FieldsValueOwned::Unnamed(values) => values
            .get(j)
            .cloned()
            .ok_or(shrink_wrap::Error::MalformedSeries.into()),
        FieldsValueOwned::Unit => Err(shrink_wrap::Error::MalformedSeries.into()),
    }
}

fn items_mismatch(item_ty: &TypeOwned) -> anyhow::Error {
    anyhow!("an item doesn't match its type ({item_ty:?})")
}

/// The fields of `len` items from the columns of each field.
fn fields_into_items(
    kind: FieldsKind,
    len: usize,
    fields: Vec<(String, SeriesColumn)>,
) -> Vec<FieldsValueOwned> {
    if kind == FieldsKind::Unit {
        return vec![FieldsValueOwned::Unit; len];
    }
    let mut columns: Vec<(String, std::vec::IntoIter<ValueOwned>)> = fields
        .into_iter()
        .map(|(name, column)| (name, column.into_items().into_iter()))
        .collect();
    (0..len)
        .map(|_| {
            let mut values = columns
                .iter_mut()
                .map(|(name, column)| (name, column.next().unwrap_or(ValueOwned::Bool(false))));
            if kind == FieldsKind::Named {
                FieldsValueOwned::Named(values.map(|(name, v)| (name.clone(), v)).collect())
            } else {
                FieldsValueOwned::Unnamed(values.by_ref().map(|(_, v)| v).collect())
            }
        })
        .collect()
}

/// Reads a `Series` of `item_ty` at `rd` as items (a [ValueOwned::Vec]).
pub(crate) fn des_series(
    rd: &mut BufReader,
    item_ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
    b: &mut Budget,
) -> Result<ValueOwned> {
    let series = DecodedSeries::read(rd, item_ty, api_bundle, b)?;
    // the names of the fields and variants are copied into every item
    charge_names(&series.root, b)?;
    Ok(ValueOwned::Vec(series.into_items()))
}

/// Takes the share of the field and variant names [SeriesColumn::into_items] copies into each item.
fn charge_names(column: &SeriesColumn, b: &mut Budget) -> Result<()> {
    match column {
        SeriesColumn::Fields { kind, len, fields } => {
            for (name, column) in fields {
                if *kind == FieldsKind::Named {
                    b.names(name, *len)?;
                }
                charge_names(column, b)?;
            }
        }
        SeriesColumn::Enum {
            variant_of,
            variants,
        } => {
            let mut counts = vec![0usize; variants.len()];
            for &slot in variant_of {
                if let Some(count) = counts.get_mut(slot as usize) {
                    *count += 1;
                }
            }
            for (v, count) in variants.iter().zip(counts) {
                b.names(&v.ident, count)?;
                charge_names(&v.fields, b)?;
            }
        }
        SeriesColumn::Option { inner, .. } => charge_names(inner, b)?,
        SeriesColumn::Empty | SeriesColumn::Leaf(_) | SeriesColumn::Default { .. } => {}
    }
    Ok(())
}

impl ValueOwned {
    /// Reads one top-level value of type `ty`, a [Series](TypeOwned::Series), column by column: the columns
    /// themselves, without building items (see [DecodedSeries::into_items] for them).
    pub fn des_series_columns_dyn(
        bytes: &[u8],
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<DecodedSeries> {
        let TypeOwned::Series(item_ty) = ty else {
            bail!("expected a Series, got {ty:?}");
        };
        let mut rd = BufReader::new(bytes);
        let mut b = Budget::for_bytes(bytes.len());
        DecodedSeries::read(&mut rd, item_ty, api_bundle, &mut b)
    }
}

struct Decoder<'a, 'b> {
    api_bundle: &'a ApiBundleOwned,
    b: &'b mut Budget,
}

/// A group's columns, and the tag (enum discriminant, `Option`'s 0 / 1) of each item when it has one: a change of
/// it restarts the fields declared after it.
type Group = (SeriesColumn, Option<Vec<u32>>);

impl Decoder<'_, '_> {
    /// The group of `ty` over the items of `restarts` (one per item, whether its columns restart there).
    fn group(
        &mut self,
        rd: &mut BufReader,
        ty: &TypeOwned,
        restarts: &[bool],
        level: Level,
    ) -> Result<Group> {
        let m = restarts.len();
        if m == 0 {
            return Ok((SeriesColumn::Empty, None));
        }
        // an out-of-line definition is not a value of its own
        let ty = match ty {
            TypeOwned::OutOfLine { type_idx } => self.api_bundle.get_ty(type_idx.0)?.0,
            ty => ty,
        };
        // a value per item of every group, and the group one level deeper
        self.b.take(m)?;
        self.b.enter()?;
        let result = self.group_of(rd, ty, restarts, level);
        self.b.leave();
        result
    }

    fn group_of(
        &mut self,
        rd: &mut BufReader,
        ty: &TypeOwned,
        restarts: &[bool],
        level: Level,
    ) -> Result<Group> {
        let m = restarts.len();
        let api_bundle = self.api_bundle;
        match ty {
            TypeOwned::Bool => {
                let mut values = Vec::with_capacity(m);
                read_run(rd, RunKind::Bool, m, |v| {
                    values.push(ValueOwned::Bool(v == 1));
                    Ok(())
                })?;
                Ok((SeriesColumn::Leaf(values), None))
            }
            TypeOwned::NumericAny(any) => Ok((
                SeriesColumn::Leaf(numeric(rd, numeric_base(any), restarts)?),
                None,
            )),
            TypeOwned::String => {
                let values = self.opaque_bytes(rd, m, |bytes, b| {
                    let s = core::str::from_utf8(bytes)
                        .map_err(|_| shrink_wrap::Error::MalformedUtf8)?;
                    b.bytes(s.len())?;
                    Ok(ValueOwned::String(s.to_string()))
                })?;
                Ok((SeriesColumn::Leaf(values), None))
            }
            TypeOwned::Vec(inner) if is_u8(inner) => {
                let values = self.opaque_bytes(rd, m, |bytes, b| {
                    b.take(bytes.len())?;
                    Ok(ValueOwned::Vec(
                        bytes
                            .iter()
                            .map(|&x| ValueOwned::Numeric(NumericValue::U8(x)))
                            .collect(),
                    ))
                })?;
                Ok((SeriesColumn::Leaf(values), None))
            }
            TypeOwned::Option { some_ty } => {
                let mut some = Vec::with_capacity(m);
                let mut tags = Vec::with_capacity(m);
                read_run(rd, RunKind::OptionTag, m, |v| {
                    some.push(v == 1);
                    tags.push(v);
                    Ok(())
                })?;
                // entering `Some` from `None` restarts
                let inner_restarts: Vec<bool> = (0..m)
                    .filter(|&i| some[i])
                    .map(|i| restarts[i] || (i > 0 && !some[i - 1]))
                    .collect();
                let (inner, _) = self.group(rd, some_ty, &inner_restarts, Level::Nested)?;
                Ok((
                    SeriesColumn::Option {
                        some,
                        inner: Box::new(inner),
                    },
                    Some(tags),
                ))
            }
            TypeOwned::Tuple(types) => {
                let fields = self.elements(rd, types.iter(), restarts)?;
                Ok((
                    SeriesColumn::Fields {
                        kind: FieldsKind::Tuple,
                        len: m,
                        fields,
                    },
                    None,
                ))
            }
            TypeOwned::Array { len, ty } => {
                let len = len.fixed()? as usize;
                let fields = self.elements(rd, std::iter::repeat_n(ty.as_ref(), len), restarts)?;
                Ok((
                    SeriesColumn::Fields {
                        kind: FieldsKind::Array,
                        len: m,
                        fields,
                    },
                    None,
                ))
            }
            TypeOwned::Struct(item_struct) => {
                if item_struct.sparse {
                    bail!(
                        "struct '{}' is sparse: not in Series v1 (only as a #[series(opaque)] field)",
                        item_struct.ident
                    );
                }
                let extent = read_struct_head(rd, level, item_struct.is_unsized(), m)?;
                let (kind, defs) = fields_of(&item_struct.fields);
                let fields = self.struct_fields(rd, defs, restarts, extent)?;
                Ok((
                    SeriesColumn::Fields {
                        kind,
                        len: m,
                        fields,
                    },
                    None,
                ))
            }
            TypeOwned::Enum(item_enum) => {
                // the variants with items, in order of first appearance (a slot each): the definition index and
                // the items in it; the work is per item and per tag change, never per variant of the type
                let mut variant_of: Vec<u32> = Vec::with_capacity(m);
                let mut tags = Vec::with_capacity(m);
                let mut present: Vec<(usize, Vec<usize>)> = vec![];
                let mut slot_of: HashMap<usize, u32> = HashMap::new();
                let mut last: Option<(u32, u32)> = None;
                let b = &mut *self.b;
                read_run(rd, RunKind::Tag, m, |tag| {
                    let slot = match last {
                        Some((t, slot)) if t == tag => slot,
                        _ => {
                            let position = item_enum
                                .variants
                                .iter()
                                .position(|v| v.discriminant.0 == tag);
                            // the lookup is the work of a value per that many variants looked through
                            b.take(
                                position.unwrap_or(item_enum.variants.len()) / VARIANTS_PER_VALUE,
                            )
                            .map_err(|_| shrink_wrap::Error::MalformedSeries)?;
                            let idx = position
                                .ok_or(shrink_wrap::Error::EnumFutureVersionOrMalformedData)?;
                            let slot = *slot_of.entry(idx).or_insert_with(|| {
                                present.push((idx, vec![]));
                                (present.len() - 1) as u32
                            });
                            last = Some((tag, slot));
                            slot
                        }
                    };
                    present[slot as usize].1.push(variant_of.len());
                    variant_of.push(slot);
                    tags.push(tag);
                    Ok(())
                })?;
                let mut variants: Vec<VariantColumns> = present
                    .iter()
                    .map(|&(idx, _)| VariantColumns {
                        ident: item_enum.variants[idx].ident.clone(),
                        variant_idx: idx as u32,
                        fields: SeriesColumn::Empty,
                    })
                    .collect();
                // the data variants' groups, in discriminant order
                let mut order: Vec<usize> = (0..present.len()).collect();
                order.sort_by_key(|&slot| item_enum.variants[present[slot].0].discriminant.0);
                for slot in order {
                    let (idx, items) = &present[slot];
                    let (kind, defs) = fields_of(&item_enum.variants[*idx].fields);
                    // entering the variant from another one restarts it
                    let rs: Vec<bool> = items
                        .iter()
                        .map(|&i| restarts[i] || (i > 0 && variant_of[i - 1] as usize != slot))
                        .collect();
                    let fields = self.struct_fields(rd, defs, &rs, StructExtent::All)?;
                    variants[slot].fields = SeriesColumn::Fields {
                        kind,
                        len: items.len(),
                        fields,
                    };
                }
                Ok((
                    SeriesColumn::Enum {
                        variant_of,
                        variants,
                    },
                    Some(tags),
                ))
            }
            TypeOwned::Instance { .. } | TypeOwned::Param(_) => Err(anyhow!(
                "a generic type that is not instantiated (crate::generics::prepare was not run)"
            )),
            other => Err(anyhow!(
                "{} is not in Series v1 (only as a #[series(opaque)] field)",
                other
                    .human_name(true, api_bundle)
                    .unwrap_or_else(|_| "this type".into())
            )),
        }
    }

    /// The fields of a struct or variant (flags left out: they belong to their `Option`), as far as `extent` says
    /// the data has them.
    fn struct_fields(
        &mut self,
        rd: &mut BufReader,
        defs: &[FieldOwned],
        restarts: &[bool],
        extent: StructExtent,
    ) -> Result<Vec<(String, SeriesColumn)>> {
        let fields: Vec<(String, &FieldOwned)> = defs
            .iter()
            .filter(|f| !f.is_flag())
            .enumerate()
            .map(|(j, f)| (f.ident.clone().unwrap_or_else(|| j.to_string()), f))
            .collect();
        let m = restarts.len();
        let mut out = Vec::with_capacity(fields.len());
        // the restarts of the next field: the parent's, and where an enum or `Option` field before it changed
        let mut rs = restarts.to_vec();
        for (j, (name, def)) in fields.iter().enumerate() {
            if !extent.has_field(rd, j) {
                // an older writer: the field's default, else malformed
                let value = def
                    .default
                    .clone()
                    .ok_or(shrink_wrap::Error::MalformedSeries)?;
                self.b.copies(&value, m)?;
                out.push((name.clone(), SeriesColumn::Default { value, len: m }));
                continue;
            }
            if def.series_opaque {
                let column = self.opaque_field(rd, &def.ty, m)?;
                out.push((name.clone(), column));
                continue;
            }
            let (column, tag) = self.group(rd, &def.ty, &rs, Level::Nested)?;
            out.push((name.clone(), column));
            if let Some(tag) = tag {
                add_tag_restarts(&mut rs, &tag);
            }
        }
        extent.finish(rd, fields.len())?;
        Ok(out)
    }

    /// Tuple and array elements: the struct rule without a head, named by position.
    fn elements<'t>(
        &mut self,
        rd: &mut BufReader,
        types: impl Iterator<Item = &'t TypeOwned>,
        restarts: &[bool],
    ) -> Result<Vec<(String, SeriesColumn)>> {
        let mut out = vec![];
        let mut rs = restarts.to_vec();
        for (j, ty) in types.enumerate() {
            let (column, tag) = self.group(rd, ty, &rs, Level::Nested)?;
            out.push((j.to_string(), column));
            if let Some(tag) = tag {
                add_tag_restarts(&mut rs, &tag);
            }
        }
        Ok(out)
    }

    /// An opaque run over `m` items: each new value made by `make` once, copied to the items that repeat it.
    fn opaque_bytes(
        &mut self,
        rd: &mut BufReader,
        m: usize,
        mut make: impl FnMut(&[u8], &mut Budget) -> Result<ValueOwned>,
    ) -> Result<Vec<ValueOwned>> {
        let mut values: Vec<ValueOwned> = Vec::with_capacity(m);
        let mut error = None;
        let b = &mut *self.b;
        let read = read_opaque_run(rd, m, |bytes, changes| {
            let result = if changes {
                make(bytes, b)
            } else {
                // a copy of the previous item's value costs what that one cost
                let prev = values.last().cloned().unwrap_or(ValueOwned::Bool(false));
                b.copies(&prev, 1).map(|_| prev)
            };
            match result {
                Ok(value) => {
                    values.push(value);
                    Ok(())
                }
                Err(e) => {
                    error = Some(e);
                    Err(shrink_wrap::Error::MalformedSeries)
                }
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
        read?;
        Ok(values)
    }

    /// A field marked `#[series(opaque)]`: a run of each value's own bytes (`to_ww_bytes`), read as a top-level
    /// value of the field's type.
    fn opaque_field(
        &mut self,
        rd: &mut BufReader,
        ty: &TypeOwned,
        m: usize,
    ) -> Result<SeriesColumn> {
        if m == 0 {
            return Ok(SeriesColumn::Empty);
        }
        self.b.take(m)?;
        let api_bundle = self.api_bundle;
        let values = self.opaque_bytes(rd, m, |bytes, b| {
            let mut rd = BufReader::new(bytes);
            des_inner(&mut rd, ty, api_bundle, b)
        })?;
        Ok(SeriesColumn::Leaf(values))
    }
}

/// Adds to the restarts of the fields after an enum or `Option` field where its `tags` (one per item) changed from
/// the item before.
fn add_tag_restarts(restarts: &mut [bool], tags: &[u32]) {
    for i in 1..restarts.len().min(tags.len()) {
        if tags[i] != tags[i - 1] {
            restarts[i] = true;
        }
    }
}

fn fields_of(fields: &FieldsOwned) -> (FieldsKind, &[FieldOwned]) {
    match fields {
        FieldsOwned::Named(defs) => (FieldsKind::Named, defs),
        FieldsOwned::Unnamed(defs) => (FieldsKind::Unnamed, defs),
        FieldsOwned::Unit => (FieldsKind::Unit, &[]),
    }
}

fn is_u8(ty: &TypeOwned) -> bool {
    matches!(ty, TypeOwned::NumericAny(any) if *numeric_base(any) == NumericBaseType::U8)
}

/// A numeric or float column over the items of `restarts`, values as the plain decoder makes them.
fn numeric(
    rd: &mut BufReader,
    base: &NumericBaseType,
    restarts: &[bool],
) -> Result<Vec<ValueOwned>> {
    use NumericBaseType as B;
    let m = restarts.len();
    let mut out = Vec::with_capacity(m);
    let restarts = restarts.iter().copied();
    let (width, signed) = match base {
        B::F32 => {
            read_float::<f32>(rd, restarts, |v| {
                out.push(ValueOwned::Numeric(NumericValue::F32(v)));
                Ok(())
            })?;
            return Ok(out);
        }
        B::F64 => {
            read_float::<f64>(rd, restarts, |v| {
                out.push(ValueOwned::Numeric(NumericValue::F64(v)));
                Ok(())
            })?;
            return Ok(out);
        }
        B::U128 | B::I128 => {
            let signed = *base == B::I128;
            read_numeric128(rd, signed, m, |v| {
                out.push(ValueOwned::Numeric(if signed {
                    NumericValue::I128(v as i128)
                } else {
                    NumericValue::U128(v)
                }));
                Ok(())
            })?;
            return Ok(out);
        }
        B::Nibble => (4, false),
        B::U8 => (8, false),
        B::U16 => (16, false),
        B::U32 | B::UNib32 | B::UVlq32 => (32, false),
        B::U64 | B::UVlq64 => (64, false),
        B::I8 => (8, true),
        B::I16 => (16, true),
        B::I32 => (32, true),
        B::I64 => (64, true),
        B::UB(bits) => (bits.0, false),
        B::IB(bits) => (bits.0, true),
        other => bail!("numeric type {other:?} is not supported in a Series"),
    };
    if !(1..=64).contains(&width) {
        bail!("numeric type {base:?} has no column");
    }
    read_numeric(rd, width, signed, restarts, |bits| {
        let x = if signed {
            let shift = 64 - width as u32;
            ((bits << shift) as i64 >> shift) as u64
        } else {
            bits
        };
        out.push(ValueOwned::Numeric(match base {
            B::Nibble => NumericValue::Nibble(Nibble::new_masked(x as u8)),
            B::U8 => NumericValue::U8(x as u8),
            B::U16 => NumericValue::U16(x as u16),
            B::U32 => NumericValue::U32(x as u32),
            B::U64 => NumericValue::U64(x),
            B::UNib32 => NumericValue::UNib32(UNib32(x as u32)),
            B::UVlq32 => NumericValue::UVlq32(UVlq32(x as u32)),
            B::UVlq64 => NumericValue::UVlq64(UVlq64(x)),
            B::I8 => NumericValue::I8(x as i8),
            B::I16 => NumericValue::I16(x as i16),
            B::I32 => NumericValue::I32(x as i32),
            B::I64 => NumericValue::I64(x as i64),
            B::IB(bits) => signed_value(bits.0, x as i64),
            _ => unsigned_value(width, x),
        }));
        Ok(())
    })?;
    Ok(out)
}
