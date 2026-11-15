//! Raw device formats: little and big endian, packed 1, 10, 12 and 24 bits.

use super::{Sample, SampleError};

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
    out.fill(0);
    for (i, v) in values.iter().enumerate() {
        let raw = v.to_bits();
        let fits = if signed {
            let s = sign_extend(raw, scalar_bits);
            let half = 1i64 << (bits - 1);
            (-half..half).contains(&s)
        } else {
            raw <= mask
        };
        if !fits {
            return Err(SampleError::ValueTooWide { index: i });
        }
        let v = raw & mask;
        let bit = i * bits as usize;
        let at = bit / 8;
        let sh = (bit % 8) as u32;
        let n = (sh + bits).div_ceil(8) as usize;
        if msb {
            let word = (v << (64 - bits - sh)).to_be_bytes();
            for (o, b) in out[at..at + n].iter_mut().zip(word) {
                *o |= b;
            }
        } else {
            let word = (v << sh).to_le_bytes();
            for (o, b) in out[at..at + n].iter_mut().zip(word) {
                *o |= b;
            }
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
    match (bits, msb, signed) {
        (1, _, _) => unpack::<1, false, false, T>(bytes, first, out),
        (10, false, false) => unpack::<10, false, false, T>(bytes, first, out),
        (10, false, true) => unpack::<10, false, true, T>(bytes, first, out),
        (10, true, false) => unpack::<10, true, false, T>(bytes, first, out),
        (10, true, true) => unpack::<10, true, true, T>(bytes, first, out),
        (12, false, false) => unpack::<12, false, false, T>(bytes, first, out),
        (12, false, true) => unpack::<12, false, true, T>(bytes, first, out),
        (12, true, false) => unpack::<12, true, false, T>(bytes, first, out),
        (12, true, true) => unpack::<12, true, true, T>(bytes, first, out),
        (24, false, false) => unpack::<24, false, false, T>(bytes, first, out),
        (24, false, true) => unpack::<24, false, true, T>(bytes, first, out),
        (24, true, false) => unpack::<24, true, false, T>(bytes, first, out),
        (24, true, true) => unpack::<24, true, true, T>(bytes, first, out),
        _ => unreachable!("packed widths are 1, 10, 12 and 24 (codec table)"),
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
