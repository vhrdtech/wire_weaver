//! Delta codecs: first- or second-order differences, frame of reference, bit-packed per group of [DELTA_GROUP]
//! samples on the shared [bitpack](crate::bitpack) primitive.

use super::raw::{mask, sign_extend};
use super::{
    DELTA_GROUP, ENTRY, GroupJob, GroupReader, GroupWriter, Sample, SampleError, decode_grouped,
};
use crate::FromFile;
use crate::bitpack::{self, Width};

/// How a delta codec sees values.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) struct Flavor {
    /// 1: differences of values; 2: differences of differences.
    pub order: u8,
}

/// The most bytes `count` samples of `values` scalars of `w` bits code into: the table and, per group and lane, the
/// header (first value, first difference for order 2, minimum, width byte) with a spare byte, and the values at
/// full width. None past what the table's `u32` ends can address.
pub(super) fn max_len(order: u8, count: usize, values: usize, w: u32) -> Option<usize> {
    let groups = count.div_ceil(DELTA_GROUP);
    let lanes = values.checked_div(count).unwrap_or(0);
    let bytes = usize::try_from(w / 8).ok()?;
    let header = usize::from(order)
        .checked_add(1)?
        .checked_mul(bytes)?
        .checked_add(2)?;
    let headers = groups.checked_mul(lanes)?.checked_mul(header)?;
    let data = values.checked_mul(bytes)?.checked_add(headers)?;
    u32::try_from(data).ok()?;
    groups.checked_mul(ENTRY)?.checked_add(data)
}

/// Code `values` (samples of `lanes` scalars) into `out`, sized by [max_len]; returns the bytes written.
pub(super) fn encode<T: Sample>(
    flavor: Flavor,
    values: &[T],
    lanes: usize,
    out: &mut [u8],
) -> Result<usize, SampleError> {
    let full = SampleError::Full { needed: out.len() };
    let count = values
        .len()
        .checked_div(lanes)
        .ok_or(SampleError::BadShape)?;
    let table = count
        .div_ceil(DELTA_GROUP)
        .checked_mul(ENTRY)
        .ok_or(SampleError::BadShape)?;
    let (entries, data) = out.split_at_mut_checked(table).ok_or(full)?;
    let mut w = GroupWriter { out: data, at: 0 };
    let group_values = DELTA_GROUP
        .checked_mul(lanes)
        .ok_or(SampleError::BadShape)?;
    for (group, entry) in values
        .chunks(group_values)
        .zip(entries.as_chunks_mut::<ENTRY>().0)
    {
        for l in 0..lanes {
            encode_lane(flavor, &mut w, group.iter().skip(l).step_by(lanes)).ok_or(full)?;
        }
        // max_len keeps the data under 4 GiB
        *entry = u32::try_from(w.at).ok().ok_or(full)?.to_le_bytes();
    }
    table.checked_add(w.at).ok_or(full)
}

/// Code a group's lane: its header and its differences minus the minimum, bit-packed.
fn encode_lane<'v, T: Sample + 'v>(
    flavor: Flavor,
    w: &mut GroupWriter<'_>,
    lane: impl Iterator<Item = &'v T>,
) -> Option<()> {
    let bits = T::ELEMENT.scalar_bits();
    let bytes = size_of::<T>();
    let m = mask(bits);
    let mut vals = [0u64; DELTA_GROUP];
    let mut n = 0usize;
    for (x, v) in vals.iter_mut().zip(lane) {
        *x = v.to_bits();
        n = n.checked_add(1)?;
    }
    let vals = vals.get(..n)?;
    w.uint(*vals.first()?, bytes)?;
    // differences, wrapping in the scalar's bits, then of them again for order 2
    let mut d = [0u64; DELTA_GROUP];
    for (o, (a, b)) in d.iter_mut().zip(vals.iter().zip(vals.iter().skip(1))) {
        *o = b.wrapping_sub(*a) & m;
    }
    // the differences: one fewer than the values, none of a single value
    let mut k = n.saturating_sub(1);
    if flavor.order == 2 {
        let first_delta = if k > 0 { *d.first()? } else { 0 };
        w.uint(first_delta, bytes)?;
        let mut dd = [0u64; DELTA_GROUP];
        for (o, (a, b)) in dd.iter_mut().zip(d.iter().zip(d.iter().skip(1))) {
            *o = b.wrapping_sub(*a) & m;
        }
        d = dd;
        k = k.saturating_sub(1);
    }
    let d = d.get(..k)?;
    // no difference: minimum 0
    let min = d
        .iter()
        .map(|x| sign_extend(*x, bits))
        .min()
        .unwrap_or(0)
        .cast_unsigned();
    let mut residuals = [0u64; DELTA_GROUP];
    let mut top = 0u64;
    for (r, x) in residuals.iter_mut().zip(d) {
        *r = x.wrapping_sub(min) & m;
        top |= *r;
    }
    let width = bitpack::width_of(top);
    w.uint(min & m, bytes)?;
    w.u8(u8::try_from(width).ok()?)?;
    w.packed(residuals.iter().copied(), k, width)
}

/// Check the group table of a column of `count` samples.
pub(super) fn check_table(bytes: &[u8], count: usize) -> Result<(), SampleError> {
    super::check_table(bytes, count.div_ceil(DELTA_GROUP))
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
    decode_grouped(bytes, count, lanes, first, out, DELTA_GROUP, |job| {
        decode_group(flavor, lanes, job)
    })
}

/// Decode a group, handing out the samples asked for; None when it is malformed.
fn decode_group<T: Sample>(flavor: Flavor, lanes: usize, job: GroupJob<'_, '_, T>) -> Option<()> {
    let GroupJob {
        rest,
        len,
        count,
        skip,
        dst,
    } = job;
    let bits = T::ELEMENT.scalar_bits();
    let bytes = size_of::<T>();
    let take = dst.len().checked_div(lanes)?;
    // a value takes the residuals before it: the samples asked for are within the group
    if skip.checked_add(take)? > count.min(DELTA_GROUP) {
        return None;
    }
    // the residuals: one per value past the order, none in a group shorter than it
    let k = count.saturating_sub(usize::from(flavor.order));
    let mut r = GroupReader {
        bytes: rest.get(..len)?,
        at: 0,
    };
    let mut residuals = [0u64; DELTA_GROUP];
    for l in 0..lanes {
        let first = r.uint(bytes)?;
        let first_delta = if flavor.order == 2 { r.uint(bytes)? } else { 0 };
        let min = r.uint(bytes)?;
        let width = Width::from_file(FromFile::new(r.u8()?))
            .ok()
            .filter(|w| w.bits() <= bits)?;
        // the bytes after the lane stay in reach of the unpack kernels
        let lane_src = rest.get(r.at..)?;
        r.take(bitpack::packed_len(k, width.bits()))?;
        if take == 0 {
            continue;
        }
        width.unpack_u64(lane_src, &mut residuals);
        let header = [first, first_delta, min];
        if lanes == 1 {
            rebuild(flavor, header, &residuals, skip, dst.iter_mut());
        } else {
            let slots = dst.chunks_exact_mut(lanes).filter_map(|s| s.get_mut(l));
            rebuild(flavor, header, &residuals, skip, slots);
        }
    }
    (r.at == len).then_some(())
}

/// Rebuild a lane from its header (first value, first difference, minimum) and residuals, writing the values from
/// `skip` on into `slots`. Value `i` takes the first `i` residuals, so `skip` and the slots are at most
/// [DELTA_GROUP] together. Differences wrap in the scalar's bits: the format's meaning.
#[inline(always)]
fn rebuild<'o, T: Sample + 'o>(
    flavor: Flavor,
    [first, first_delta, min]: [u64; 3],
    residuals: &[u64; DELTA_GROUP],
    skip: usize,
    slots: impl Iterator<Item = &'o mut T>,
) {
    let mut res = residuals.iter().map(|r| r.wrapping_add(min));
    let mut v = first;
    if flavor.order == 1 {
        for step in res.by_ref().take(skip) {
            v = v.wrapping_add(step);
        }
        for (o, step) in slots.zip(res) {
            *o = T::from_bits(v);
            v = v.wrapping_add(step);
        }
    } else {
        // `d` is the difference to the next value
        let mut d = first_delta;
        for step in res.by_ref().take(skip) {
            v = v.wrapping_add(d);
            d = d.wrapping_add(step);
        }
        for (o, step) in slots.zip(res) {
            *o = T::from_bits(v);
            v = v.wrapping_add(d);
            d = d.wrapping_add(step);
        }
    }
}
