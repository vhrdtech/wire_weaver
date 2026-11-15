//! Delta codecs: first- or second-order differences, frame of reference, bit-packed per group of [DELTA_GROUP]
//! samples on the shared [bitpack](crate::bitpack) primitive.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unreachable,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "STD-131: hardening of this module in progress"
)]

use super::{DELTA_GROUP, Sample, SampleError};
use crate::bitpack;
use core::ops::Range;

use super::ENTRY;

/// How a delta codec sees values.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) struct Flavor {
    /// 1: differences of values; 2: differences of differences.
    pub order: u8,
}

#[inline(always)]
const fn mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// Sign-extend the low `bits` of `v`.
#[inline(always)]
fn signed(v: u64, bits: u32) -> i64 {
    ((v << (64 - bits)) as i64) >> (64 - bits)
}

/// Bytes of a lane's header in a group: first value, first difference (order 2), the minimum, the width byte.
const fn header_len(order: u8, bytes: usize) -> usize {
    bytes * (1 + order as usize) + 1
}

/// The most bytes `count` samples of `values` scalars of `w` bits code into.
pub(super) fn max_len(order: u8, count: usize, values: usize, w: u32) -> Option<usize> {
    let groups = count.div_ceil(DELTA_GROUP);
    let lanes = values.checked_div(count).unwrap_or(0);
    let headers = groups
        .checked_mul(lanes)?
        .checked_mul(header_len(order, (w / 8) as usize) + 1)?;
    let packed = values.checked_mul((w / 8) as usize)?;
    (groups * ENTRY).checked_add(headers)?.checked_add(packed)
}

fn put(out: &mut [u8], at: &mut usize, v: u64, bytes: usize) {
    out[*at..*at + bytes].copy_from_slice(&v.to_le_bytes()[..bytes]);
    *at += bytes;
}

fn get(src: &[u8], at: &mut usize, bytes: usize) -> Option<u64> {
    let b = src.get(*at..*at + bytes)?;
    let mut w = [0u8; 8];
    w[..bytes].copy_from_slice(b);
    *at += bytes;
    Some(u64::from_le_bytes(w))
}

/// Code `values` (samples of `lanes` scalars) into `out`, sized by [max_len]; returns the bytes written.
pub(super) fn encode<T: Sample>(
    flavor: Flavor,
    values: &[T],
    lanes: usize,
    out: &mut [u8],
) -> usize {
    let w = T::ELEMENT.scalar_bits();
    let bytes = (w / 8) as usize;
    let m = mask(w);
    let count = values.len() / lanes;
    let groups = count.div_ceil(DELTA_GROUP);
    let table = groups * ENTRY;
    let mut at = table;
    let mut residuals = [0u64; DELTA_GROUP];
    for g in 0..groups {
        let group = &values[g * DELTA_GROUP * lanes..count.min((g + 1) * DELTA_GROUP) * lanes];
        let n = group.len() / lanes;
        for l in 0..lanes {
            let mut lane = [0u64; DELTA_GROUP];
            for (x, v) in lane.iter_mut().zip(group[l..].iter().step_by(lanes)) {
                *x = v.to_bits();
            }
            let lane = &lane[..n];
            put(out, &mut at, lane[0], bytes);
            // differences, wrapping in w bits, then of them again for order 2
            let mut d = [0u64; DELTA_GROUP];
            for j in 1..n {
                d[j - 1] = lane[j].wrapping_sub(lane[j - 1]) & m;
            }
            let mut k = n - 1;
            if flavor.order == 2 {
                put(out, &mut at, if n > 1 { d[0] } else { 0 }, bytes);
                for j in 1..k {
                    d[j - 1] = d[j].wrapping_sub(d[j - 1]) & m;
                }
                k = k.saturating_sub(1);
            }
            let d = &d[..k];
            let min = d.iter().map(|x| signed(*x, w)).min().unwrap_or(0);
            let mut top = 0u64;
            for (r, x) in residuals.iter_mut().zip(d) {
                *r = x.wrapping_sub(min as u64) & m;
                top |= *r;
            }
            let width = bitpack::width_of(top);
            put(out, &mut at, min as u64 & m, bytes);
            out[at] = width as u8;
            at += 1;
            at += bitpack::pack(&mut out[at..], residuals[..k].iter().copied(), width);
        }
        let end = u32::try_from(at - table).expect("a column under 4 GiB");
        out[g * ENTRY..(g + 1) * ENTRY].copy_from_slice(&end.to_le_bytes());
    }
    at
}

/// Check the group table of a column of `count` samples.
pub(super) fn check_table(bytes: &[u8], count: usize) -> Result<(), SampleError> {
    super::check_table(bytes, count.div_ceil(DELTA_GROUP))
}

/// The bytes of group `g` in the column; the table was checked.
pub(super) fn group_range(bytes: &[u8], count: usize, g: usize) -> Range<usize> {
    super::table_range(bytes, count.div_ceil(DELTA_GROUP), g)
}

/// Decode samples from `first` on into `out` (whole samples of `lanes` scalars) from a column whose table was
/// checked.
pub(super) fn decode<T: Sample>(
    flavor: Flavor,
    bytes: &[u8],
    count: usize,
    lanes: usize,
    first: usize,
    out: &mut [T],
) -> Result<(), SampleError> {
    let n = out.len() / lanes;
    let mut s = first;
    while s < first + n {
        let g = s / DELTA_GROUP;
        let g_first = g * DELTA_GROUP;
        let g_count = count.min(g_first + DELTA_GROUP) - g_first;
        let skip = s - g_first;
        let take = (first + n - s).min(g_count - skip);
        let range = group_range(bytes, count, g);
        // the bytes after the group stay in reach of the unpack kernels
        let src = &bytes[range.start..];
        let dst = &mut out[(s - first) * lanes..(s - first + take) * lanes];
        decode_group(flavor, src, range.len(), g_count, lanes, skip, dst)
            .ok_or(SampleError::Corrupt { group: g })?;
        s += take;
    }
    Ok(())
}

/// Decode a group of `g_count` samples whose `len` bytes start `src`, handing out the samples `skip..` into `dst`;
/// None when it is malformed.
fn decode_group<T: Sample>(
    flavor: Flavor,
    src: &[u8],
    len: usize,
    g_count: usize,
    lanes: usize,
    skip: usize,
    dst: &mut [T],
) -> Option<()> {
    let w = T::ELEMENT.scalar_bits();
    let bytes = (w / 8) as usize;
    let take = dst.len() / lanes;
    let k = g_count.saturating_sub(usize::from(flavor.order));
    let group = &src[..len];
    let mut at = 0;
    let mut residuals = [0u64; DELTA_GROUP];
    for l in 0..lanes {
        let first = get(group, &mut at, bytes)?;
        let first_delta = if flavor.order == 2 {
            get(group, &mut at, bytes)?
        } else {
            0
        };
        let min = get(group, &mut at, bytes)?;
        let width = u32::from(*group.get(at)?);
        at += 1;
        if width > w {
            return None;
        }
        let packed = bitpack::packed_len(k, width);
        if at + packed > len {
            return None;
        }
        let lane_src = &src[at..];
        at += packed;
        let need = skip + take;
        if need == 0 {
            continue;
        }
        bitpack::unpack_u64(lane_src, width, &mut residuals);
        let mut v = first;
        let mut d = first_delta;
        let emit = |i: usize, v: u64, dst: &mut [T]| {
            if i >= skip {
                dst[(i - skip) * lanes + l] = T::from_bits(v);
            }
        };
        emit(0, v, dst);
        if flavor.order == 1 {
            for i in 1..need {
                v = v.wrapping_add(residuals[i - 1].wrapping_add(min));
                emit(i, v, dst);
            }
        } else {
            for i in 1..need {
                if i >= 2 {
                    d = d.wrapping_add(residuals[i - 2].wrapping_add(min));
                }
                v = v.wrapping_add(d);
                emit(i, v, dst);
            }
        }
    }
    (at == len).then_some(())
}
