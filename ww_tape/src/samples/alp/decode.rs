//! The ALP decoder: every read of the column checked, a malformed group an error.

use super::{ALP_GROUP, DECIMAL, DICT_BITS_MAX, DICT_MAX, F32, F64, Float, LEFT_MAX, RD};
use crate::FromFile;
use crate::bitpack::{Width, packed_len};
use crate::samples::raw::sign_extend;
use crate::samples::{Element, GroupJob, GroupReader, Sample, SampleError, decode_grouped};

/// Exception positions: ascending, below `n`.
fn positions(p: &[u8], n: usize) -> Option<()> {
    let mut next = 0usize;
    for &i in p {
        let i = usize::from(i);
        if i < next || i >= n {
            return None;
        }
        next = i.checked_add(1)?;
    }
    Some(())
}

/// A decimal lane after its scheme byte into `out`; `rest` is the column from the group's start on, so the unpack
/// kernels may read past the lane.
fn decimal<F: Float>(
    r: &mut GroupReader<'_>,
    rest: &[u8],
    n: usize,
    out: &mut [u64; ALP_GROUP],
) -> Option<()> {
    let e = r.u8()?;
    let f = r.u8()?;
    let m = F::factors(e, f)?;
    let bits = F::BITS;
    let base = sign_extend(r.uint(F::BYTES)?, bits);
    let width = Width::from_file(FromFile::new(r.u8()?))
        .ok()
        .filter(|w| w.bits() <= bits)?;
    let x = usize::from(r.u8()?);
    if x > n {
        return None;
    }
    let packed = rest.get(r.at..)?;
    r.take(packed_len(n, width.bits()))?;
    let mut d = [0i64; ALP_GROUP];
    width.decode_line(packed, base, 0, &mut d);
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
    r: &mut GroupReader<'_>,
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
    let right = F::BITS.checked_sub(left)?;
    let packed = right.checked_sub(zeros)?;
    let fits = |v: u16| u32::from(v).checked_shr(left).is_none_or(|top| top == 0);
    let mut dict = [0u16; DICT_MAX];
    for e in dict.iter_mut().take(1usize.checked_shl(code_bits)?) {
        let v = r.u16()?;
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
    // code_bits is at most DICT_BITS_MAX, packed at most the float's bits
    Width::new(code_bits)
        .ok()?
        .unpack_u32(codes_src, &mut codes)
        .ok()?;
    Width::new(packed).ok()?.unpack_u64(right_src, out);
    if zeros > 0 {
        // a shift past the scalar leaves no right bit
        for o in out.iter_mut() {
            *o = o.checked_shl(zeros).unwrap_or(0);
        }
    }
    // left << right; 0 when the left part is empty (right is the whole scalar)
    let glue = |l: u16| u64::from(l).checked_shl(right).unwrap_or(0);
    for (o, c) in out.iter_mut().zip(codes) {
        // a code has at most DICT_BITS_MAX bits: always a dictionary entry
        *o |= glue(*dict.get(usize::try_from(c).ok()?)?);
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

/// Decode a group, handing out the samples asked for; None when it is malformed.
fn decode_group<F: Float, T: Sample>(lanes: usize, job: GroupJob<'_, '_, T>) -> Option<()> {
    let GroupJob {
        rest,
        len,
        count,
        skip,
        dst,
    } = job;
    let mut r = GroupReader {
        bytes: rest.get(..len)?,
        at: 0,
    };
    let mut vals = [0u64; ALP_GROUP];
    for l in 0..lanes {
        match r.u8()? {
            DECIMAL => decimal::<F>(&mut r, rest, count, &mut vals)?,
            RD => rd::<F>(&mut r, rest, count, &mut vals)?,
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
        Element::F32 => decode_grouped(bytes, count, lanes, first, out, ALP_GROUP, |job| {
            decode_group::<F32, T>(lanes, job)
        }),
        Element::F64 => decode_grouped(bytes, count, lanes, first, out, ALP_GROUP, |job| {
            decode_group::<F64, T>(lanes, job)
        }),
        element => Err(SampleError::Unsupported {
            codec: crate::Codec::ALP,
            element,
        }),
    }
}
