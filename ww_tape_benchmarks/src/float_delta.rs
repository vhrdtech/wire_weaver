//! Delta on floats, bench-only: each value's bits mapped to an ordered integer (a negative value's bits inverted, a
//! positive one's sign bit set), then ww_tape's `DELTA` or `DELTA2` on those integers. Not part of the ww_tape
//! format (ids 34 and 35 are reserved there); the bytes are those the removed `FLOAT_DELTA` codecs wrote.

/// A float's bits as an ordered integer of `bits` bits: negative values flipped, positive ones above them.
pub fn to_ordered(v: u64, bits: u32) -> u64 {
    let top = 1u64 << (bits - 1);
    if v & top != 0 {
        !v & mask(bits)
    } else {
        v | top
    }
}

/// The float bits of an ordered integer of `bits` bits.
pub fn from_ordered(v: u64, bits: u32) -> u64 {
    let top = 1u64 << (bits - 1);
    if v & top != 0 {
        v & !top
    } else {
        !v & mask(bits)
    }
}

const fn mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}
