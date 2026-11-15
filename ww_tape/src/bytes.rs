//! Little-endian byte readers and writers at an offset, checked: a read past the end is None, never a panic.
//!
//! Plain readers (`le_*`) are for values that never size anything: ids, ticks, CRCs, flags, magic. A count, a
//! length, an offset or a layout value is read with a `file_*` reader, which hands it out as a [FromFile]: it reaches
//! a slice length, a loop bound or an allocation only through a named cap or a known length.

use crate::FromFile;

const _: () = assert!(usize::BITS <= u64::BITS, "a usize widens to a u64");

/// `v` as a `u64`: a usize is at most 64 bits (the assert above).
#[inline]
pub(crate) const fn u64_of(v: usize) -> u64 {
    #[expect(
        clippy::as_conversions,
        reason = "a usize is at most 64 bits; From is not const"
    )]
    let w = v as u64;
    w
}

/// The `N` bytes at `at`, None past the end.
#[inline]
pub(crate) fn array<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..)?.first_chunk::<N>().copied()
}

#[inline]
pub(crate) fn le_u8(b: &[u8], at: usize) -> Option<u8> {
    b.get(at).copied()
}

#[inline]
pub(crate) fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    array(b, at).map(u16::from_le_bytes)
}

#[inline]
pub(crate) fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    array(b, at).map(u32::from_le_bytes)
}

#[inline]
pub(crate) fn le_u64(b: &[u8], at: usize) -> Option<u64> {
    array(b, at).map(u64::from_le_bytes)
}

#[inline]
pub(crate) fn le_i64(b: &[u8], at: usize) -> Option<i64> {
    array(b, at).map(i64::from_le_bytes)
}

/// A count, length, offset or layout byte.
#[inline]
pub(crate) fn file_u8(b: &[u8], at: usize) -> Option<FromFile<u8>> {
    le_u8(b, at).map(FromFile::new)
}

/// A count, length, offset or layout value of 2 bytes.
#[inline]
pub(crate) fn file_u16(b: &[u8], at: usize) -> Option<FromFile<u16>> {
    le_u16(b, at).map(FromFile::new)
}

/// A count, length, offset or layout value of 4 bytes.
#[inline]
pub(crate) fn file_u32(b: &[u8], at: usize) -> Option<FromFile<u32>> {
    le_u32(b, at).map(FromFile::new)
}

/// A count, length, offset or layout value of 8 bytes.
#[inline]
pub(crate) fn file_u64(b: &[u8], at: usize) -> Option<FromFile<u64>> {
    le_u64(b, at).map(FromFile::new)
}

/// Write `v` at `at`; None, writing nothing, when it doesn't fit.
#[inline]
pub(crate) fn put<const N: usize>(buf: &mut [u8], at: usize, v: [u8; N]) -> Option<()> {
    buf.get_mut(at..)?
        .first_chunk_mut::<N>()?
        .copy_from_slice(&v);
    Some(())
}

#[inline]
pub(crate) fn put_u16(buf: &mut [u8], at: usize, v: u16) -> Option<()> {
    put(buf, at, v.to_le_bytes())
}

#[inline]
pub(crate) fn put_u32(buf: &mut [u8], at: usize, v: u32) -> Option<()> {
    put(buf, at, v.to_le_bytes())
}

#[inline]
pub(crate) fn put_u64(buf: &mut [u8], at: usize, v: u64) -> Option<()> {
    put(buf, at, v.to_le_bytes())
}

#[inline]
pub(crate) fn put_i64(buf: &mut [u8], at: usize, v: i64) -> Option<()> {
    put(buf, at, v.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_past_the_end_are_none() {
        let b = [1u8, 2, 3, 4, 5];
        assert_eq!(le_u32(&b, 1), Some(0x0504_0302));
        assert_eq!(le_u32(&b, 2), None);
        assert_eq!(le_u16(&b, usize::MAX), None);
        assert!(file_u32(&b, 0).is_some_and(|v| v.is(0x0403_0201)));
        let mut out = [0u8; 4];
        assert_eq!(put_u32(&mut out, 1, 7), None);
        assert_eq!(out, [0; 4], "a write that doesn't fit writes nothing");
        assert_eq!(put_u16(&mut out, 2, 0x0102), Some(()));
        assert_eq!(out, [0, 0, 2, 1]);
    }
}
