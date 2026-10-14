//! The fixed-layout fast path (SW-44, SW-45): types whose fields sit at offsets known at compile time.
//!
//! shrink_wrap writes a value field after field. When no field's size depends on its value, every field starts at
//! the same place in every value, and reading a field is a load at a constant offset. `#[derive_shrink_wrap]` sees
//! such types and generates, next to the usual serdes code:
//!
//! - `ENCODED_LEN`: the longest encoding, the buffer to have ready; `MIN_LEN`: the shortest; `fixed_len()`: the
//!   length of one value. They are all the same unless the last field has a variable length (see below);
//! - an offset const per field: `<FIELD>_OFFSET` (bytes) for byte-aligned fields, `<FIELD>_BIT_OFFSET` and
//!   `<FIELD>_BITS` for bit fields (bits counted from the most significant bit of the first byte, as shrink_wrap
//!   packs them); for an enum `TAG_LEN` and per variant `<VARIANT>_TAG`, `<VARIANT>_LEN`, `<VARIANT>_<FIELD>_..`;
//! - `from_fixed(&[u8])` / `to_fixed(&mut [u8])`: loads and stores at those offsets, no
//!   [BufReader](crate::BufReader) or [BufWriter](crate::BufWriter), nothing generic. Same bytes as
//!   `ser_shrink_wrap`. A short buffer or an unknown tag is an error, never a panic;
//! - [FixedLayout], whose [write_c](FixedLayout::write_c) prints the matching C reader ([write_c_header]).
//!
//! # What has a fixed layout
//!
//! - **Numbers**: `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, little-endian, each starting on a byte, as shrink_wrap
//!   writes them. Arrays of them.
//! - **Bit fields**: `bool`, `U1`..`U32`, `I2`..`I32` and `Nibble`, packed into bytes most significant bit first
//!   (a `Nibble` starts on a 4-bit boundary). A byte-aligned field after them starts a fresh byte, the unused bits
//!   are zero. The bit offsets are as fixed as the byte ones.
//! - **Other fixed-layout types** (with the `fixed_layout` directive: the macro cannot look inside a type defined
//!   elsewhere, the compiler checks it). Each starts on a byte boundary. Where its first or last field is a bit
//!   field, shrink_wrap would pack it together with a bit field of the parent next to it, which no fixed offset
//!   can describe: that is a compile error asking for a byte-aligned field between them.
//! - **One field of variable length, last**: an `Option<T>` (present only when its flag bit is set), a
//!   `Result<T, E>` (`T` or `E` by its flag bit) or an enum whose variants differ in length. Every field before it
//!   keeps its offset; nothing could after it. Their payload starts on a byte (`Option<bool>` is refused: use a
//!   `U2` or two `bool`s). The flags are ordinary bit fields, also where `#[flag]` moves them.
//!
//! Not: `Vec`, `RefVec`, strings, `MaxLen`, `UNib32` and the other varints (their size depends on the value),
//! `RefBox`, `TailSize`, tuples, ranges, 128-bit numbers, bit fields wider than 32 bits, `#[default]` fields. A
//! struct must be `sized`, `final_structure` or `self_describing` (an evolvable type may grow, so it has no fixed
//! layout); an enum `sized` or `self_describing` with a byte-aligned tag, `ww_repr = u8`, `u16` or `u32`.
//!
//! Types that qualify get the fast path without asking. The `fixed_layout` directive makes it a requirement: a
//! field that breaks it is a compile error that says why and what to use instead. On a struct it also picks the
//! size assumption when none is given: `sized`, or `self_describing` when the last field is an `Option` or
//! `Result`.
//!
//! ```
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     magic: [u8; 4],
//!     urgent: bool,
//!     retries: U3,
//!     count: u32,
//! }
//!
//! assert_eq!(Header::URGENT_BIT_OFFSET, 32);
//! assert_eq!(Header::RETRIES_BIT_OFFSET, 33);
//! assert_eq!(Header::COUNT_OFFSET, 5);
//! assert_eq!(Header::ENCODED_LEN, 9);
//! let h = Header::from_fixed(&[b'C', b'K', b'P', b'L', 0b1011_0000, 2, 0, 0, 0]).unwrap();
//! assert!(h.urgent);
//! assert_eq!(h.retries.value(), 3);
//! assert_eq!(h.count, 2);
//! ```
//!
//! With `fixed_layout`, a field without a fixed layout is a compile error:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     count: UNib32, // variable length: use u32
//! }
//! ```
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     count: Option<u32>, // not the last field
//!     id: u8,
//! }
//! ```
//!
//! A nested type must have a fixed layout itself:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed)]
//! struct Evolvable {
//!     a: u8,
//! }
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     inner: Evolvable,
//! }
//! ```
//!
//! and one length, unless it is the last field:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, self_describing, fixed_layout, ww_repr = u8)]
//! enum Op {
//!     Stop,
//!     Go(u32),
//! }
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     op: Op,
//!     count: u32,
//! }
//! ```
//!
//! Bits of a nested type cannot share a byte with bits of the parent:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Flags {
//!     a: bool,
//! }
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     flags: Flags,
//!     b: bool,
//! }
//! ```
//!
//! An enum tag must be byte-aligned:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, sized, fixed_layout, ww_repr = nib)]
//! enum Mode {
//!     A,
//!     B,
//! }
//! ```

use core::fmt::{Display, Formatter, Write};

use crate::Error;

/// A type with a fixed layout, implemented by `#[derive_shrink_wrap]`: see the [module](self) docs.
pub trait FixedLayout: Sized {
    /// The longest encoding in bytes: a buffer of this length holds every value. For the length of one value see
    /// [fixed_len](Self::fixed_len).
    const ENCODED_LEN: usize;
    /// The shortest encoding in bytes.
    const MIN_LEN: usize;
    /// Every value has the length [ENCODED_LEN](Self::ENCODED_LEN). Only such types can be fields of another
    /// fixed-layout type before its last field.
    const SAME_LEN: bool;
    /// The first field is a bit field: it would share a byte with bit fields of the parent before it.
    const LEADS_WITH_BITS: bool;
    /// The last field is a bit field (or a flag the value may end with): bit fields of the parent after it would
    /// share its last byte.
    const TRAILS_WITH_BITS: bool;

    /// Reads the value at the start of `buf`.
    fn read_fixed(buf: &[u8]) -> Result<Self, Error>;

    /// Writes the value at the start of `buf`, returns its length.
    fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, Error>;

    /// Length of this value on the wire, at most [ENCODED_LEN](Self::ENCODED_LEN).
    fn fixed_len(&self) -> usize;

    /// The C defines and readers of this type, names starting with `prefix` (`PREFIX` for defines). Needs the
    /// readers of [C_PRELUDE] in front of it.
    fn write_c(prefix: &str, out: &mut dyn Write) -> core::fmt::Result;
}

/// `N` bytes of `b` from `off`. Generated readers call it with const offsets on an array of known length, so the
/// bounds checks fold away.
#[inline(always)]
pub fn take<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(&b[off..off + N]);
    out
}

/// `N` bytes of `b` from `off`, or an error when `b` is too short: for the payload of a variable last field.
#[inline(always)]
pub fn take_at<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N], Error> {
    match b.get(off..).and_then(|b| b.first_chunk::<N>()) {
        Some(bytes) => Ok(*bytes),
        None => Err(Error::OutOfBoundsReadRawSlice),
    }
}

/// `n` (1 to 32) bits of `b` from bit `bit`, counted from the most significant bit of `b[0]`, as shrink_wrap packs
/// them. With constant arguments this folds to a few loads and shifts.
#[inline(always)]
pub fn get_bits(b: &[u8], bit: usize, n: u32) -> u32 {
    let first = bit / 8;
    let last = (bit + n as usize - 1) / 8;
    let mut window: u64 = 0;
    let mut i = first;
    while i <= last {
        window = (window << 8) | b[i] as u64;
        i += 1;
    }
    let shift = (last + 1) * 8 - bit - n as usize;
    ((window >> shift) & (u64::MAX >> (64 - n))) as u32
}

/// Sets `n` (1 to 32) bits of `b` from bit `bit` to the low bits of `val`. The bits must be zero before: the
/// generated writers clear the bytes of their bit fields first.
#[inline(always)]
pub fn put_bits(b: &mut [u8], bit: usize, n: u32, val: u32) {
    let first = bit / 8;
    let last = (bit + n as usize - 1) / 8;
    let shift = (last + 1) * 8 - bit - n as usize;
    let window = ((val as u64) & (u64::MAX >> (64 - n))) << shift;
    let mut i = first;
    while i <= last {
        b[i] |= (window >> ((last - i) * 8)) as u8;
        i += 1;
    }
}

/// Shows a str in ASCII upper case, for the defines of [FixedLayout::write_c].
pub struct Upper<'a>(pub &'a str);

impl Display for Upper<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        for c in self.0.chars() {
            f.write_char(c.to_ascii_uppercase())?;
        }
        Ok(())
    }
}

/// The loads the generated C readers use: little-endian numbers and bit fields (most significant bit first).
/// Octets are `uint8_t`: a target without it (16-bit `char`, TI C2000) needs the octet accessors of the C runtime
/// instead (API-26).
pub const C_PRELUDE: &str = r#"#include <stddef.h>
#include <stdint.h>
#include <string.h>

#ifndef SW_FIXED_LOADS
#define SW_FIXED_LOADS
static inline uint8_t sw_fixed_u8(const uint8_t *p) { return p[0]; }
static inline uint16_t sw_fixed_u16(const uint8_t *p) { return (uint16_t)((unsigned)p[0] | ((unsigned)p[1] << 8u)); }
static inline uint32_t sw_fixed_u32(const uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8u) | ((uint32_t)p[2] << 16u) | ((uint32_t)p[3] << 24u);
}
static inline uint64_t sw_fixed_u64(const uint8_t *p) {
    return (uint64_t)sw_fixed_u32(p) | ((uint64_t)sw_fixed_u32(p + 4u) << 32u);
}
static inline float sw_fixed_f32(const uint8_t *p) {
    uint32_t u = sw_fixed_u32(p);
    float f;
    memcpy(&f, &u, sizeof f);
    return f;
}
static inline double sw_fixed_f64(const uint8_t *p) {
    uint64_t u = sw_fixed_u64(p);
    double f;
    memcpy(&f, &u, sizeof f);
    return f;
}
/* n (1 to 32) bits from bit `bit`, counted from the most significant bit of p[0] */
static inline uint32_t sw_fixed_bits(const uint8_t *p, size_t bit, unsigned n) {
    size_t first = bit / 8u;
    size_t last = (bit + n - 1u) / 8u;
    size_t i;
    uint64_t window = 0u;
    for (i = first; i <= last; i++) {
        window = (window << 8u) | (uint64_t)p[i];
    }
    return (uint32_t)((window >> ((last + 1u) * 8u - bit - n)) & (UINT64_MAX >> (64u - n)));
}
/* the same, sign extended from bit n - 1 */
static inline int32_t sw_fixed_sbits(const uint8_t *p, size_t bit, unsigned n) {
    uint32_t sign = (uint32_t)1u << (n - 1u);
    uint32_t u = sw_fixed_bits(p, bit, n);
    return (int32_t)(int64_t)((int64_t)(u ^ sign) - (int64_t)sign);
}
#endif
"#;

/// A whole C header: include guard `guard`, [C_PRELUDE], then the readers of `types` (`T::write_c` of each
/// fixed-layout type), names starting with `prefix`.
pub fn write_c_header(
    out: &mut dyn Write,
    guard: &str,
    prefix: &str,
    types: &[fn(&str, &mut dyn Write) -> core::fmt::Result],
) -> core::fmt::Result {
    writeln!(
        out,
        "/* Generated by shrink_wrap's fixed_layout (SW-44) from the Rust types, do not edit. */"
    )?;
    writeln!(out, "#ifndef {guard}\n#define {guard}\n")?;
    out.write_str(C_PRELUDE)?;
    for write_c in types {
        write_c(prefix, out)?;
    }
    writeln!(out, "\n#endif /* {guard} */")
}
