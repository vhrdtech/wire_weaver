//! The fixed-layout fast path (SW-44): types whose encoding is a fixed little-endian byte layout.
//!
//! `#[derive_shrink_wrap]` gives a type that qualifies (every field a byte-aligned fixed-width number, an array of
//! them or another fixed-layout type; a `sized` or `final_structure` struct, a `sized` or `self_describing` enum
//! with a `u8`, `u16` or `u32` `ww_repr`) an offset const per field, `ENCODED_LEN`, and `from_fixed` / `to_fixed`
//! that read and write the slice directly, without [BufReader](crate::BufReader) or [BufWriter](crate::BufWriter).
//! The bytes are the ones `ser_shrink_wrap` writes. The `fixed_layout` directive makes it a requirement: a field
//! that breaks it is a compile error naming the field. [FixedLayout::write_c] prints the matching C reader; see
//! [write_c_header].
//!
//! ```
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     magic: [u8; 4],
//!     count: u32,
//! }
//!
//! assert_eq!(Header::COUNT_OFFSET, 4);
//! assert_eq!(Header::ENCODED_LEN, 8);
//! let h = Header::from_fixed(&[b'C', b'K', b'P', b'L', 2, 0, 0, 0]).unwrap();
//! assert_eq!(h.count, 2);
//! ```
//!
//! A `bool` (or `Option`, a list, a string, a variable-length number, a bit-packed `U4`) is a compile error with
//! `fixed_layout`: "fixed_layout: field `flag`: bool is one bit, bit-packed".
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     count: u32,
//!     flag: bool,
//! }
//! ```
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     count: Option<u32>,
//! }
//! ```
//!
//! A nested type must have a fixed layout itself:
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, sized)]
//! struct Flags {
//!     a: bool,
//! }
//!
//! #[derive_shrink_wrap(borrowed, fixed_layout)]
//! struct Header {
//!     flags: Flags,
//! }
//! ```
//!
//! and the same length for every value, or nothing after it could have a fixed offset:
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
//! An evolvable struct (no size assumption) has no fixed layout: it may grow.
//!
//! ```compile_fail
//! use shrink_wrap::prelude::*;
//!
//! #[derive_shrink_wrap(borrowed, self_describing, fixed_layout)]
//! struct Header {
//!     count: u32,
//! }
//! ```

use core::fmt::{Display, Formatter, Write};

use crate::Error;

/// A type with a fixed byte layout. Implemented by `#[derive_shrink_wrap]`, see the [module](self) docs.
pub trait FixedLayout: Sized {
    /// Length on the wire in bytes; for an enum whose variants differ in size, that of the longest one.
    const ENCODED_LEN: usize;
    /// Every value has the length [ENCODED_LEN](Self::ENCODED_LEN). Only such types can be fields of another
    /// fixed-layout type.
    const SAME_LEN: bool;

    /// Reads the value at the start of `buf`.
    fn read_fixed(buf: &[u8]) -> Result<Self, Error>;

    /// Writes the value at the start of `buf`, returns its length.
    fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, Error>;

    /// Length of this value on the wire.
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

/// Little-endian loads the generated C readers use. Octets are `uint8_t`: a target without it (16-bit `char`,
/// TI C2000) needs the octet accessors of the C runtime instead (API-26).
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
