//! Raw device formats: little and big endian, packed 1, 10, 12 and 24 bits.
//!
//! An `_LSB` column is the [bitpack] layout (horizontal, LSB first), so it packs and unpacks with the shared primitive
//! and its vector kernels; `_MSB` columns have their own loop.

use super::{Sample, SampleError};
use crate::bitpack;

/// Bytes of one value of `T`.
#[inline(always)]
fn width<T: Sample>() -> usize {
    (T::ELEMENT.scalar_bits() / 8) as usize
}

#[inline(always)]
fn load_le(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    w[..b.len()].copy_from_slice(b);
    u64::from_le_bytes(w)
}

#[inline(always)]
fn load_be(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    w[8 - b.len()..].copy_from_slice(b);
    u64::from_be_bytes(w)
}

pub(super) fn encode_le<T: Sample>(values: &[T], out: &mut [u8]) -> usize {
    let w = width::<T>();
    for (v, o) in values.iter().zip(out.chunks_exact_mut(w)) {
        o.copy_from_slice(&v.to_bits().to_le_bytes()[..w]);
    }
    values.len() * w
}

pub(super) fn encode_be<T: Sample>(values: &[T], out: &mut [u8]) -> usize {
    let w = width::<T>();
    for (v, o) in values.iter().zip(out.chunks_exact_mut(w)) {
        o.copy_from_slice(&v.to_bits().to_be_bytes()[8 - w..]);
    }
    values.len() * w
}

pub(super) fn decode_le<T: Sample>(bytes: &[u8], first: usize, out: &mut [T]) {
    let w = width::<T>();
    let src = &bytes[first * w..(first + out.len()) * w];
    for (o, b) in out.iter_mut().zip(src.chunks_exact(w)) {
        *o = T::from_bits(load_le(b));
    }
}

pub(super) fn decode_be<T: Sample>(bytes: &[u8], first: usize, out: &mut [T]) {
    let w = width::<T>();
    let src = &bytes[first * w..(first + out.len()) * w];
    for (o, b) in out.iter_mut().zip(src.chunks_exact(w)) {
        *o = T::from_bits(load_be(b));
    }
}

/// Sign-extend the low `bits` of `v`.
#[inline(always)]
fn sign_extend(v: u64, bits: u32) -> i64 {
    ((v << (64 - bits)) as i64) >> (64 - bits)
}

pub(super) fn encode_packed<T: Sample>(
    values: &[T],
    bits: u32,
    msb: bool,
    out: &mut [u8],
) -> Result<usize, SampleError> {
    let scalar_bits = T::ELEMENT.scalar_bits();
    let signed = T::ELEMENT.is_signed();
    let mask = (1u64 << bits) - 1;
    for (i, v) in values.iter().enumerate() {
        let raw = v.to_bits();
        let fits = if signed {
            let half = 1i64 << (bits - 1);
            (-half..half).contains(&sign_extend(raw, scalar_bits))
        } else {
            raw <= mask
        };
        if !fits {
            return Err(SampleError::ValueTooWide { index: i });
        }
    }
    if !msb {
        bitpack::pack(out, values.iter().map(|v| v.to_bits()), bits);
        return Ok(out.len());
    }
    out.fill(0);
    for (i, v) in values.iter().enumerate() {
        let bit = i * bits as usize;
        let (at, sh) = (bit / 8, (bit % 8) as u32);
        let n = (sh + bits).div_ceil(8) as usize;
        let word = ((v.to_bits() & mask) << (64 - bits - sh)).to_be_bytes();
        for (o, b) in out[at..at + n].iter_mut().zip(word) {
            *o |= b;
        }
    }
    Ok(out.len())
}

pub(super) fn decode_packed<T: Sample>(
    bytes: &[u8],
    bits: u32,
    msb: bool,
    signed: bool,
    first: usize,
    out: &mut [T],
) {
    if !msb {
        return if signed {
            unpack_lsb::<true, T>(bytes, bits, first, out)
        } else {
            unpack_lsb::<false, T>(bytes, bits, first, out)
        };
    }
    match (bits, signed) {
        (10, false) => unpack::<10, true, false, T>(bytes, first, out),
        (10, true) => unpack::<10, true, true, T>(bytes, first, out),
        (12, false) => unpack::<12, true, false, T>(bytes, first, out),
        (12, true) => unpack::<12, true, true, T>(bytes, first, out),
        (24, false) => unpack::<24, true, false, T>(bytes, first, out),
        (24, true) => unpack::<24, true, true, T>(bytes, first, out),
        _ => unreachable!("MSB widths are 10, 12 and 24 (codec table)"),
    }
}

/// Unpack `out.len()` values of `bits` bits from value `first` on in the [bitpack] layout: single reads up to the
/// next group of [bitpack::GROUP] values, whole groups with the vector kernels (a group starts on a byte: `GROUP · bits`
/// is a multiple of 8), single reads for the rest.
fn unpack_lsb<const SIGNED: bool, T: Sample>(bytes: &[u8], bits: u32, first: usize, out: &mut [T]) {
    let g = bitpack::GROUP;
    let value = |raw: u64| {
        T::from_bits(if SIGNED {
            sign_extend(raw, bits) as u64
        } else {
            raw
        })
    };
    let head = (first.next_multiple_of(g) - first).min(out.len());
    let (head_out, rest) = out.split_at_mut(head);
    for (k, o) in head_out.iter_mut().enumerate() {
        *o = value(bitpack::read(bytes, first + k, bits));
    }
    let mut at = first + head;
    let mut group = [0u32; bitpack::GROUP];
    let mut chunks = rest.chunks_exact_mut(g);
    for chunk in &mut chunks {
        bitpack::unpack_u32(&bytes[at * bits as usize / 8..], bits, &mut group);
        // sign extension in 32 bits, which vectorises (packed widths are at most 24)
        let sh = 32 - bits;
        for (o, raw) in chunk.iter_mut().zip(group) {
            let v = if SIGNED {
                (((raw << sh) as i32) >> sh) as i64 as u64
            } else {
                u64::from(raw)
            };
            *o = T::from_bits(v);
        }
        at += g;
    }
    for (k, o) in chunks.into_remainder().iter_mut().enumerate() {
        *o = value(bitpack::read(bytes, at + k, bits));
    }
}

/// Value `i` of `W` bits from a word loaded at its first byte.
#[inline(always)]
fn extract<const W: u32, const MSB: bool, const SIGNED: bool>(word: u64, sh: u32) -> u64 {
    let v = if MSB {
        (word << sh) >> (64 - W)
    } else {
        (word >> sh) & ((1u64 << W) - 1)
    };
    if SIGNED { sign_extend(v, W) as u64 } else { v }
}

/// Unpack `out.len()` values of `W` bits from value `first` on: an 8-byte load per value where 8 bytes are left,
/// a padded load in the last bytes.
fn unpack<const W: u32, const MSB: bool, const SIGNED: bool, T: Sample>(
    bytes: &[u8],
    first: usize,
    out: &mut [T],
) {
    let w = W as usize;
    // values whose 8-byte load stays in `bytes`: i·W/8 + 8 <= len
    let fast = if bytes.len() >= 8 {
        ((bytes.len() - 8) * 8 + 7) / w + 1
    } else {
        0
    };
    let split = fast.saturating_sub(first).min(out.len());
    let (head, tail) = out.split_at_mut(split);
    for (k, o) in head.iter_mut().enumerate() {
        let bit = (first + k) * w;
        let at = bit / 8;
        let b: [u8; 8] = bytes[at..at + 8]
            .try_into()
            .expect("8 bytes in the fast part");
        let word = if MSB {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        };
        *o = T::from_bits(extract::<W, MSB, SIGNED>(word, (bit % 8) as u32));
    }
    for (k, o) in tail.iter_mut().enumerate() {
        let bit = (first + split + k) * w;
        let at = bit / 8;
        let rest = &bytes[at..bytes.len().min(at + 8)];
        let word = if MSB {
            load_be(rest) << (8 * (8 - rest.len()))
        } else {
            load_le(rest)
        };
        *o = T::from_bits(extract::<W, MSB, SIGNED>(word, (bit % 8) as u32));
    }
}
