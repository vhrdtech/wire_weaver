//! Bit layout of types, to check that fields added in between the old ones only take bits that were padding before.
//!
//! Padding inside a type (e.g. the bits before a byte-aligned `u8` that follows a `bool`) is always written as zeroes
//! and skipped when reading. A new field that fits into such padding, without moving any of the old fields, is read as
//! all zeroes from old data and skipped by old readers. Where padding is depends on the bit offset a type starts at:
//! `Unsized` types always start at a byte boundary, `FinalStructure`, `SelfDescribing` and `Sized` ones are packed
//! tightly into their parent and can start at any bit, so all the offsets are checked for them. Only `Unsized` types
//! can also grow at the end, which is not checked here.
//!
//! Fields are flattened down to leaves: nested in-line non-`Unsized` structs and arrays of fixed size elements are
//! expanded. Anything of variable size (e.g. `UNib32`, an enum with data, `Option`, an `Unsized` type) or unknown
//! (e.g. a type skipped from another crate) must be at the same position on both sides and can't be added.

use std::collections::HashMap;

use wire_weaver::shrink_wrap::ElementSize;
use ww_numeric::NumericAnyTypeOwned;
use ww_self::{
    ApiBundleOwned, FieldOwned, FieldsOwned, NumericBaseType, Repr, TypeLocationOwned, TypeOwned,
};

/// Arrays longer than this are not expanded, but treated as one variable size leaf.
const MAX_EXPANDED_ARRAY_LEN: u32 = 1024;

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Fixed {
        /// Alignment in bits before the value.
        align: u32,
        bits: u32,
        /// All zero bits are a valid value.
        zero_ok: bool,
    },
    Variable {
        align: u32,
        /// Size is a multiple of this many bits.
        granularity: u32,
    },
}

#[derive(Debug)]
struct Leaf {
    /// Path of the leaf through fields and array elements, e.g. `speed.limits[1]`.
    path: String,
    kind: Kind,
}

const UNKNOWN: Kind = Kind::Variable {
    align: 1,
    granularity: 1,
};

/// Checks that `more` is `fewer` with new fields put into its unused padding bits. Old fields are matched by name.
///
/// `byte_aligned`: the type always starts at a byte boundary (`Unsized` ones do), otherwise it can start at any bit.
pub(crate) fn check_padding_reuse(
    fewer_bundle: &ApiBundleOwned,
    fewer: &[FieldOwned],
    more_bundle: &ApiBundleOwned,
    more: &[FieldOwned],
    byte_aligned: bool,
) -> Result<(), String> {
    let (mut old, mut new) = (vec![], vec![]);
    flatten(fewer_bundle, fewer, "", &mut old, 0);
    flatten(more_bundle, more, "", &mut new, 0);

    let variable = |leaves: &[Leaf]| -> Vec<(String, Kind)> {
        leaves
            .iter()
            .filter(|l| matches!(l.kind, Kind::Variable { .. }))
            .map(|l| (l.path.clone(), l.kind.clone()))
            .collect()
    };
    if variable(&old) != variable(&new) {
        return Err(
            "only fixed size fields can be added into unused padding bits, variable size ones must stay the same"
                .into(),
        );
    }
    for leaf in &old {
        if !new
            .iter()
            .any(|l| l.path == leaf.path && l.kind == leaf.kind)
        {
            return Err(format!("field `{}` removed or changed", leaf.path));
        }
    }

    // variable size leaves split the fields into segments, each is laid out from a start offset in its own set
    let (old_segments, new_segments) = (segments(&old), segments(&new));
    let mut start_offsets: Vec<u32> = if byte_aligned {
        vec![0]
    } else {
        (0..8).collect()
    };
    for (i, (old_segment, new_segment)) in old_segments.iter().zip(&new_segments).enumerate() {
        let next = old_segments.get(i + 1).and_then(|s| s.first());
        let end_align = match next {
            Some(Leaf {
                kind: Kind::Variable { align, .. },
                ..
            }) => *align,
            _ => 1,
        };
        for &start in &start_offsets {
            compare_segment(old_segment, new_segment, start, end_align)?;
        }
        if let Some(Leaf {
            kind: Kind::Variable { align, granularity },
            ..
        }) = next
        {
            let step = gcd(gcd(*align, *granularity), 8);
            start_offsets = (0..8).step_by(step as usize).collect();
        }
    }
    Ok(())
}

/// If all fields of `fewer` are named and are in `more` in the same order, returns the index in `more` of the last one.
pub(crate) fn last_old_field(fewer: &[FieldOwned], more: &[FieldOwned]) -> Option<usize> {
    let mut last = None;
    let mut from = 0;
    for field in fewer {
        field.ident.as_ref()?;
        let idx = from
            + more[from..]
                .iter()
                .position(|f| f.ident == field.ident && f.is_flag() == field.is_flag())?;
        last = Some(idx);
        from = idx + 1;
    }
    last
}

/// Leaves split before each variable size one, which is then the first leaf of its segment.
fn segments(leaves: &[Leaf]) -> Vec<&[Leaf]> {
    let mut segments = vec![];
    let mut start = 0;
    for (i, leaf) in leaves.iter().enumerate() {
        if matches!(leaf.kind, Kind::Variable { .. }) {
            segments.push(&leaves[start..i]);
            start = i;
        }
    }
    segments.push(&leaves[start..]);
    segments
}

fn compare_segment(old: &[Leaf], new: &[Leaf], start: u32, end_align: u32) -> Result<(), String> {
    let (old_positions, old_end) = lay_out(old, start, end_align);
    let (new_positions, new_end) = lay_out(new, start, end_align);
    let old_by_path: HashMap<&str, (u32, u32)> = old
        .iter()
        .zip(&old_positions)
        .map(|(leaf, pos)| (leaf.path.as_str(), *pos))
        .collect();
    for (leaf, (leaf_start, leaf_end)) in new.iter().zip(&new_positions) {
        if let Some(old_pos) = old_by_path.get(leaf.path.as_str()) {
            if *old_pos != (*leaf_start, *leaf_end) {
                return Err(format!(
                    "field `{}` moved from bit {} to {} (for a type starting at bit offset {start})",
                    leaf.path, old_pos.0, leaf_start
                ));
            }
            continue;
        }
        if !matches!(leaf.kind, Kind::Fixed { zero_ok: true, .. }) {
            return Err(format!(
                "new field `{}` can't be read from all zero bits",
                leaf.path
            ));
        }
        let overlaps = old_positions
            .iter()
            .any(|(s, e)| *leaf_start < *e && *s < *leaf_end);
        if overlaps || *leaf_end > old_end {
            return Err(format!(
                "new field `{}` doesn't fit into unused padding bits (for a type starting at bit offset {start})",
                leaf.path
            ));
        }
    }
    if old_end != new_end {
        return Err(format!(
            "size changed (for a type starting at bit offset {start})"
        ));
    }
    Ok(())
}

/// (start, end) bit positions of each leaf, and the end of the segment. A variable size leaf is only at the start
/// of a segment, and it's zero length here: it's the same on both sides.
fn lay_out(leaves: &[Leaf], start: u32, end_align: u32) -> (Vec<(u32, u32)>, u32) {
    let mut pos = start;
    let mut positions = vec![];
    for leaf in leaves {
        match leaf.kind {
            Kind::Fixed { align, bits, .. } => {
                pos = pos.next_multiple_of(align);
                positions.push((pos, pos + bits));
                pos += bits;
            }
            Kind::Variable { .. } => positions.push((pos, pos)),
        }
    }
    (positions, pos.next_multiple_of(end_align))
}

fn flatten(
    bundle: &ApiBundleOwned,
    fields: &[FieldOwned],
    prefix: &str,
    out: &mut Vec<Leaf>,
    depth: u32,
) {
    for (i, field) in fields.iter().enumerate() {
        let name = match &field.ident {
            Some(ident) => ident.clone(),
            None => i.to_string(),
        };
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}.{name}")
        };
        // a relocated flag has the same name as its field
        let path = if field.is_flag() {
            format!("{path} flag")
        } else {
            path
        };
        leaf(bundle, &field.ty, path, out, depth);
    }
}

fn leaf(bundle: &ApiBundleOwned, ty: &TypeOwned, path: String, out: &mut Vec<Leaf>, depth: u32) {
    // self-referential types are not fixed size anyway
    if depth > 16 {
        out.push(Leaf {
            path,
            kind: UNKNOWN,
        });
        return;
    }
    let kind = match ty {
        TypeOwned::Bool | TypeOwned::Flag => Kind::Fixed {
            align: 1,
            bits: 1,
            zero_ok: true,
        },
        TypeOwned::NumericAny(n) => numeric(n),
        TypeOwned::OutOfLine { type_idx } => match bundle.types.get(type_idx.0 as usize) {
            Some(TypeLocationOwned::InLine { ty, .. }) => {
                return leaf(bundle, ty, path, out, depth + 1);
            }
            _ => UNKNOWN,
        },
        TypeOwned::Struct(s) if !matches!(s.size, ElementSize::Unsized) => {
            let fields = match &s.fields {
                FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => fields.as_slice(),
                FieldsOwned::Unit => &[],
            };
            flatten(bundle, fields, &path, out, depth + 1);
            return;
        }
        TypeOwned::Enum(e)
            if e.variants
                .iter()
                .all(|v| matches!(v.fields, FieldsOwned::Unit)) =>
        {
            match repr(&e.repr) {
                Kind::Fixed { align, bits, .. } => Kind::Fixed {
                    align,
                    bits,
                    zero_ok: e.variants.iter().any(|v| v.discriminant.0 == 0),
                },
                variable => variable,
            }
        }
        TypeOwned::Enum(e) => match repr(&e.repr) {
            Kind::Fixed { align, .. } | Kind::Variable { align, .. } => Kind::Variable {
                align,
                granularity: 1,
            },
        },
        TypeOwned::Array { len, ty } if len.0 <= MAX_EXPANDED_ARRAY_LEN => {
            for i in 0..len.0 {
                leaf(bundle, ty, format!("{path}[{i}]"), out, depth + 1);
            }
            return;
        }
        _ => UNKNOWN,
    };
    out.push(Leaf { path, kind });
}

fn numeric(n: &NumericAnyTypeOwned) -> Kind {
    let (base, zero_ok) = match n {
        NumericAnyTypeOwned::Base(base) => (base, true),
        NumericAnyTypeOwned::ShiftScale { base, .. } => (base, true),
        // zero might be outside the valid range or list
        NumericAnyTypeOwned::SubType { base, .. } => (base, false),
    };
    let (align, bits) = match base {
        NumericBaseType::Nibble => (4, 4),
        NumericBaseType::U8 | NumericBaseType::I8 => (8, 8),
        NumericBaseType::U16 | NumericBaseType::I16 => (8, 16),
        NumericBaseType::U32 | NumericBaseType::I32 | NumericBaseType::F32 => (8, 32),
        NumericBaseType::U64 | NumericBaseType::I64 | NumericBaseType::F64 => (8, 64),
        NumericBaseType::U128 | NumericBaseType::I128 => (8, 128),
        NumericBaseType::UB(bits) => (1, bits.0 as u32),
        NumericBaseType::IB(bits) => (1, bits.0 as u32),
        NumericBaseType::UNib32 => {
            return Kind::Variable {
                align: 4,
                granularity: 4,
            };
        }
        _ => return UNKNOWN,
    };
    Kind::Fixed {
        align,
        bits,
        zero_ok,
    }
}

fn repr(repr: &Repr) -> Kind {
    let (align, bits) = match repr {
        Repr::Nibble => (4, 4),
        Repr::BitAligned(bits) => (1, *bits as u32),
        Repr::UNib32 => {
            return Kind::Variable {
                align: 4,
                granularity: 4,
            };
        }
        Repr::ByteAlignedU8 => (8, 8),
        Repr::ByteAlignedU16 => (8, 16),
        Repr::ByteAlignedU32 => (8, 32),
    };
    Kind::Fixed {
        align,
        bits,
        zero_ok: true,
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}
