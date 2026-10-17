//! Worst-case sizes of [SeriesColumns](super::SeriesColumns) groups (SW-62): what
//! [MAX_ITEM_BITS](super::SeriesColumns::MAX_ITEM_BITS) and [MAX_FIXED_BITS](super::SeriesColumns::MAX_FIXED_BITS)
//! are made of, for the impls here, the derive and hand-written ones. `None` is unbounded (a string, an opaque
//! field), and stays `None` through every operation; so does an overflow.
//!
//! The bounds hold for every [CodecPolicy](super::CodecPolicy): a fixed codec can be worse than writing a column
//! plain (the packed coder over blocks of one residual, the prefix code at its longest), so they are the worst of
//! all codecs, not of the one `SMALLEST` would pick. A run is bounded per item by a change at every item: a change
//! after a gap of `g` items costs `1 + nib(g)` plus the value, spread over `g + 1` items, which is never more than
//! a change at every item (`nib(0)` is 4 bits).

/// `a + b`.
pub const fn add(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(a), Some(b)) => a.checked_add(b),
        _ => None,
    }
}

/// The larger of `a` and `b`.
pub const fn max(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a > b { a } else { b }),
        _ => None,
    }
}

/// `a × k`.
pub const fn times(a: Option<usize>, k: usize) -> Option<usize> {
    match a {
        Some(a) => a.checked_mul(k),
        None => None,
    }
}

/// Bits of `nib(value)`: 4 per group of 3 bits.
pub const fn nib_bits(value: u32) -> usize {
    let mut groups = 1;
    let mut value = value >> 3;
    while value != 0 {
        groups += 1;
        value >>= 3;
    }
    4 * groups
}

/// The 4-bit header of a numeric or float column.
pub const COLUMN_HEADER_BITS: usize = 4;

/// What an evolvable struct writes before its fields: its field count (outermost) or group length (nested), a
/// `nib` of at most 11 groups.
pub const STRUCT_HEAD_BITS: usize = 44;

/// An item of a numeric column of `width` bits (1 to 64): a segment start at the width, a residual in the prefix
/// code (a `1` per bucket below the width, the width), or a packed block of one residual (the block width field
/// and the residual), whichever is the longest.
pub const fn numeric_item_bits(width: u8) -> usize {
    // the prefix code: a 1, then a 1 for each bucket of 7, 9, 12, 20, 32 bits below the width
    let buckets = [7u8, 9, 12, 20, 32];
    let mut prefix = 1 + width as usize;
    let mut k = 0;
    while k < buckets.len() {
        if buckets[k] < width {
            prefix += 1;
        }
        k += 1;
    }
    let packed = width as usize + (8 - width.leading_zeros() as usize);
    if prefix > packed { prefix } else { packed }
}

/// An item of a 128-bit numeric column: always raw + plain.
pub const NUMERIC128_ITEM_BITS: usize = 128;

/// An item of a float column of `bits` (32, 64) with `lead_bits` (5, 6) in the XOR code: `11`, the leading zero
/// count, the length and up to every bit.
pub const fn float_item_bits(bits: u8, lead_bits: u8) -> usize {
    2 + 2 * lead_bits as usize + bits as usize
}

/// The first value and the end bit of a bool run.
pub const BOOL_RUN_FIXED_BITS: usize = 2;

/// A change of a bool run (`1`, `nib(gap)`, no value).
pub const BOOL_RUN_ITEM_BITS: usize = 5;

/// The first value (`nib`) and the end bit of an `Option`'s tag run.
pub const OPTION_TAG_FIXED_BITS: usize = 5;

/// A change of an `Option`'s tag run (`1`, `nib(gap)`, no value).
pub const OPTION_TAG_ITEM_BITS: usize = 5;

/// The first value and the end bit of an enum's tag run, discriminants up to `max_tag`.
pub const fn tag_run_fixed_bits(max_tag: u32) -> usize {
    nib_bits(max_tag) + 1
}

/// A change of an enum's tag run, discriminants up to `max_tag`: `1`, `nib(gap)`, `nib(tag)`.
pub const fn tag_run_item_bits(max_tag: u32) -> usize {
    1 + nib_bits(0) + nib_bits(max_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_widths() {
        // packed: width + bits of the block width field; prefix: 1 + buckets below + width
        assert_eq!(numeric_item_bits(1), 2);
        assert_eq!(numeric_item_bits(8), 12);
        assert_eq!(numeric_item_bits(16), 21);
        assert_eq!(numeric_item_bits(32), 38);
        assert_eq!(numeric_item_bits(64), 71);
        assert_eq!(float_item_bits(32, 5), 44);
        assert_eq!(float_item_bits(64, 6), 78);
    }

    #[test]
    fn nib_and_options() {
        assert_eq!(nib_bits(0), 4);
        assert_eq!(nib_bits(7), 4);
        assert_eq!(nib_bits(8), 8);
        assert_eq!(nib_bits(u32::MAX), 44);
        assert_eq!(add(Some(1), Some(2)), Some(3));
        assert_eq!(add(Some(1), None), None);
        assert_eq!(add(Some(usize::MAX), Some(1)), None);
        assert_eq!(max(Some(4), Some(2)), Some(4));
        assert_eq!(times(Some(3), 4), Some(12));
        assert_eq!(times(None, 0), None);
    }
}
