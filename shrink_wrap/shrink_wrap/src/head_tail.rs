//! Fixed head, sequential tail (SW-46): a size-first encoding for types with lists, strings and options.
//!
//! The [fixed layout](crate::fixed_layout) puts every field at a constant offset, which only works while no field
//! before the last has a variable length. `#[derive_shrink_wrap(head_tail)]` splits a value in two instead:
//!
//! - the **head**, of a length known at compile time: every fixed-width field at a constant offset, as in the
//!   fixed layout (numbers little-endian on a byte, bit fields packed most significant bit first), plus, for each
//!   variable field, what is fixed about it: a list or string has its count there (`u16`, little-endian), an
//!   `Option` or `Result` its flag bit (where `#[flag]` puts it, as usual), a nested `head_tail` or fixed-layout
//!   type its own head (starting and ending on a byte);
//! - the **tail**, right after the head: the variable parts in field order: list items (each a fixed-layout value
//!   of one length, so the list is `count * ITEM_LEN` bytes), string bytes, an `Option` payload when the flag is
//!   set (its head, then its tail), the tail of a nested type.
//!
//! An enum is its tag (`ww_repr = u8`, `u16` or `u32`) as the head, then the head and tail of the variant as the
//! tail.
//!
//! Reading a field of the head is a load at a constant offset. The tail is read once, front to back, by a cursor
//! that only ever takes a number of bytes it already knows ([Tail]): no lengths at the back, no `BufReader`, no
//! per-type generic reader. Nothing in the head is written twice, and a reader that wants one field never has to
//! look at the others.
//!
//! This is a wire format of its own, not shrink_wrap's: a `head_tail` type has no `SerializeShrinkWrap` /
//! `DeserializeShrinkWrap`, only [HeadTail] and the inherent `from_head_tail` / `to_head_tail` /
//! `head_tail_len`. Lists are [List] (not `RefVec`, whose items are read through a `BufReader`), strings
//! `&str`. A type is either `head_tail` or not: nothing is decided from the values.
//!
//! What a `head_tail` type may hold:
//!
//! - numbers, arrays of numbers and bit fields, as in the fixed layout;
//! - `&str` and `List<'i, T>` anywhere, `T` a number or a fixed-layout type of one length (`SAME_LEN`);
//! - `Option<T>` and `Result<T, E>` anywhere, `T` / `E` a number, an array of numbers or a type implementing
//!   [HeadTail];
//! - other types implementing [HeadTail]: other `head_tail` types and every fixed-layout type (the derive
//!   implements it for them: one of length `SAME_LEN` is all head, any other all tail).
//!
//! Not: `Vec` / `RefVec` (use [List]), varints, `RefBox`, `TailSize`, tuples, `#[default]`, owned (`std`)
//! versions of the type.
//!
//! ```
//! use shrink_wrap::prelude::*;
//! use shrink_wrap::head_tail::List;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
//! struct Wave {
//!     line: u8,
//!     idle: bool,
//!     delay: u32,
//! }
//!
//! #[derive_shrink_wrap(head_tail, derive(Debug, PartialEq))]
//! struct Load<'i> {
//!     off: u16,
//!     defer: bool,
//!     glitch: Option<Wave>,
//!     frames: List<'i, u16>,
//! }
//!
//! // head: off (2), defer and glitch's flag (1 byte of bits), the count of frames (2)
//! assert_eq!(Load::HEAD_LEN, 5);
//! assert_eq!(Load::FRAMES_OFFSET, 3);
//! let v = Load { off: 7, defer: true, glitch: None, frames: List::Slice(&[1, 2]) };
//! let mut buf = [0u8; 32];
//! let n = v.to_head_tail(&mut buf).unwrap();
//! assert_eq!(&buf[..n], &[7, 0, 0b1000_0000, 2, 0, 1, 0, 2, 0]);
//! assert_eq!(Load::from_head_tail(&buf[..n]).unwrap(), v);
//! ```
//!
//! A list is a `List`, read item by item from the wire:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(head_tail)]
//! struct Load<'i> {
//!     frames: RefVec<'i, u16>, // use shrink_wrap::head_tail::List<'i, u16>
//! }
//! ```
//!
//! and its items have one length:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//! use shrink_wrap::head_tail::List;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Maybe {
//!     a: u8,
//!     b: Option<u32>,
//! }
//!
//! #[derive_shrink_wrap(head_tail)]
//! struct Load<'i> {
//!     items: List<'i, Maybe>,
//! }
//! ```
//!
//! A `head_tail` type has no owned version:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(head_tail, owned)]
//! struct Load {
//!     off: u16,
//! }
//! ```

use core::fmt::{Debug, Formatter, Write};
use core::marker::PhantomData;

use crate::Error;
use crate::fixed_layout::FixedLayout;

/// A type in the head+tail encoding, implemented by `#[derive_shrink_wrap]` for `head_tail` and fixed-layout
/// types: see the [module](self) docs.
pub trait HeadTail<'i>: Sized {
    /// Bytes of the head.
    const HEAD_LEN: usize;
    /// The name of the type in C (snake case, without the prefix).
    const C_NAME: &'static str;
    /// The C text has a tail walker `<prefix><C_NAME>_tail` (else the type is all head).
    const C_TAIL: bool;

    /// Reads the value from its head (at least [HEAD_LEN](Self::HEAD_LEN) bytes) and the tail cursor.
    fn read_ht(head: &'i [u8], tail: &mut Tail<'i>) -> Result<Self, Error>;

    /// Writes the head into `head` (at least [HEAD_LEN](Self::HEAD_LEN) bytes) and the tail through `tail`.
    fn write_ht(&self, head: &mut [u8], tail: &mut TailWr) -> Result<(), Error>;

    /// Bytes this value takes in the tail.
    fn tail_len(&self) -> usize;

    /// What [view] gives (SW-52): for a `head_tail` struct `X` the `XView<'i>` the derive generates, a reference
    /// to the head (fields read at their offsets when asked for) and the tail parts, walked once; for a
    /// `head_tail` enum the `XView` enum of the variants' views; for a fixed-layout type the value itself.
    type View;

    /// Views the value from its head (at least [HEAD_LEN](Self::HEAD_LEN) bytes) and the tail cursor: checks the
    /// lengths and walks the tail, reads no field of the head but the counts and flags it needs for that.
    fn view_ht(head: &'i [u8], tail: &mut Tail<'i>) -> Result<Self::View, Error>;

    /// The C defines, readers and tail walker of this type, names starting with `prefix`. Needs
    /// [C_PRELUDE](crate::fixed_layout::C_PRELUDE) and [C_TAIL_PRELUDE] in front of it, and the C text of the
    /// types it holds.
    fn write_c_ht(prefix: &str, out: &mut dyn Write) -> core::fmt::Result;
}

/// Reads a value that starts at `buf[0]`: its head, then its tail.
#[inline]
pub fn read<'i, T: HeadTail<'i>>(buf: &'i [u8]) -> Result<T, Error> {
    let Some((head, rest)) = buf.split_at_checked(T::HEAD_LEN) else {
        return Err(Error::OutOfBoundsReadRawSlice);
    };
    T::read_ht(head, &mut Tail::new(rest))
}

/// Views a value that starts at `buf[0]`: see [HeadTail::View].
#[inline]
pub fn view<'i, T: HeadTail<'i>>(buf: &'i [u8]) -> Result<T::View, Error> {
    let Some((head, rest)) = buf.split_at_checked(T::HEAD_LEN) else {
        return Err(Error::OutOfBoundsReadRawSlice);
    };
    T::view_ht(head, &mut Tail::new(rest))
}

/// Writes `v` at the start of `buf`, returns its length.
#[inline]
pub fn write<'i, T: HeadTail<'i>>(v: &T, buf: &mut [u8]) -> Result<usize, Error> {
    let Some((head, rest)) = buf.split_at_mut_checked(T::HEAD_LEN) else {
        return Err(Error::OutOfBoundsWriteRawSlice);
    };
    let mut tail = TailWr::new(rest);
    v.write_ht(head, &mut tail)?;
    Ok(T::HEAD_LEN + tail.pos())
}

/// The cursor over a tail: hands out the bytes of each variable part in turn.
pub struct Tail<'i> {
    rest: &'i [u8],
}

impl<'i> Tail<'i> {
    pub fn new(rest: &'i [u8]) -> Self {
        Tail { rest }
    }

    /// The next `n` bytes.
    #[inline]
    pub fn take(&mut self, n: usize) -> Result<&'i [u8], Error> {
        match self.rest.split_at_checked(n) {
            Some((bytes, rest)) => {
                self.rest = rest;
                Ok(bytes)
            }
            None => Err(Error::OutOfBoundsReadRawSlice),
        }
    }

    /// The next `N` bytes, as an array: reads at constant offsets from it need no bounds checks.
    #[inline]
    pub fn take_array<const N: usize>(&mut self) -> Result<&'i [u8; N], Error> {
        match self.rest.split_first_chunk::<N>() {
            Some((bytes, rest)) => {
                self.rest = rest;
                Ok(bytes)
            }
            None => Err(Error::OutOfBoundsReadRawSlice),
        }
    }

    /// `n` items of `item_len` bytes each.
    #[inline]
    pub fn take_items(&mut self, n: usize, item_len: usize) -> Result<&'i [u8], Error> {
        // a count is a u16: with items of at most 64 KiB the product fits a 32-bit usize, and the check folds away
        // (a checked multiplication is a call to a division routine on a Cortex-M0)
        if usize::BITS >= 32 && n <= u16::MAX as usize && item_len <= u16::MAX as usize {
            return self.take(n * item_len);
        }
        match n.checked_mul(item_len) {
            Some(len) => self.take(len),
            None => Err(Error::OutOfBoundsReadRawSlice),
        }
    }

    /// What is left, without taking it.
    pub fn rest(&self) -> &'i [u8] {
        self.rest
    }
}

/// The writer of a tail: hands out the bytes for each variable part in turn.
pub struct TailWr<'a> {
    rest: &'a mut [u8],
    pos: usize,
}

impl<'a> TailWr<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        TailWr { rest: buf, pos: 0 }
    }

    /// The next `n` bytes, to fill. They are no longer borrowed from the writer: a nested value can write its
    /// head into them and its tail through the writer.
    #[inline]
    pub fn put(&mut self, n: usize) -> Result<&'a mut [u8], Error> {
        let rest = core::mem::take(&mut self.rest);
        if n > rest.len() {
            self.rest = rest;
            return Err(Error::OutOfBoundsWriteRawSlice);
        }
        let (bytes, rest) = rest.split_at_mut(n);
        self.rest = rest;
        self.pos += n;
        Ok(bytes)
    }

    /// Bytes written so far.
    pub fn pos(&self) -> usize {
        self.pos
    }
}

/// A count in the head: `u16`.
#[inline]
pub fn count(n: usize) -> Result<[u8; 2], Error> {
    match u16::try_from(n) {
        Ok(n) => Ok(n.to_le_bytes()),
        Err(_) => Err(Error::LenTooLong),
    }
}

/// A list in a `head_tail` type: its count in the head, its items one after another in the tail, each a
/// fixed-layout value of [ENCODED_LEN](FixedLayout::ENCODED_LEN) bytes. A list read from the wire keeps the bytes
/// and reads an item when asked for it.
pub enum List<'i, T> {
    /// Items in memory, to write.
    Slice(&'i [T]),
    /// Items on the wire.
    Wire {
        bytes: &'i [u8],
        len: usize,
        _t: PhantomData<T>,
    },
}

impl<T> Clone for List<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for List<'_, T> {}

impl<'i, T> List<'i, T> {
    pub const fn new(items: &'i [T]) -> Self {
        List::Slice(items)
    }

    /// `len` items in `bytes`, as [Tail] hands them out.
    pub const fn wire(bytes: &'i [u8], len: usize) -> Self {
        List::Wire {
            bytes,
            len,
            _t: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            List::Slice(items) => items.len(),
            List::Wire { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'i, T: FixedLayout + Clone> List<'i, T> {
    /// Item `i`, `None` past the end; an error when its bytes are not a valid value (an unknown enum tag).
    #[inline]
    pub fn get(&self, i: usize) -> Option<Result<T, Error>> {
        match self {
            List::Slice(items) => items.get(i).cloned().map(Ok),
            List::Wire { bytes, len, .. } => {
                if i >= *len {
                    return None;
                }
                Some(T::read_fixed(
                    bytes.get(i * T::ENCODED_LEN..).unwrap_or(&[]),
                ))
            }
        }
    }

    pub fn iter(&self) -> ListIter<'i, T> {
        match *self {
            List::Slice(items) => ListIter::Slice(items.iter()),
            List::Wire { bytes, len, .. } => ListIter::Wire {
                bytes,
                left: len,
                _t: PhantomData,
            },
        }
    }

    /// Writes the items one after another into `out`, which is `len() * T::ENCODED_LEN` bytes.
    #[inline]
    pub fn write_items(&self, out: &mut [u8]) -> Result<(), Error> {
        match self {
            List::Slice(items) => {
                // not `chunks_exact_mut`: it divides by the chunk length
                let mut at = 0;
                for item in items.iter() {
                    match out.get_mut(at..) {
                        Some(rest) => at += item.write_fixed(rest)?,
                        None => return Err(Error::OutOfBoundsWriteRawSlice),
                    }
                }
                Ok(())
            }
            List::Wire { bytes, .. } => match out.get_mut(..bytes.len()) {
                Some(out) => {
                    out.copy_from_slice(bytes);
                    Ok(())
                }
                None => Err(Error::OutOfBoundsWriteRawSlice),
            },
        }
    }
}

impl<'i> List<'i, u8> {
    /// The bytes themselves.
    pub fn as_bytes(&self) -> &'i [u8] {
        match self {
            List::Slice(items) => items,
            List::Wire { bytes, .. } => bytes,
        }
    }
}

/// The items of a [List], each read when it is reached.
pub enum ListIter<'i, T> {
    Slice(core::slice::Iter<'i, T>),
    /// The bytes from the next item on, and how many items are left.
    Wire {
        bytes: &'i [u8],
        left: usize,
        _t: PhantomData<T>,
    },
}

impl<T: FixedLayout + Clone> Iterator for ListIter<'_, T> {
    type Item = Result<T, Error>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ListIter::Slice(items) => items.next().cloned().map(Ok),
            ListIter::Wire { bytes, left, .. } => {
                if *left == 0 {
                    return None;
                }
                *left -= 1;
                let item = T::read_fixed(bytes);
                *bytes = bytes.get(T::ENCODED_LEN..).unwrap_or(&[]);
                Some(item)
            }
        }
    }
}

impl<T: FixedLayout + Clone + Debug> Debug for List<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: FixedLayout + Clone + PartialEq> PartialEq for List<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

/// The C helpers the generated tail walkers use.
pub const C_TAIL_PRELUDE: &str = r#"
#ifndef SW_HEAD_TAIL
#define SW_HEAD_TAIL
/* the next n bytes of the tail at *t (before end): their start, or NULL when there are fewer */
static inline const uint8_t *sw_ht_take(const uint8_t **t, const uint8_t *end, size_t n) {
    const uint8_t *at = *t;
    if (at == NULL || (size_t)(end - at) < n) {
        return NULL;
    }
    *t = at + n;
    return at;
}
#endif
"#;

/// A whole C header for `head_tail` types: include guard `guard`, the preludes, then the readers of `types` (each
/// `T::write_c_ht`, the types a type holds before it), names starting with `prefix`.
pub fn write_c_header(
    out: &mut dyn Write,
    guard: &str,
    prefix: &str,
    types: &[fn(&str, &mut dyn Write) -> core::fmt::Result],
) -> core::fmt::Result {
    writeln!(
        out,
        "/* Generated by shrink_wrap's head_tail (SW-46) from the Rust types, do not edit. */"
    )?;
    writeln!(out, "#ifndef {guard}\n#define {guard}\n")?;
    out.write_str(crate::fixed_layout::C_PRELUDE)?;
    out.write_str(C_TAIL_PRELUDE)?;
    for write_c in types {
        write_c(prefix, out)?;
    }
    writeln!(out, "\n#endif /* {guard} */")
}

macro_rules! prim {
    ($($ty:ty),*) => {$(
        impl FixedLayout for $ty {
            const ENCODED_LEN: usize = size_of::<$ty>();
            const MIN_LEN: usize = size_of::<$ty>();
            const SAME_LEN: bool = true;
            const LEADS_WITH_BITS: bool = false;
            const TRAILS_WITH_BITS: bool = false;

            #[inline]
            fn read_fixed(buf: &[u8]) -> Result<Self, Error> {
                Ok(<$ty>::from_le_bytes(crate::fixed_layout::take_at::<{ size_of::<$ty>() }>(buf, 0)?))
            }

            #[inline]
            fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, Error> {
                match buf.first_chunk_mut::<{ size_of::<$ty>() }>() {
                    Some(b) => {
                        *b = self.to_le_bytes();
                        Ok(size_of::<$ty>())
                    }
                    None => Err(Error::OutOfBoundsWriteRawSlice),
                }
            }

            fn fixed_len(&self) -> usize {
                size_of::<$ty>()
            }

            fn write_c(_prefix: &str, _out: &mut dyn Write) -> core::fmt::Result {
                Ok(())
            }
        }
    )*};
}

// numbers are fixed-layout values of their own, for the items of a List
prim!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
