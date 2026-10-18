//! Which bits of a buffer hold which value (TOOL-9): the dynamic decoder of [value](crate::value) run with a trace,
//! every value it reads recorded with its place in the buffer, for `ww explain` and later a GUI.
//!
//! A value takes bits from the front of the buffer and, for lengths and sizes, nibbles from the back (the FIFO of
//! sizes), so each [Part] has both ranges. Parts are in the order they were read, a tree by [Part::depth]: a struct
//! is followed by its fields, an enum by its discriminant and its variant's fields, a `Vec` by its length and items.

use crate::value::{Budget, des_field, type_name};
use crate::{ApiBundleOwned, Repr, TypeOwned, ValueOwned};
use shrink_wrap::BufReader;
use shrink_wrap::buf_reader::Cursors;
use std::ops::Range;
use std::rc::Rc;
use ww_numeric::NumericBaseType;

/// A buffer decoded with the place of every value in it.
#[derive(Debug)]
pub struct Explained {
    /// What was read, in order, as a tree by [Part::depth]; up to the failure when the decode failed.
    pub parts: Vec<Part>,
    /// The decoded value (one per type for [explain_fields]), or why the decode failed.
    pub result: anyhow::Result<Vec<ValueOwned>>,
    /// Size of the buffer in bytes.
    pub len: usize,
}

/// One thing read from the buffer: a value, or a length, discriminant or flag that belongs to one.
#[derive(Clone, Debug)]
pub struct Part {
    /// 0 for a top-level value, its fields 1, and so on.
    pub depth: usize,
    /// Field name, `[i]` of an item, `some` / `ok` / `err` inside an `Option` or `Result`, empty for a top-level
    /// value.
    pub label: String,
    pub kind: PartKind,
    /// Rust-like name of the type, e.g. `u8`, `Vec<Sample>`, `Mode`.
    pub ty: String,
    /// Bits read from the front, counted from bit 7 of byte 0.
    pub front: Range<usize>,
    /// Nibbles read from the back (lengths and sizes): nibble `2k` is bits 7:4 of byte `k`, `2k + 1` its bits 3:0.
    pub back: Range<usize>,
    /// The value, for a part with no parts inside it (a number, a string, a flag).
    pub value: Option<ValueOwned>,
    /// The variant of an enum, or the error the decode stopped at inside this part.
    pub note: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PartKind {
    Value,
    /// Length of a `Vec` or a string, or the size of an unsized value (evolvable struct or enum, `Box`).
    Size,
    /// Which variant of an enum.
    Discriminant,
    /// `Some` / `Ok` (1) or `None` / `Err` (0).
    Flag,
    /// Bits skipped to align the next value to a nibble or a byte.
    Padding,
}

/// Decode `bytes` as one top-level value of type `ty`, as
/// [des_shrink_wrap_dyn](ValueOwned::des_shrink_wrap_dyn) does, recording the place of every value.
pub fn explain(bytes: &[u8], ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> Explained {
    run(bytes, |rd, b| {
        crate::value::des_inner(rd, ty, api_bundle, b).map(|v| vec![v])
    })
}

/// Decode `bytes` as the fields of an evolvable struct (method arguments), as
/// [des_shrink_wrap_vec_dyn](ValueOwned::des_shrink_wrap_vec_dyn) does: one value per `(name, type)`.
pub fn explain_fields<'t>(
    bytes: &[u8],
    fields: impl Iterator<Item = (&'t str, &'t TypeOwned)>,
    api_bundle: &ApiBundleOwned,
) -> Explained {
    run(bytes, |rd, b| {
        fields
            .map(|(name, ty)| {
                b.label(name);
                des_field(rd, ty, api_bundle, b)
            })
            .collect()
    })
}

fn run(
    bytes: &[u8],
    decode: impl FnOnce(&mut BufReader, &mut Budget) -> anyhow::Result<Vec<ValueOwned>>,
) -> Explained {
    // the reader and the trace share one allocation: reader positions map back to it by address
    let base: Rc<[u8]> = bytes.into();
    let mut b = Budget::for_bytes(bytes.len());
    b.trace = Some(Box::new(Trace::new(base.clone())));
    let mut rd = BufReader::new(&base);
    let result = decode(&mut rd, &mut b);
    let trace = b.trace.take().expect("trace is set above");
    Explained {
        parts: trace.parts,
        result,
        len: bytes.len(),
    }
}

/// What [Budget] carries during [explain].
#[derive(Debug)]
pub(crate) struct Trace {
    base: Rc<[u8]>,
    parts: Vec<Part>,
    depth: usize,
    /// Label of the next value, set by its container.
    label: Option<String>,
    /// Name of the next value's type, set by a wrapper (out-of-line reference, `Box`, max length) that is not a
    /// part of its own.
    ty: Option<String>,
}

/// An open [Part], see [Trace::enter].
pub(crate) struct Open {
    idx: usize,
    before: Cursors,
    /// The value is read at a multiple of this many bits (1, 4 or 8), see [alignment].
    align: usize,
}

impl Trace {
    fn new(base: Rc<[u8]>) -> Self {
        Trace {
            base,
            parts: vec![],
            depth: 0,
            label: None,
            ty: None,
        }
    }

    fn cursors(&self, rd: &BufReader) -> Option<Cursors> {
        rd.cursors_in(&self.base)
    }

    /// Label of the next value; joined to a label still waiting (`mode.some`: an `Option` field whose flag was
    /// read before it).
    pub(crate) fn label(&mut self, label: impl Into<String>) {
        let label = label.into();
        self.label = Some(match self.label.take() {
            Some(waiting) => format!("{waiting}.{label}"),
            None => label,
        });
    }

    /// A value of type `ty` starts: a part of its own, or for a wrapper only its type name for the next one.
    pub(crate) fn enter(
        &mut self,
        rd: &BufReader,
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Option<Open> {
        let before = self.cursors(rd)?;
        let name = || self.ty.clone().unwrap_or_else(|| type_name(ty, api_bundle));
        if matches!(
            ty,
            TypeOwned::OutOfLine { .. } | TypeOwned::Box(_) | TypeOwned::MaxLen { .. }
        ) {
            self.ty = Some(name());
            return None;
        }
        let part = Part {
            depth: self.depth,
            label: self.label.take().unwrap_or_default(),
            kind: PartKind::Value,
            ty: name(),
            front: before.front_bit..before.front_bit,
            back: before.back_nibble..before.back_nibble,
            value: None,
            note: None,
        };
        self.ty = None;
        self.parts.push(part);
        self.depth += 1;
        Some(Open {
            idx: self.parts.len() - 1,
            before,
            align: ty
                .get_in_line(api_bundle)
                .map_or(1, |ty| alignment(ty, api_bundle)),
        })
    }

    pub(crate) fn leave(
        &mut self,
        open: Option<Open>,
        rd: &BufReader,
        result: &anyhow::Result<ValueOwned>,
    ) {
        let Some(Open { idx, before, align }) = open else {
            return;
        };
        self.depth -= 1;
        let after = self.cursors(rd).unwrap_or(before);
        let leaf = idx + 1 == self.parts.len();
        let mut start = before.front_bit;
        if leaf && result.is_ok() {
            start = self.padding(start, align, after.front_bit, idx);
        }
        let idx = if start != before.front_bit {
            idx + 1
        } else {
            idx
        };
        let part = &mut self.parts[idx];
        part.front = start..after.front_bit.max(start);
        part.back = after.back_nibble.min(before.back_nibble)..before.back_nibble;
        match result {
            Ok(ValueOwned::Enum { variant, .. }) => part.note = Some(variant.clone()),
            Ok(value) if leaf => part.value = Some(value.clone()),
            Ok(_) => {}
            Err(e) => part.note = Some(format!("{e:#}")),
        }
    }

    /// A length, discriminant or flag read between `before` and `rd`, inside the value read now.
    pub(crate) fn part(
        &mut self,
        before: Option<Cursors>,
        rd: &BufReader,
        kind: PartKind,
        label: &str,
        value: ValueOwned,
        align: usize,
    ) {
        let (Some(before), Some(after)) = (before, self.cursors(rd)) else {
            return;
        };
        let start = self.padding(before.front_bit, align, after.front_bit, self.parts.len());
        let ty = match &value {
            ValueOwned::Bool(_) => "bool".to_string(),
            ValueOwned::Numeric(n) => format!("{:?}", n.ty()).to_lowercase(),
            _ => String::new(),
        };
        self.parts.push(Part {
            depth: self.depth,
            label: label.to_string(),
            kind,
            ty,
            front: start..after.front_bit,
            back: after.back_nibble..before.back_nibble,
            value: Some(value),
            note: None,
        });
    }

    /// A [Padding](PartKind::Padding) part at `idx` for the bits from `start` up to the next multiple of `align`,
    /// when the value read up to `end` starts there; where the value starts.
    fn padding(&mut self, start: usize, align: usize, end: usize, idx: usize) -> usize {
        let aligned = start.next_multiple_of(align);
        if aligned == start || aligned > end {
            return start;
        }
        let padding = Part {
            depth: self.depth,
            label: "padding".to_string(),
            kind: PartKind::Padding,
            ty: String::new(),
            front: start..aligned,
            back: 0..0,
            value: None,
            note: None,
        };
        self.parts.insert(idx, padding);
        aligned
    }

    pub(crate) fn before(&self, rd: &BufReader) -> Option<Cursors> {
        self.cursors(rd)
    }
}

/// Bits a value of `ty` (in line) is aligned to when read: 8 for byte-aligned numbers and strings, 4 for nibbles,
/// 1 for bits and for values made of other values (their parts align themselves).
fn alignment(ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> usize {
    match ty {
        TypeOwned::NumericAny(n) => base_alignment(crate::value::numeric_base(n)),
        TypeOwned::Range(base) | TypeOwned::RangeInclusive(base) => base_alignment(base),
        TypeOwned::String => 8,
        TypeOwned::MaxLen { ty, .. } | TypeOwned::Box(ty) => ty
            .get_in_line(api_bundle)
            .map_or(1, |ty| alignment(ty, api_bundle)),
        _ => 1,
    }
}

fn base_alignment(base: &NumericBaseType) -> usize {
    match base {
        NumericBaseType::UB(_) | NumericBaseType::IB(_) => 1,
        NumericBaseType::Nibble | NumericBaseType::UNib32 => 4,
        _ => 8,
    }
}

/// Bits an enum's discriminant is aligned to.
pub(crate) fn repr_alignment(repr: &Repr) -> usize {
    match repr {
        Repr::BitAligned(_) => 1,
        Repr::Nibble | Repr::UNib32 => 4,
        Repr::ByteAlignedU8 | Repr::ByteAlignedU16 | Repr::ByteAlignedU32 => 8,
    }
}
