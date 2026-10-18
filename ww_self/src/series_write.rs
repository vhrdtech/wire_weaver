//! Dynamic encoder of [Series](TypeOwned::Series) (CLT-29): a `Series<T>` written from [ValueOwned] items with `T`
//! known only from the type tree, the same bytes `#[derive_shrink_wrap(.., series)]` writes for the same items and
//! [CodecPolicy] (the rules of `docs/serdes/series.md`, the column codecs of [shrink_wrap::series::dynamic]).
//!
//! Two passes, the mirror of the decoder in `series.rs`: [Encoder] walks the type tree with the items, group by
//! group in column order, each group over its subsequence of items with the restart flag of each (from the tags of
//! the enums and `Option`s above it and of the enum fields declared earlier beside it), into a [Plan]: the
//! column values of every leaf and the shape around them. [Plan::write] then writes it, picking each column's codec
//! by the policy. A nested evolvable struct's group length is measured by writing its fields into a
//! [BitCounter]; the count depends only on where the fields start within a byte, so it is kept per start bit and
//! a struct nested in structs is measured at most eight times, not once per enclosing measuring pass.
//!
//! Limits, as for decoding: at most [SERIES_MAX_ITEMS] items, types nested at most [MAX_DECODE_DEPTH] levels, and
//! at most [ENCODE_MAX_VALUES] column values in all (a value per item of every group).

use crate::series::{add_tag_restarts, fields_of, is_u8};
use crate::value::{
    MAX_DECODE_DEPTH, check_field_names, find_field, float_value, int_value, missing_field,
    not_instantiated, numeric_base, type_name, variant_names,
};
use crate::{ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned};
use anyhow::{Result, anyhow, bail};
use shrink_wrap::series::dynamic::{
    RunKind, SERIES_MAX_ITEMS, write_count_owned, write_float, write_numeric, write_numeric128,
    write_opaque_field, write_opaque_run, write_run, write_struct_head,
};
use shrink_wrap::series::{BitCounter, BitSink, CodecPolicy, Level};
use shrink_wrap::{BufWriterOwned, Error};
use std::borrow::Cow;
use std::cell::Cell;
use ww_numeric::{NumericBaseType, NumericValue};

/// Most column values one dynamic `Series` encode collects (a value per item of every group): 256 leaf columns of
/// [SERIES_MAX_ITEMS] items.
pub const ENCODE_MAX_VALUES: usize = 1 << 24;

impl ValueOwned {
    /// Serialize a [Series](TypeOwned::Series) value (a [ValueOwned::Vec] of its items) as one top-level value of
    /// type `ty`, the codecs of its columns picked by `policy`: the bytes `Series::with_policy(items,
    /// policy).to_ww_bytes()` gives for the same items. [ser_shrink_wrap_dyn](Self::ser_shrink_wrap_dyn) writes a
    /// `Series` (also one inside another value) with [CodecPolicy::SMALLEST].
    pub fn ser_series_dyn(
        &self,
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
        policy: CodecPolicy,
    ) -> Result<Vec<u8>> {
        let TypeOwned::Series(item_ty) = ty.get_in_line(api_bundle)? else {
            bail!("expected a Series, got {}", type_name(ty, api_bundle));
        };
        let mut wr = BufWriterOwned::new();
        ser_series(&mut wr, self, item_ty, api_bundle, policy)?;
        Ok(wr.finish_and_take()?)
    }
}

/// Writes a `Series` of `item_ty` from `value`, its items: the count, the group, the padding to the next byte.
pub(crate) fn ser_series(
    wr: &mut BufWriterOwned,
    value: &ValueOwned,
    item_ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
    policy: CodecPolicy,
) -> Result<()> {
    let (ValueOwned::Vec(items) | ValueOwned::Array(items)) = value else {
        bail!(
            "expected Series<{}> (a list of items), got {value:?}",
            type_name(item_ty, api_bundle)
        );
    };
    if items.len() > SERIES_MAX_ITEMS {
        bail!(
            "a Series holds at most {SERIES_MAX_ITEMS} items, got {}",
            items.len()
        );
    }
    let items: Vec<&ValueOwned> = items.iter().collect();
    let plan = if items.is_empty() {
        Plan::empty()
    } else {
        let mut enc = Encoder {
            api_bundle,
            values_left: ENCODE_MAX_VALUES,
            depth: 0,
        };
        enc.group(&items, item_ty, &vec![false; items.len()], Level::Outermost)?
            .0
    };
    write_count_owned(wr, items.len())?;
    plan.write(wr, &policy)?;
    wr.align_byte();
    Ok(())
}

/// What a group writes before its parts.
#[derive(Debug)]
enum Head {
    None,
    /// An evolvable struct at the top: its field count.
    Count(u32),
    /// A nested evolvable struct: the bits of its parts, measured. `memo[at % 8]` is what they take when they
    /// start at bit `at`.
    Len {
        memo: Cell<[Option<usize>; 8]>,
    },
}

/// A group's columns as they are written: a column of leaf values, or the parts of a struct, tuple, array, enum
/// or `Option` (tag run first) in column order.
#[derive(Debug)]
enum Plan {
    Numeric {
        width: u8,
        signed: bool,
        items: Vec<(u64, bool)>,
    },
    Numeric128 {
        signed: bool,
        items: Vec<u128>,
    },
    F32(Vec<(f32, bool)>),
    F64(Vec<(f64, bool)>),
    Run(RunKind, Vec<u32>),
    /// Strings and byte lists: a change where the bytes differ.
    Bytes(Vec<Vec<u8>>),
    /// A `#[series(opaque)]` field: a change where the value differs.
    Opaque(Vec<OpaqueItem>),
    Group {
        head: Head,
        parts: Vec<Plan>,
    },
}

/// One item of an opaque field: its bytes, and its value as decoded from them, which is what the generated writer
/// compares with `==` (`0.0 == -0.0`, NaN never equal; a value given as another integer variant is still equal).
#[derive(Debug)]
struct OpaqueItem {
    bytes: Vec<u8>,
    value: ValueOwned,
}

impl PartialEq for OpaqueItem {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl AsRef<[u8]> for OpaqueItem {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Plan {
    /// A group without items: writes nothing.
    fn empty() -> Plan {
        Plan::Group {
            head: Head::None,
            parts: vec![],
        }
    }

    fn write<S: BitSink>(&self, sink: &mut S, policy: &CodecPolicy) -> Result<(), Error> {
        match self {
            Plan::Numeric {
                width,
                signed,
                items,
            } => write_numeric(sink, *width, *signed, items.iter().copied(), policy.numeric)
                .map(|_| ()),
            Plan::Numeric128 { signed, items } => {
                write_numeric128(sink, *signed, items.iter().copied())
            }
            Plan::F32(items) => write_float(sink, items.iter().copied(), policy.float).map(|_| ()),
            Plan::F64(items) => write_float(sink, items.iter().copied(), policy.float).map(|_| ()),
            Plan::Run(kind, items) => write_run(sink, *kind, items.iter().copied()),
            Plan::Bytes(items) => write_opaque_run(sink, items.iter().map(Vec::as_slice)),
            Plan::Opaque(items) => write_opaque_field(sink, items.iter()),
            Plan::Group { head, parts } => {
                match head {
                    Head::None => {}
                    Head::Count(fields) => {
                        write_struct_head(sink, Level::Outermost, true, *fields, |_| Ok(0))?
                    }
                    Head::Len { memo } => write_struct_head(sink, Level::Nested, true, 0, |at| {
                        let mut known = memo.get();
                        if let Some(bits) = known[at % 8] {
                            return Ok(bits);
                        }
                        let mut counter = BitCounter::at(at);
                        for part in parts {
                            part.write(&mut counter, policy)?;
                        }
                        known[at % 8] = Some(counter.bits());
                        memo.set(known);
                        Ok(counter.bits())
                    })?,
                }
                parts.iter().try_for_each(|part| part.write(sink, policy))
            }
        }
    }
}

struct Encoder<'a> {
    api_bundle: &'a ApiBundleOwned,
    values_left: usize,
    depth: usize,
}

/// A group's plan, and the tag (enum discriminant, `Option`'s 0 / 1) of each item when it has one: a change of it
/// restarts the fields declared after it.
type Group = (Plan, Option<Vec<u32>>);

impl Encoder<'_> {
    /// The group of `ty` over `items` (its subsequence), `restarts` whether each item's columns restart there.
    fn group(
        &mut self,
        items: &[&ValueOwned],
        ty: &TypeOwned,
        restarts: &[bool],
        level: Level,
    ) -> Result<Group> {
        let m = items.len();
        if m == 0 {
            return Ok((Plan::empty(), None));
        }
        let ty = match ty {
            TypeOwned::OutOfLine { type_idx } => self.api_bundle.get_ty(type_idx.0)?.0,
            ty => ty,
        };
        self.values_left = self.values_left.checked_sub(m).ok_or_else(|| {
            anyhow!("a Series of more than {ENCODE_MAX_VALUES} column values in all")
        })?;
        if self.depth >= MAX_DECODE_DEPTH {
            bail!("a Series item nested deeper than {MAX_DECODE_DEPTH} levels");
        }
        self.depth += 1;
        let result = self.group_of(items, ty, restarts, level);
        self.depth -= 1;
        result
    }

    fn group_of(
        &mut self,
        items: &[&ValueOwned],
        ty: &TypeOwned,
        restarts: &[bool],
        level: Level,
    ) -> Result<Group> {
        let api_bundle = self.api_bundle;
        let mismatch =
            |value: &ValueOwned| anyhow!("expected {}, got {value:?}", type_name(ty, api_bundle));
        match ty {
            TypeOwned::Bool => {
                let values = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Bool(b) => Ok(*b as u32),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<_>>()?;
                Ok((Plan::Run(RunKind::Bool, values), None))
            }
            TypeOwned::NumericAny(any) => {
                let values = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Numeric(n) => Ok(n),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok((numeric(&values, numeric_base(any), restarts)?, None))
            }
            TypeOwned::String => {
                let values = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::String(s) => Ok(s.as_bytes().to_vec()),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<_>>()?;
                Ok((Plan::Bytes(values), None))
            }
            TypeOwned::Vec(inner) if is_u8(inner) => {
                let values = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Vec(bytes) | ValueOwned::Array(bytes) => bytes
                            .iter()
                            .map(|b| match b {
                                ValueOwned::Numeric(n) => u8::try_from(int_value(n)?)
                                    .map_err(|_| anyhow!("{n:?} is out of range for U8")),
                                b => Err(anyhow!("expected U8, got {b:?}")),
                            })
                            .collect::<Result<Vec<u8>>>(),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<_>>()?;
                Ok((Plan::Bytes(values), None))
            }
            TypeOwned::Option { some_ty } => {
                let mut tags = Vec::with_capacity(items.len());
                let mut inner = vec![];
                let mut inner_restarts = vec![];
                for (i, v) in items.iter().enumerate() {
                    let ValueOwned::Option(o) = v else {
                        return Err(mismatch(v));
                    };
                    tags.push(o.is_some() as u32);
                    if let Some(value) = o {
                        inner.push(value.as_ref());
                        // entering `Some` from `None` restarts
                        inner_restarts.push(restarts[i] || (i > 0 && tags[i - 1] == 0));
                    }
                }
                let (inner, _) = self.group(&inner, some_ty, &inner_restarts, Level::Nested)?;
                Ok((
                    Plan::Group {
                        head: Head::None,
                        parts: vec![Plan::Run(RunKind::OptionTag, tags.clone()), inner],
                    },
                    Some(tags),
                ))
            }
            TypeOwned::Tuple(types) => {
                let elements = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Tuple(values) if values.len() == types.len() => Ok(values),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let parts = self.elements(&elements, types.iter(), restarts)?;
                Ok((
                    Plan::Group {
                        head: Head::None,
                        parts,
                    },
                    None,
                ))
            }
            TypeOwned::Array {
                len,
                ty: element_ty,
            } => {
                let len = len.fixed()? as usize;
                let elements = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Array(values) | ValueOwned::Vec(values)
                            if values.len() == len =>
                        {
                            Ok(values)
                        }
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let parts = self.elements(
                    &elements,
                    std::iter::repeat_n(element_ty.as_ref(), len),
                    restarts,
                )?;
                Ok((
                    Plan::Group {
                        head: Head::None,
                        parts,
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
                let fields = items
                    .iter()
                    .map(|v| match v {
                        ValueOwned::Struct { fields } => Ok(fields),
                        v => Err(mismatch(v)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let parts =
                    self.struct_fields(&fields, &item_struct.fields, restarts, &item_struct.ident)?;
                let head = match level {
                    _ if !item_struct.is_unsized() => Head::None,
                    Level::Outermost => Head::Count(parts.len() as u32),
                    Level::Nested => Head::Len {
                        memo: Cell::new([None; 8]),
                    },
                    Level::Fields => Head::None,
                };
                Ok((Plan::Group { head, parts }, None))
            }
            TypeOwned::Enum(item_enum) => {
                // each item's variant (an index into the definition) and its tag
                let mut variant_of = Vec::with_capacity(items.len());
                let mut tags = Vec::with_capacity(items.len());
                for v in items {
                    let ValueOwned::Enum { variant, .. } = v else {
                        return Err(mismatch(v));
                    };
                    let idx = item_enum
                        .variants
                        .iter()
                        .position(|d| &d.ident == variant)
                        .ok_or_else(|| {
                            anyhow!(
                                "enum {} does not have variant {variant}, expected one of: {}",
                                item_enum.ident,
                                variant_names(item_enum)
                            )
                        })?;
                    variant_of.push(idx);
                    tags.push(item_enum.variants[idx].discriminant.0);
                }
                let mut parts = vec![Plan::Run(RunKind::Tag, tags.clone())];
                // the variants with items, in discriminant order
                let mut present: Vec<usize> = variant_of.clone();
                present.sort_by_key(|&idx| (item_enum.variants[idx].discriminant.0, idx));
                present.dedup();
                for idx in present {
                    let def = &item_enum.variants[idx];
                    let mut fields = vec![];
                    let mut rs = vec![];
                    for (i, v) in items.iter().enumerate() {
                        if variant_of[i] != idx {
                            continue;
                        }
                        let ValueOwned::Enum { fields: f, .. } = v else {
                            unreachable!("checked above")
                        };
                        fields.push(f);
                        // entering the variant from another one restarts it
                        rs.push(restarts[i] || (i > 0 && variant_of[i - 1] != idx));
                    }
                    let path = format!("{}::{}", item_enum.ident, def.ident);
                    parts.extend(self.struct_fields(&fields, &def.fields, &rs, &path)?);
                }
                Ok((
                    Plan::Group {
                        head: Head::None,
                        parts,
                    },
                    Some(tags),
                ))
            }
            TypeOwned::Instance { .. } | TypeOwned::Param(_) => Err(not_instantiated(ty)),
            other => Err(anyhow!(
                "{} is not in Series v1 (only as a #[series(opaque)] field)",
                other
                    .human_name(true, api_bundle)
                    .unwrap_or_else(|_| "this type".into())
            )),
        }
    }

    /// The fields of a struct or variant (flags left out: they belong to their `Option`), one plan each, from each
    /// item's field values: named ones by name (a missing one is written as plain serialization does), unnamed ones
    /// by position.
    fn struct_fields(
        &mut self,
        fields: &[&FieldsValueOwned],
        fields_def: &FieldsOwned,
        restarts: &[bool],
        path: &str,
    ) -> Result<Vec<Plan>> {
        let (_, all_defs) = fields_of(fields_def);
        let defs: Vec<&FieldOwned> = all_defs.iter().filter(|f| !f.is_flag()).collect();
        for f in fields {
            match (fields_def, f) {
                (FieldsOwned::Named(_), FieldsValueOwned::Named(values)) => {
                    check_field_names(values, all_defs, path)?
                }
                (FieldsOwned::Unnamed(_), FieldsValueOwned::Unnamed(values))
                    if values.len() == defs.len() => {}
                (FieldsOwned::Unit, FieldsValueOwned::Unit) => {}
                (_, f) => bail!("{path}: fields {f:?} don't match the definition"),
            }
        }
        let mut out = Vec::with_capacity(defs.len());
        // the restarts of the next field: the parent's, and where an enum or `Option` field before it changed
        let mut rs = restarts.to_vec();
        for (j, def) in defs.iter().enumerate() {
            let mut missing: Option<ValueOwned> = None;
            let mut column: Vec<Cow<ValueOwned>> = Vec::with_capacity(fields.len());
            for f in fields {
                let value = match f {
                    FieldsValueOwned::Named(values) => match find_field(values, def) {
                        Some(v) => Cow::Borrowed(v),
                        None => {
                            if missing.is_none() {
                                missing = Some(missing_field(def, path, self.api_bundle)?);
                            }
                            Cow::Owned(missing.clone().expect("set above"))
                        }
                    },
                    FieldsValueOwned::Unnamed(values) => Cow::Borrowed(&values[j]),
                    FieldsValueOwned::Unit => unreachable!("a unit has no fields"),
                };
                column.push(value);
            }
            let column: Vec<&ValueOwned> = column.iter().map(|v| v.as_ref()).collect();
            if def.series_opaque {
                out.push(self.opaque_field(&column, &def.ty)?);
                continue;
            }
            let (plan, tag) = self.group(&column, &def.ty, &rs, Level::Nested)?;
            out.push(plan);
            if let Some(tag) = tag {
                add_tag_restarts(&mut rs, &tag);
            }
        }
        Ok(out)
    }

    /// Tuple and array elements: the struct rule without a head.
    fn elements<'t>(
        &mut self,
        items: &[&Vec<ValueOwned>],
        types: impl Iterator<Item = &'t TypeOwned>,
        restarts: &[bool],
    ) -> Result<Vec<Plan>> {
        let mut out = vec![];
        let mut rs = restarts.to_vec();
        for (j, ty) in types.enumerate() {
            let column: Vec<&ValueOwned> = items.iter().map(|values| &values[j]).collect();
            let (plan, tag) = self.group(&column, ty, &rs, Level::Nested)?;
            out.push(plan);
            if let Some(tag) = tag {
                add_tag_restarts(&mut rs, &tag);
            }
        }
        Ok(out)
    }

    /// A field marked `#[series(opaque)]`: each value's own bytes (`to_ww_bytes`).
    fn opaque_field(&mut self, items: &[&ValueOwned], ty: &TypeOwned) -> Result<Plan> {
        if items.is_empty() {
            return Ok(Plan::empty());
        }
        self.values_left = self.values_left.checked_sub(items.len()).ok_or_else(|| {
            anyhow!("a Series of more than {ENCODE_MAX_VALUES} column values in all")
        })?;
        let api_bundle = self.api_bundle;
        let mut out: Vec<OpaqueItem> = Vec::with_capacity(items.len());
        for (i, value) in items.iter().enumerate() {
            // the same value as the item before: no need to write and read it again
            if i > 0 && std::ptr::eq(*value, items[i - 1]) {
                let prev = &out[i - 1];
                out.push(OpaqueItem {
                    bytes: prev.bytes.clone(),
                    value: prev.value.clone(),
                });
                continue;
            }
            let bytes = value.ser_shrink_wrap_dyn(ty, api_bundle)?;
            let value = ValueOwned::des_shrink_wrap_dyn(&bytes, ty, api_bundle)?;
            out.push(OpaqueItem { bytes, value });
        }
        Ok(Plan::Opaque(out))
    }
}

/// A numeric or float column of `values` (one per item of `restarts`), range-checked as plain serialization does.
fn numeric(values: &[&NumericValue], base: &NumericBaseType, restarts: &[bool]) -> Result<Plan> {
    use NumericBaseType as B;
    let out_of_range = |value: &NumericValue| anyhow!("{value:?} is out of range for {base:?}");
    let with_restarts = |bits: Vec<u64>| bits.into_iter().zip(restarts.iter().copied()).collect();
    macro_rules! ints {
        ($t:ty) => {
            values
                .iter()
                .map(|v| {
                    <$t>::try_from(int_value(v)?)
                        .map(|x| x as u64)
                        .map_err(|_| out_of_range(v))
                })
                .collect::<Result<Vec<u64>>>()?
        };
    }
    let (width, signed, bits) = match base {
        B::F32 => {
            let items = values
                .iter()
                .zip(restarts)
                .map(|(v, &r)| Ok((float_value(v)? as f32, r)))
                .collect::<Result<_>>()?;
            return Ok(Plan::F32(items));
        }
        B::F64 => {
            let items = values
                .iter()
                .zip(restarts)
                .map(|(v, &r)| Ok((float_value(v)?, r)))
                .collect::<Result<_>>()?;
            return Ok(Plan::F64(items));
        }
        B::U128 | B::I128 => {
            let signed = *base == B::I128;
            let items = values
                .iter()
                .map(|v| match (v, signed) {
                    (NumericValue::U128(x), false) => Ok(*x),
                    (_, false) => u128::try_from(int_value(v)?).map_err(|_| out_of_range(v)),
                    (_, true) => Ok(int_value(v)? as u128),
                })
                .collect::<Result<_>>()?;
            return Ok(Plan::Numeric128 { signed, items });
        }
        B::Nibble => {
            let bits = values
                .iter()
                .map(|v| match u8::try_from(int_value(v)?) {
                    Ok(x) if x <= 0xF => Ok(x as u64),
                    _ => Err(out_of_range(v)),
                })
                .collect::<Result<_>>()?;
            (4, false, bits)
        }
        B::U8 => (8, false, ints!(u8)),
        B::U16 => (16, false, ints!(u16)),
        B::U32 | B::UNib32 | B::UVlq32 => (32, false, ints!(u32)),
        B::U64 | B::UVlq64 => (64, false, ints!(u64)),
        // two's complement, sign-extended: the column takes the low bits
        B::I8 => (8, true, ints!(i8)),
        B::I16 => (16, true, ints!(i16)),
        B::I32 => (32, true, ints!(i32)),
        B::I64 => (64, true, ints!(i64)),
        B::UB(bits) => {
            let width = bits.0;
            let values = values
                .iter()
                .map(|v| match u64::try_from(int_value(v)?) {
                    Ok(x) if width >= 64 || x >> width == 0 => Ok(x),
                    _ => Err(out_of_range(v)),
                })
                .collect::<Result<_>>()?;
            (width, false, values)
        }
        B::IB(bits) => {
            let width = bits.0;
            let values = values
                .iter()
                .map(|v| {
                    let x = int_value(v)?;
                    let half = 1i128 << (width.clamp(1, 64) - 1);
                    if x < -half || x >= half {
                        return Err(out_of_range(v));
                    }
                    Ok(x as i64 as u64)
                })
                .collect::<Result<_>>()?;
            (width, true, values)
        }
        other => bail!("numeric type {other:?} is not supported in a Series"),
    };
    if !(1..=64).contains(&width) {
        bail!("numeric type {base:?} has no column");
    }
    Ok(Plan::Numeric {
        width,
        signed,
        items: with_restarts(bits),
    })
}
