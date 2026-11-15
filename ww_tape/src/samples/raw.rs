//! Raw device formats: little and big endian, packed 1, 10, 12 and 24 bits.
//!
//! An `_LSB` column is the [bitpack] layout (horizontal, LSB first), so it packs and unpacks with the shared primitive
//! and its vector kernels; `_MSB` columns have their own loop.
//!
//! The decoders take a column whose length [Samples::new](super::Samples::new) checked against its count; a range
//! past its bytes is still an error, never a panic.

use super::{Sample, SampleError};
use crate::bitpack;

/// How a packed column lays out its values: the [bitpack] layout at a width, or one of the `_MSB` widths.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Packing {
    /// LSB first, 1 to 24 bits.
    Lsb(u32),
    Msb10,
    Msb12,
    Msb24,
}

impl Packing {
    /// Bits per value.
    pub(super) const fn bits(self) -> u32 {
        match self {
            Packing::Lsb(bits) => bits,
            Packing::Msb10 => 10,
            Packing::Msb12 => 12,
            Packing::Msb24 => 24,
        }
    }
}

/// Bytes of one value of `T`: its size, 1 to 8 for the scalars the byte-wide layouts code.
#[inline(always)]
const fn width<T: Sample>() -> usize {
    size_of::<T>()
}

/// The low `bits` bits set: none for 0, all for 64 and up.
#[inline(always)]
pub(super) const fn mask(bits: u32) -> u64 {
    // a shift by 64 (bits 0) is None: no bit set
    match u64::MAX.checked_shr(u64::BITS.saturating_sub(bits)) {
        Some(m) => m,
        None => 0,
    }
}

/// A little-endian number of up to 8 bytes.
#[inline(always)]
fn load_le(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    for (o, i) in w.iter_mut().zip(b) {
        *o = *i;
    }
    u64::from_le_bytes(w)
}

/// A big-endian number of up to 8 bytes.
#[inline(always)]
fn load_be(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    for (o, i) in w.iter_mut().rev().zip(b.iter().rev()) {
        *o = *i;
    }
    u64::from_be_bytes(w)
}

/// Up to 8 bytes from the start of `b`, the missing ones zero, as the first bytes of a big-endian word.
#[inline(always)]
fn load_be_front(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    for (o, i) in w.iter_mut().zip(b) {
        *o = *i;
    }
    u64::from_be_bytes(w)
}

/// Code `values` little endian into `out`, which is their bytes exactly; returns its length.
pub(super) fn encode_le<T: Sample>(values: &[T], out: &mut [u8]) -> usize {
    for (v, o) in values.iter().zip(out.chunks_exact_mut(width::<T>())) {
        for (d, s) in o.iter_mut().zip(v.to_bits().to_le_bytes()) {
            *d = s;
        }
    }
    out.len()
}

/// Code `values` big endian into `out`, which is their bytes exactly; returns its length.
pub(super) fn encode_be<T: Sample>(values: &[T], out: &mut [u8]) -> usize {
    for (v, o) in values.iter().zip(out.chunks_exact_mut(width::<T>())) {
        // the low bytes, the lowest last
        for (d, s) in o.iter_mut().rev().zip(v.to_bits().to_le_bytes()) {
            *d = s;
        }
    }
    out.len()
}

/// The bytes of `n` values of `T` from value `first` on.
fn values_bytes<T: Sample>(bytes: &[u8], first: usize, n: usize) -> Result<&[u8], SampleError> {
    let w = width::<T>();
    let from = first.checked_mul(w).ok_or(SampleError::OutOfRange)?;
    let len = n.checked_mul(w).ok_or(SampleError::OutOfRange)?;
    bytes
        .get(from..)
        .and_then(|b| b.get(..len))
        .ok_or(SampleError::OutOfRange)
}

pub(super) fn decode_le<T: Sample>(
    bytes: &[u8],
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    let src = values_bytes::<T>(bytes, first, out.len())?;
    for (o, b) in out.iter_mut().zip(src.chunks_exact(width::<T>())) {
        *o = T::from_bits(load_le(b));
    }
    Ok(())
}

pub(super) fn decode_be<T: Sample>(
    bytes: &[u8],
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    let src = values_bytes::<T>(bytes, first, out.len())?;
    for (o, b) in out.iter_mut().zip(src.chunks_exact(width::<T>())) {
        *o = T::from_bits(load_be(b));
    }
    Ok(())
}

/// Sign-extend the low `bits` of `v`, `bits` 1 to 64.
#[inline(always)]
pub(super) const fn sign_extend(v: u64, bits: u32) -> i64 {
    let sh = u64::BITS.wrapping_sub(bits);
    v.wrapping_shl(sh).cast_signed().wrapping_shr(sh)
}

pub(super) fn encode_packed<T: Sample>(
    values: &[T],
    packing: Packing,
    out: &mut [u8],
) -> Result<usize, SampleError> {
    let bits = packing.bits();
    let scalar_bits = T::ELEMENT.scalar_bits();
    let signed = T::ELEMENT.is_signed();
    let m = mask(bits);
    for (i, v) in values.iter().enumerate() {
        let raw = v.to_bits();
        let fits = if signed {
            sign_extend(raw, bits) == sign_extend(raw, scalar_bits)
        } else {
            raw <= m
        };
        if !fits {
            return Err(SampleError::ValueTooWide { index: i });
        }
    }
    let needed = out.len();
    if let Packing::Lsb(bits) = packing {
        bitpack::pack(out, values.iter().map(|v| v.to_bits()), bits);
        return Ok(needed);
    }
    pack_msb(out, values.iter().map(|v| v.to_bits() & m), bits)
        .ok_or(SampleError::Full { needed })?;
    Ok(needed)
}

/// Pack `vals` (each under `2^bits`, `bits` at most 56) MSB first into `out`, the last byte's spare bits zero;
/// None when `out` is too short.
fn pack_msb(out: &mut [u8], vals: impl Iterator<Item = u64>, bits: u32) -> Option<()> {
    // `pending` holds the `n` bits not yet written, n under 8 between values
    let mut pending = 0u64;
    let mut n = 0u32;
    let mut bytes = out.iter_mut();
    for v in vals {
        pending = pending.checked_shl(bits)? | v;
        n = n.checked_add(bits)?;
        while let Some(rest) = n.checked_sub(8) {
            let [byte, ..] = pending.checked_shr(rest)?.to_le_bytes();
            *bytes.next()? = byte;
            n = rest;
        }
        pending &= mask(n);
    }
    if n > 0 {
        let [byte, ..] = pending.checked_shl(8u32.checked_sub(n)?)?.to_le_bytes();
        *bytes.next()? = byte;
    }
    Some(())
}

pub(super) fn decode_packed<T: Sample>(
    bytes: &[u8],
    packing: Packing,
    signed: bool,
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    // the values' bits end within the bytes
    let end_bits = first
        .checked_add(out.len())
        .zip(usize::try_from(packing.bits()).ok())
        .and_then(|(end, bits)| end.checked_mul(bits));
    let len_bits = bytes.len().checked_mul(8);
    if !matches!((end_bits, len_bits), (Some(e), Some(l)) if e <= l) {
        return Err(SampleError::OutOfRange);
    }
    let done = match (packing, signed) {
        (Packing::Lsb(bits), true) => unpack_lsb::<true, T>(bytes, bits, first, out),
        (Packing::Lsb(bits), false) => unpack_lsb::<false, T>(bytes, bits, first, out),
        (Packing::Msb10, false) => unpack_msb::<10, false, T>(bytes, first, out),
        (Packing::Msb10, true) => unpack_msb::<10, true, T>(bytes, first, out),
        (Packing::Msb12, false) => unpack_msb::<12, false, T>(bytes, first, out),
        (Packing::Msb12, true) => unpack_msb::<12, true, T>(bytes, first, out),
        (Packing::Msb24, false) => unpack_msb::<24, false, T>(bytes, first, out),
        (Packing::Msb24, true) => unpack_msb::<24, true, T>(bytes, first, out),
    };
    done.ok_or(SampleError::OutOfRange)
}

/// Unpack `out.len()` values of `bits` bits (1 to 24) from value `first` on in the [bitpack] layout: single reads up
/// to the next group of [bitpack::GROUP] values, whole groups with the vector kernels (a group starts on a byte:
/// `GROUP · bits` is a multiple of 8), single reads for the rest. None when the values overflow an offset.
fn unpack_lsb<const SIGNED: bool, T: Sample>(
    bytes: &[u8],
    bits: u32,
    first: usize,
    out: &mut [T],
) -> Option<()> {
    let g = bitpack::GROUP;
    let value = |raw: u64| {
        T::from_bits(if SIGNED {
            sign_extend(raw, bits).cast_unsigned()
        } else {
            raw
        })
    };
    let head = first
        .checked_next_multiple_of(g)?
        .checked_sub(first)?
        .min(out.len());
    let (head_out, rest) = out.split_at_mut(head);
    for (k, o) in head_out.iter_mut().enumerate() {
        *o = value(bitpack::read(bytes, first.checked_add(k)?, bits));
    }
    let mut at = first.checked_add(head)?;
    let bits_usize = usize::try_from(bits).ok()?;
    // sign extension in 32 bits, which vectorises (packed widths are at most 24)
    let sh = 32u32.checked_sub(bits)?;
    let mut group = [0u32; bitpack::GROUP];
    let mut chunks = rest.chunks_exact_mut(g);
    for chunk in &mut chunks {
        let from = at.checked_mul(bits_usize)? / 8;
        bitpack::unpack_u32(bytes.get(from..)?, bits, &mut group);
        for (o, raw) in chunk.iter_mut().zip(group) {
            let v = if SIGNED {
                i64::from(raw.wrapping_shl(sh).cast_signed().wrapping_shr(sh)).cast_unsigned()
            } else {
                u64::from(raw)
            };
            *o = T::from_bits(v);
        }
        at = at.checked_add(g)?;
    }
    for (k, o) in chunks.into_remainder().iter_mut().enumerate() {
        *o = value(bitpack::read(bytes, at.checked_add(k)?, bits));
    }
    Some(())
}

/// Unpack `out.len()` values of `W` bits (at most 56), MSB first, from value `first` on: an 8-byte load per value
/// where 8 bytes are left, a padded load in the last bytes. None past the end of `bytes`.
fn unpack_msb<const W: u32, const SIGNED: bool, T: Sample>(
    bytes: &[u8],
    first: usize,
    out: &mut [T],
) -> Option<()> {
    let w = usize::try_from(W).ok()?;
    let drop = u64::BITS.checked_sub(W)?;
    let mut bit = first.checked_mul(w)?;
    for o in out.iter_mut() {
        let rest = bytes.get(bit / 8..)?;
        let word = match rest.first_chunk::<8>() {
            Some(b) => u64::from_be_bytes(*b),
            None => load_be_front(rest),
        };
        let sh = u32::try_from(bit % 8).ok()?;
        let v = word.wrapping_shl(sh).wrapping_shr(drop);
        *o = T::from_bits(if SIGNED {
            sign_extend(v, W).cast_unsigned()
        } else {
            v
        });
        bit = bit.checked_add(w)?;
    }
    Some(())
}
