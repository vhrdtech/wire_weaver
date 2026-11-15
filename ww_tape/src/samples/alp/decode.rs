//! The ALP decoder: every read of the column checked, a malformed group an error.

use super::{ALP_GROUP, DECIMAL, DICT_BITS_MAX, DICT_MAX, F32, F64, Float, LEFT_MAX, RD};
use crate::bitpack::{self, packed_len};
use crate::samples::{Element, Sample, SampleError};

/// Reads a group's bytes front to back.
struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> Reader<'b> {
    fn take(&mut self, n: usize) -> Option<&'b [u8]> {
        let end = self.at.checked_add(n)?;
        let b = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(b)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    /// `n` bytes (at most 8), little endian.
    fn uint(&mut self, n: usize) -> Option<u64> {
        let mut w = [0u8; 8];
        w.get_mut(..n)?.copy_from_slice(self.take(n)?);
        Some(u64::from_le_bytes(w))
    }
}

/// Exception positions: ascending, below `n`.
fn positions(p: &[u8], n: usize) -> Option<()> {
    let mut next = 0usize;
    for &i in p {
        let i = usize::from(i);
        if i < next || i >= n {
            return None;
        }
        next = i + 1;
    }
    Some(())
}

/// A decimal lane after its scheme byte into `out`; `rest` is the column from the group's start on, so the unpack
/// kernels may read past the lane.
fn decimal<F: Float>(
    r: &mut Reader<'_>,
    rest: &[u8],
    n: usize,
    out: &mut [u64; ALP_GROUP],
) -> Option<()> {
    let e = r.u8()?;
    let f = r.u8()?;
    let m = F::factors(e, f)?;
    let bits = F::BITS;
    let base = r.uint(F::BYTES)?;
    let base = ((base << (64 - bits)) as i64) >> (64 - bits);
    let width = u32::from(r.u8()?);
    let x = usize::from(r.u8()?);
    if width > bits || x > n {
        return None;
    }
    let packed = rest.get(r.at..)?;
    r.take(packed_len(n, width))?;
    let mut d = [0i64; ALP_GROUP];
    bitpack::decode_line(packed, width, base, 0, &mut d);
    for (o, d) in out.iter_mut().zip(&d) {
        *o = F::dec(*d, m);
    }
    let p = r.take(x)?;
    positions(p, n)?;
    let v = r.take(x.checked_mul(F::BYTES)?)?;
    for (&i, v) in p.iter().zip(v.chunks_exact(F::BYTES)) {
        let mut w = [0u8; 8];
        w.get_mut(..F::BYTES)?.copy_from_slice(v);
        *out.get_mut(usize::from(i))? = u64::from_le_bytes(w);
    }
    Some(())
}

/// An `rd` lane after its scheme byte into `out`; `rest` as in [decimal].
fn rd<F: Float>(
    r: &mut Reader<'_>,
    rest: &[u8],
    n: usize,
    out: &mut [u64; ALP_GROUP],
) -> Option<()> {
    let left = u32::from(r.u8()?);
    let zeros = u32::from(r.u8()?);
    let code_bits = u32::from(r.u8()?);
    if left > LEFT_MAX || code_bits > DICT_BITS_MAX {
        return None;
    }
    let right = F::BITS - left;
    let packed = right.checked_sub(zeros)?;
    let fits = |v: u16| u32::from(v) >> left == 0;
    let mut dict = [0u16; DICT_MAX];
    for e in dict.iter_mut().take(1 << code_bits) {
        let v = r.uint(2)? as u16;
        if !fits(v) {
            return None;
        }
        *e = v;
    }
    let x = usize::from(r.u8()?);
    if x > n {
        return None;
    }
    let codes_src = rest.get(r.at..)?;
    r.take(packed_len(n, code_bits))?;
    let right_src = rest.get(r.at..)?;
    r.take(packed_len(n, packed))?;
    let mut codes = [0u32; ALP_GROUP];
    bitpack::unpack_u32(codes_src, code_bits, &mut codes);
    bitpack::unpack_u64(right_src, packed, out);
    if zeros > 0 {
        for o in out.iter_mut() {
            *o = o.checked_shl(zeros).unwrap_or(0);
        }
    }
    // left << right; 0 when the left part is empty (right is the whole scalar)
    let glue = |l: u16| u64::from(l).checked_shl(right).unwrap_or(0);
    for (o, c) in out.iter_mut().zip(codes) {
        *o |= glue(dict.get(c as usize & (DICT_MAX - 1)).copied().unwrap_or(0));
    }
    let p = r.take(x)?;
    positions(p, n)?;
    let lefts = r.take(x.checked_mul(2)?)?;
    let right_mask = glue(1).wrapping_sub(1);
    for (&i, l) in p.iter().zip(lefts.as_chunks::<2>().0) {
        let l = u16::from_le_bytes(*l);
        if !fits(l) {
            return None;
        }
        let o = out.get_mut(usize::from(i))?;
        *o = glue(l) | (*o & right_mask);
    }
    Some(())
}

/// Decode a group of `n` samples, `len` bytes from the start of `rest` (the column from the group on), handing
/// out samples `skip..` into `dst`; None when it is malformed.
fn decode_group<F: Float, T: Sample>(
    rest: &[u8],
    len: usize,
    n: usize,
    lanes: usize,
    skip: usize,
    dst: &mut [T],
) -> Option<()> {
    let mut r = Reader {
        bytes: rest.get(..len)?,
        at: 0,
    };
    let mut vals = [0u64; ALP_GROUP];
    for l in 0..lanes {
        match r.u8()? {
            DECIMAL => decimal::<F>(&mut r, rest, n, &mut vals)?,
            RD => rd::<F>(&mut r, rest, n, &mut vals)?,
            _ => return None,
        }
        for (o, v) in dst
            .iter_mut()
            .skip(l)
            .step_by(lanes)
            .zip(vals.iter().skip(skip))
        {
            *o = T::from_bits(*v);
        }
    }
    (r.at == len).then_some(())
}

fn decode_with<F: Float, T: Sample>(
    bytes: &[u8],
    count: usize,
    lanes: usize,
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    let groups = count.div_ceil(ALP_GROUP);
    let n = out.len().checked_div(lanes).ok_or(SampleError::BadShape)?;
    let end = first.checked_add(n).ok_or(SampleError::OutOfRange)?;
    let mut s = first;
    while s < end {
        let g = s / ALP_GROUP;
        let corrupt = SampleError::Corrupt { group: g };
        let g_first = g * ALP_GROUP;
        let g_count = count.min(g_first + ALP_GROUP).saturating_sub(g_first);
        let skip = s - g_first;
        let take = (end - s).min(g_count.saturating_sub(skip));
        if take == 0 {
            return Err(SampleError::OutOfRange);
        }
        let range = super::super::table_entry(bytes, groups, g).ok_or(corrupt)?;
        let rest = bytes.get(range.start..).ok_or(corrupt)?;
        let at = (s - first) * lanes;
        let dst = out
            .get_mut(at..at + take * lanes)
            .ok_or(SampleError::OutOfRange)?;
        decode_group::<F, T>(rest, range.len(), g_count, lanes, skip, dst).ok_or(corrupt)?;
        s += take;
    }
    Ok(())
}

/// Decode samples from `first` on into `out` (whole samples of `lanes` scalars) from a column whose table was
/// checked.
pub(in crate::samples) fn decode<T: Sample>(
    bytes: &[u8],
    count: usize,
    lanes: usize,
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    match T::ELEMENT {
        Element::F32 => decode_with::<F32, T>(bytes, count, lanes, first, out),
        Element::F64 => decode_with::<F64, T>(bytes, count, lanes, first, out),
        element => Err(SampleError::Unsupported {
            codec: crate::Codec::ALP,
            element,
        }),
    }
}
