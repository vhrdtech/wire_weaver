//! Worst-case serialized size of a type, computed at compile time (SW-19, the first step of API-18).
//!
//! [`SerializeShrinkWrap::MAX_SIZE`] is `Some` for a type whose every part is
//! bounded: numbers, `bool`, arrays, tuples, `Option`, `Result`, structs and enums made of them, and lists and strings
//! with a maximum length ([MaxLen](crate::MaxLen)). It is `None` for anything that can grow without a limit: a plain
//! `Vec`, `RefVec`, `String` or `&str`, `RefBox`, compressed sequences, `TailBytes`, and every type that does not set
//! it.
//!
//! [MaxSize::buffer_len] is then the length of a buffer that `to_ww_bytes` of any value of the type always fits into:
//! a bound of the serialized size, and also of the space the writer needs while serializing (lengths are kept as
//! `u16` at the back of the buffer until they are encoded). It is an upper bound, not the exact worst case: every
//! alignment that may be needed is counted as needed, every length as at least four nibbles.
//!
//! ```
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed)]
//! struct Piece<'i> {
//!     offset: u32,
//!     last: bool,
//!     data: MaxLen<RefVec<'i, u8>, 64>,
//! }
//!
//! const PIECE_MAX: usize = MaxSize::buffer_len_of(<Piece<'static> as SerializeShrinkWrap>::MAX_SIZE).unwrap();
//! let data = [0xAA; 64];
//! let piece = Piece { offset: 0, last: true, data: MaxLen::new(RefVec::new_bytes(&data)).unwrap() };
//! let mut buf = [0u8; PIECE_MAX];
//! let bytes = piece.to_ww_bytes(&mut buf).unwrap();
//! assert!(bytes.len() <= PIECE_MAX);
//! ```

use crate::nib32::UNib32;
use crate::{ElementSize, SerializeShrinkWrap};

/// Upper bound of the bits a value takes on the wire, see the [module docs](self).
///
/// Built with its `const fn`s, the way `ELEMENT_SIZE` is: [bits](Self::bits), [bytes](Self::bytes) and
/// [nibbles](Self::nibbles) for plain values, [seq](Self::seq) for fields one after another,
/// [one_of](Self::one_of) for enum variants, [repeat](Self::repeat) for arrays, [list](Self::list) for a bounded
/// list and [unsized_value](Self::unsized_value) for an `Unsized` value written as a field.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MaxSize {
    /// Bits written front to back, alignment padding inside the value included. The padding the value may need
    /// before it is not: [seq](Self::seq) adds it, from `align` and the `end_align` of what comes before.
    pub bits: usize,
    /// Nibbles of the lengths the value pushes to the back of the buffer (a list's or string's length, the size of
    /// a nested `Unsized` value), encoded when the enclosing `Unsized` value or the buffer is finished. Each one is
    /// counted as at least 4: while it waits to be encoded, it takes a `u16` slot.
    pub len_nibbles: usize,
    /// The alignment the value starts at, in bits: 1, 4 or 8. The writer pads to it first.
    pub align: u8,
    /// The alignment the value is known to end at, in bits, given that it started at `align`.
    pub end_align: u8,
}

impl MaxSize {
    /// Nothing at all, `()`.
    pub const ZERO: MaxSize = MaxSize {
        bits: 0,
        len_nibbles: 0,
        align: 1,
        end_align: 1,
    };

    /// `n` bits, bit-aligned: `bool`, `U1`..`U64`, `I2`..`I64`.
    pub const fn bits(n: usize) -> MaxSize {
        MaxSize {
            bits: n,
            len_nibbles: 0,
            align: 1,
            end_align: 1,
        }
    }

    /// `n` nibbles, nibble-aligned: `Nibble`, `UNib32`.
    pub const fn nibbles(n: usize) -> MaxSize {
        MaxSize {
            bits: n * 4,
            len_nibbles: 0,
            align: 4,
            end_align: 4,
        }
    }

    /// `n` bytes, byte-aligned: `u8`..`u128`, `i8`..`i128`, `f32`, `f64`, `UVlq32`.
    pub const fn bytes(n: usize) -> MaxSize {
        MaxSize {
            bits: n * 8,
            len_nibbles: 0,
            align: 8,
            end_align: 8,
        }
    }

    /// A length pushed to the back of the buffer, `max` the largest value it can have.
    pub const fn len_slot(max: usize) -> MaxSize {
        MaxSize {
            bits: 0,
            len_nibbles: len_slot_nibbles(max),
            align: 1,
            end_align: 1,
        }
    }

    /// Length of a buffer that a root value of this size (`to_ww_bytes`) always fits into, with its lengths
    /// encoded at the end.
    pub const fn buffer_len(&self) -> usize {
        self.finished_bits().div_ceil(8)
    }

    /// [buffer_len](Self::buffer_len) of a `MAX_SIZE`, `None` stays `None`.
    pub const fn buffer_len_of(max_size: Option<MaxSize>) -> Option<usize> {
        match max_size {
            Some(max_size) => Some(max_size.buffer_len()),
            None => None,
        }
    }

    /// Bits of the value with its lengths encoded after it, as `finish` and an `Unsized` value do: aligned to a
    /// nibble, a padding nibble, the lengths.
    const fn finished_bits(&self) -> usize {
        if self.len_nibbles == 0 {
            self.bits
        } else {
            self.bits.div_ceil(4) * 4 + 4 + self.len_nibbles * 4
        }
    }

    const fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

/// Nibbles counted for a length of at most `max`: its `UNib32` encoding, but at least the 4 of the `u16` slot it
/// takes until then.
const fn len_slot_nibbles(max: usize) -> usize {
    let max = if max > u32::MAX as usize {
        u32::MAX
    } else {
        max as u32
    };
    let nibbles = UNib32(max).len_nibbles();
    if nibbles < 4 { 4 } else { nibbles }
}

const fn max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

const fn min_u8(a: u8, b: u8) -> u8 {
    if a < b { a } else { b }
}

const fn max_u8(a: u8, b: u8) -> u8 {
    if a > b { a } else { b }
}

/// Bits of padding between a value that ends at `end_align` and one that starts at `align`, in the worst case.
const fn padding(end_align: u8, align: u8) -> usize {
    if end_align >= align {
        0
    } else {
        (align - end_align) as usize
    }
}

/// Composition, on `Option<MaxSize>` as `MAX_SIZE` is: `None` (unbounded) in, `None` out.
impl MaxSize {
    /// `a` followed by `b`: struct fields, a flag and the value behind it. `None` if either is unbounded.
    pub const fn seq(a: Option<MaxSize>, b: Option<MaxSize>) -> Option<MaxSize> {
        let (Some(a), Some(b)) = (a, b) else {
            return None;
        };
        let len_nibbles = a.len_nibbles + b.len_nibbles;
        if b.is_empty() {
            return Some(MaxSize { len_nibbles, ..a });
        }
        if a.is_empty() {
            return Some(MaxSize { len_nibbles, ..b });
        }
        Some(MaxSize {
            bits: a.bits + padding(a.end_align, b.align) + b.bits,
            len_nibbles,
            align: a.align,
            end_align: b.end_align,
        })
    }

    /// Either `a` or `b`, both starting at the same place: enum variants, `Ok` or `Err`. `None` if either is unbounded.
    pub const fn one_of(a: Option<MaxSize>, b: Option<MaxSize>) -> Option<MaxSize> {
        let (Some(a), Some(b)) = (a, b) else {
            return None;
        };
        let len_nibbles = max(a.len_nibbles, b.len_nibbles);
        if a.is_empty() && b.is_empty() {
            return Some(MaxSize { len_nibbles, ..a });
        }
        // padded to the larger alignment in front, so an empty one ends there
        let align = max_u8(a.align, b.align);
        let end_a = if a.is_empty() { align } else { a.end_align };
        let end_b = if b.is_empty() { align } else { b.end_align };
        Some(MaxSize {
            bits: max(a.bits, b.bits),
            len_nibbles,
            align,
            end_align: min_u8(end_a, end_b),
        })
    }

    /// `count` values one after another: an array `[T; count]`, the elements of a list.
    pub const fn repeat(a: Option<MaxSize>, count: usize) -> Option<MaxSize> {
        let Some(a) = a else {
            return None;
        };
        if count == 0 {
            return Some(MaxSize::ZERO);
        }
        if a.is_empty() {
            return Some(MaxSize {
                len_nibbles: a.len_nibbles * count,
                ..a
            });
        }
        Some(MaxSize {
            bits: a.bits * count + padding(a.end_align, a.align) * (count - 1),
            len_nibbles: a.len_nibbles * count,
            align: a.align,
            end_align: a.end_align,
        })
    }

    /// A list of at most `max_len` elements of size `item`: its length, then the elements.
    pub const fn list(item: Option<MaxSize>, max_len: usize) -> Option<MaxSize> {
        MaxSize::seq(
            Some(MaxSize::len_slot(max_len)),
            MaxSize::repeat(item, max_len),
        )
    }

    /// An `Unsized` value of size `inner` written as a field ([BufWriter::write](crate::BufWriter::write)): byte-aligned,
    /// its own lengths encoded at its end, and its size in bytes pushed to the parent's lengths.
    pub const fn unsized_value(inner: Option<MaxSize>) -> Option<MaxSize> {
        let Some(inner) = inner else {
            return None;
        };
        let bytes = inner.buffer_len();
        Some(MaxSize {
            bits: bytes * 8,
            len_nibbles: len_slot_nibbles(bytes),
            align: 8,
            end_align: 8,
        })
    }

    /// `MAX_SIZE` of `T` written as a field with [BufWriter::write](crate::BufWriter::write): an `Unsized` `T` gets its
    /// size slot, see [unsized_value](Self::unsized_value). What a `MAX_SIZE` of a type is made of.
    pub const fn of_write<T: SerializeShrinkWrap + ?Sized>() -> Option<MaxSize> {
        if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            MaxSize::unsized_value(T::MAX_SIZE)
        } else {
            T::MAX_SIZE
        }
    }
}

/// [MaxSize::of_write], as a free function next to [write_pushes_len](crate::write_pushes_len).
pub const fn write_max_size<T: SerializeShrinkWrap + ?Sized>() -> Option<MaxSize> {
    MaxSize::of_write::<T>()
}
