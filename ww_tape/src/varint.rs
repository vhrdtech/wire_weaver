//! LEB128 `uvar` and zigzag `ivar`, as in the logring format.

use crate::FromFile;

/// Bytes a `uvar` of `v` takes: 1 to 10.
pub const fn uvar_len(v: u64) -> usize {
    // 7 bits a byte; v | 1 keeps 0 at one byte
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "v | 1 is nonzero: at most 63 leading zeros, under u64::BITS"
    )]
    let bits = u64::BITS - (v | 1).leading_zeros();
    // bits is 1 to 64: the quotient is 1 to 10, a usize on every target
    #[expect(
        clippy::as_conversions,
        reason = "1 to 10 fits a usize; From is not const"
    )]
    let n = bits.div_ceil(7) as usize;
    n
}

/// Bytes an `ivar` of `v` takes.
pub const fn ivar_len(v: i64) -> usize {
    uvar_len(zigzag(v))
}

const fn zigzag(v: i64) -> u64 {
    // the sign spread over every bit, then the magnitude shifted up: a bijection of the 64 bits
    v.wrapping_shl(1).cast_unsigned() ^ (v >> 63).cast_unsigned()
}

/// Write `v` at the start of `buf`, returns the bytes written; None, with `buf` partly written, when `buf` is
/// shorter than [uvar_len] of `v`.
pub(crate) fn put_uvar(buf: &mut [u8], mut v: u64) -> Option<usize> {
    for (i, slot) in buf.iter_mut().enumerate() {
        let [low, ..] = v.to_le_bytes();
        let b = low & 0x7f;
        v >>= 7;
        if v == 0 {
            *slot = b;
            return i.checked_add(1);
        }
        *slot = b | 0x80;
    }
    None
}

/// [put_uvar] of the zigzag form of `v`.
pub(crate) fn put_ivar(buf: &mut [u8], v: i64) -> Option<usize> {
    put_uvar(buf, zigzag(v))
}

pub(crate) struct Reader<'b>(pub &'b [u8]);

impl<'b> Reader<'b> {
    /// A `uvar` that never sizes anything (a delta, an id); None past the end or past 10 bytes.
    pub fn uvar(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..70).step_by(7) {
            let (&b, rest) = self.0.split_first()?;
            self.0 = rest;
            let part = u64::from(b & 0x7f);
            // the 10th byte holds bit 63 only: more would not fit a u64
            if part.checked_shl(shift)?.checked_shr(shift)? != part {
                return None;
            }
            v |= part << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    /// A `uvar` that is a count, a length or an offset: see [FromFile].
    #[allow(
        dead_code,
        reason = "STD-131: the modules move to it as they are hardened"
    )]
    pub fn uvar_file(&mut self) -> Option<FromFile<u64>> {
        self.uvar().map(FromFile::new)
    }

    pub fn ivar(&mut self) -> Option<i64> {
        let u = self.uvar()?;
        // zigzag back: the low bit is the sign, the rest the magnitude
        Some((u >> 1).cast_signed() ^ (u & 1).cast_signed().wrapping_neg())
    }

    pub fn take(&mut self, n: usize) -> Option<&'b [u8]> {
        let (a, b) = self.0.split_at_checked(n)?;
        self.0 = b;
        Some(a)
    }
}
