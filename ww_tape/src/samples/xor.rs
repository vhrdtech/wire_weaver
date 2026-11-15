//! XOR float codecs: Gorilla and Chimp control bits, in groups of [XOR_GROUP] samples.

use super::{Sample, SampleError, XOR_GROUP};
use core::ops::Range;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Flavor {
    Gorilla,
    Chimp,
}

/// Bytes of the group table entry.
const ENTRY: usize = 4;

/// Chimp's leading zeros, rounded down, by index.
const CHIMP_LEADING: [u32; 8] = [0, 8, 12, 16, 18, 20, 22, 24];

/// Index in [CHIMP_LEADING] of `lz` leading zeros, rounded down.
#[inline(always)]
fn chimp_index(lz: u32) -> u32 {
    match lz {
        0..=7 => 0,
        8..=11 => 1,
        12..=15 => 2,
        16..=17 => 3,
        18..=19 => 4,
        20..=21 => 5,
        22..=23 => 6,
        _ => 7,
    }
}

/// Bits of the meaningful-length field: 5 for 32-bit values, 6 for 64-bit.
#[inline(always)]
const fn len_bits(w: u32) -> u32 {
    if w == 32 { 5 } else { 6 }
}

/// Trailing zeros above which Chimp codes the centre bits only.
#[inline(always)]
const fn chimp_threshold(w: u32) -> u32 {
    if w == 32 { 5 } else { 6 }
}

/// The most bytes `count` samples of `values` scalars of `w` bits code into: the table, the worst case of both
/// flavours (`w` plus 14 bits a value) and a byte of padding per group.
pub(super) fn max_len(count: usize, values: usize, w: u32) -> Option<usize> {
    let groups = count.div_ceil(XOR_GROUP);
    let bits = values.checked_mul(w as usize + 14)?;
    (groups * (ENTRY + 1)).checked_add(bits.div_ceil(8))
}

/// Writes bits from the top of each byte down into a buffer the caller sized.
struct BitWriter<'o> {
    out: &'o mut [u8],
    at: usize,
    acc: u64,
    n: u32,
}

impl<'o> BitWriter<'o> {
    /// Append the low `bits` bits of `v` (`bits` up to 64; the bits above them are zero).
    #[inline(always)]
    fn put(&mut self, v: u64, bits: u32) {
        if bits == 0 {
            return;
        }
        if bits > 32 {
            self.put(v >> 32, bits - 32);
            self.put(v & 0xffff_ffff, 32);
            return;
        }
        self.acc = (self.acc << bits) | v;
        self.n += bits;
        while self.n >= 8 {
            self.n -= 8;
            self.out[self.at] = (self.acc >> self.n) as u8;
            self.at += 1;
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    /// Pad to a byte with zeros; returns the bytes written so far.
    fn flush(&mut self) -> usize {
        if self.n > 0 {
            self.put(0, 8 - self.n);
        }
        self.at
    }
}

/// Reads bits from the top of each byte down, refusing to read past its bytes.
struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    /// The next `bits` bits (up to 57), or None past the end.
    #[inline(always)]
    fn get(&mut self, bits: u32) -> Option<u64> {
        if bits == 0 {
            return Some(0);
        }
        let end = self.bit + bits as usize;
        if end > self.bytes.len() * 8 {
            return None;
        }
        let at = self.bit / 8;
        let word = if at + 8 <= self.bytes.len() {
            u64::from_be_bytes(self.bytes[at..at + 8].try_into().expect("8 bytes"))
        } else {
            let mut w = [0u8; 8];
            w[..self.bytes.len() - at].copy_from_slice(&self.bytes[at..]);
            u64::from_be_bytes(w)
        };
        let v = (word << (self.bit % 8)) >> (64 - bits);
        self.bit = end;
        Some(v)
    }

    /// The next `bits` bits, up to 64.
    #[inline(always)]
    fn get_wide(&mut self, bits: u32) -> Option<u64> {
        if bits > 32 {
            let hi = self.get(bits - 32)?;
            Some((hi << 32) | self.get(32)?)
        } else {
            self.get(bits)
        }
    }
}

/// What a lane remembers between values.
#[derive(Copy, Clone, Default)]
struct Lane {
    prev: u64,
    /// Gorilla: the window's leading zeros and length (0: no window); Chimp: the kept rounded leading zeros (u32::MAX:
    /// none).
    lead: u32,
    len: u32,
}

impl Lane {
    fn new(flavor: Flavor, first: u64) -> Self {
        Lane {
            prev: first,
            lead: if flavor == Flavor::Chimp { u32::MAX } else { 0 },
            len: 0,
        }
    }
}

#[inline(always)]
fn put_gorilla(wr: &mut BitWriter<'_>, lane: &mut Lane, v: u64, w: u32) {
    let x = lane.prev ^ v;
    lane.prev = v;
    if x == 0 {
        wr.put(0, 1);
        return;
    }
    // leading zeros of the w-bit value
    let lead = (x.leading_zeros() - (64 - w)).min(31);
    let trail = x.trailing_zeros();
    if lane.len != 0 && lead >= lane.lead && trail >= w - lane.lead - lane.len {
        wr.put(0b10, 2);
        wr.put(x >> (w - lane.lead - lane.len), lane.len);
    } else {
        let len = w - lead - trail;
        wr.put(0b11, 2);
        wr.put(u64::from(lead), 5);
        wr.put(u64::from(len % w), len_bits(w));
        wr.put(x >> trail, len);
        lane.lead = lead;
        lane.len = len;
    }
}

#[inline(always)]
fn get_gorilla(rd: &mut BitReader<'_>, lane: &mut Lane, w: u32) -> Option<u64> {
    if rd.get(1)? == 0 {
        return Some(lane.prev);
    }
    if rd.get(1)? == 1 {
        let lead = rd.get(5)? as u32;
        let mut len = rd.get(len_bits(w))? as u32;
        if len == 0 {
            len = w;
        }
        if lead + len > w {
            return None;
        }
        lane.lead = lead;
        lane.len = len;
    } else if lane.len == 0 {
        return None;
    }
    let bits = rd.get_wide(lane.len)?;
    lane.prev ^= bits << (w - lane.lead - lane.len);
    Some(lane.prev)
}

#[inline(always)]
fn put_chimp(wr: &mut BitWriter<'_>, lane: &mut Lane, v: u64, w: u32) {
    let x = lane.prev ^ v;
    lane.prev = v;
    if x == 0 {
        wr.put(0b00, 2);
        lane.lead = u32::MAX;
        return;
    }
    let index = chimp_index(x.leading_zeros() - (64 - w));
    let lead = CHIMP_LEADING[index as usize];
    let trail = x.trailing_zeros();
    if trail > chimp_threshold(w) {
        let len = w - lead - trail;
        wr.put(0b01, 2);
        wr.put(u64::from(index), 3);
        wr.put(u64::from(len), len_bits(w));
        wr.put(x >> trail, len);
        lane.lead = u32::MAX;
    } else if lead == lane.lead {
        wr.put(0b10, 2);
        wr.put(x, w - lead);
    } else {
        wr.put(0b11, 2);
        wr.put(u64::from(index), 3);
        wr.put(x, w - lead);
        lane.lead = lead;
    }
}

#[inline(always)]
fn get_chimp(rd: &mut BitReader<'_>, lane: &mut Lane, w: u32) -> Option<u64> {
    let x = match rd.get(2)? {
        0b00 => {
            lane.lead = u32::MAX;
            0
        }
        0b01 => {
            let lead = CHIMP_LEADING[rd.get(3)? as usize];
            let len = rd.get(len_bits(w))? as u32;
            if len == 0 || lead + len > w {
                return None;
            }
            lane.lead = u32::MAX;
            rd.get_wide(len)? << (w - lead - len)
        }
        0b10 => {
            if lane.lead == u32::MAX {
                return None;
            }
            rd.get_wide(w - lane.lead)?
        }
        _ => {
            let lead = CHIMP_LEADING[rd.get(3)? as usize];
            lane.lead = lead;
            rd.get_wide(w - lead)?
        }
    };
    lane.prev ^= x;
    Some(lane.prev)
}

/// Code `values` (samples of `lanes` scalars) into `out`, sized by [max_len]; returns the bytes written.
pub(super) fn encode<T: Sample>(
    flavor: Flavor,
    values: &[T],
    lanes: usize,
    out: &mut [u8],
) -> usize {
    let w = T::ELEMENT.scalar_bits();
    let count = values.len() / lanes;
    let groups = count.div_ceil(XOR_GROUP);
    let (table, data) = out.split_at_mut(groups * ENTRY);
    let mut wr = BitWriter {
        out: data,
        at: 0,
        acc: 0,
        n: 0,
    };
    for (g, entry) in table.as_chunks_mut::<ENTRY>().0.iter_mut().enumerate() {
        let group = &values[g * XOR_GROUP * lanes..count.min((g + 1) * XOR_GROUP) * lanes];
        for l in 0..lanes {
            let mut lane_values = group[l..].iter().step_by(lanes).map(|v| v.to_bits());
            let first = lane_values.next().expect("a group has a sample");
            wr.put(first, w);
            let mut lane = Lane::new(flavor, first);
            for v in lane_values {
                match flavor {
                    Flavor::Gorilla => put_gorilla(&mut wr, &mut lane, v, w),
                    Flavor::Chimp => put_chimp(&mut wr, &mut lane, v, w),
                }
            }
        }
        let end = wr.flush();
        entry.copy_from_slice(
            &u32::try_from(end)
                .expect("a column under 4 GiB")
                .to_le_bytes(),
        );
    }
    groups * ENTRY + wr.at
}

/// Check the group table of a column of `count` samples: ascending ends, the last one the stream's length.
pub(super) fn check_table(bytes: &[u8], count: usize) -> Result<(), SampleError> {
    let bad = SampleError::BadLength { len: bytes.len() };
    let groups = count.div_ceil(XOR_GROUP);
    let table = groups.checked_mul(ENTRY).ok_or(bad)?;
    if bytes.len() < table {
        return Err(bad);
    }
    let mut prev = 0usize;
    for e in bytes[..table].as_chunks::<ENTRY>().0 {
        let end = u32::from_le_bytes(*e) as usize;
        if end < prev {
            return Err(bad);
        }
        prev = end;
    }
    if prev != bytes.len() - table {
        return Err(bad);
    }
    Ok(())
}

/// The bytes of group `g` in the column, table included in the offsets; the table was checked.
pub(super) fn group_range(bytes: &[u8], count: usize, g: usize) -> Range<usize> {
    let table = count.div_ceil(XOR_GROUP) * ENTRY;
    let end_of = |i: usize| {
        u32::from_le_bytes(
            bytes[i * ENTRY..(i + 1) * ENTRY]
                .try_into()
                .expect("4 bytes"),
        ) as usize
    };
    let from = if g == 0 { 0 } else { end_of(g - 1) };
    table + from..table + end_of(g)
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
    let w = T::ELEMENT.scalar_bits();
    let n = out.len() / lanes;
    let mut s = first;
    while s < first + n {
        let g = s / XOR_GROUP;
        let g_first = g * XOR_GROUP;
        let g_count = count.min(g_first + XOR_GROUP) - g_first;
        // samples of this group to hand out: skip..take
        let skip = s - g_first;
        let take = (first + n - s).min(g_count - skip);
        let dst = &mut out[(s - first) * lanes..(s - first + take) * lanes];
        decode_group(
            flavor,
            &bytes[group_range(bytes, count, g)],
            g_count,
            lanes,
            w,
            skip,
            dst,
        )
        .ok_or(SampleError::Corrupt { group: g })?;
        s += take;
    }
    Ok(())
}

/// Decode a group of `g_count` samples, handing out the samples `skip..` into `dst`; None when its bits are
/// malformed.
fn decode_group<T: Sample>(
    flavor: Flavor,
    bytes: &[u8],
    g_count: usize,
    lanes: usize,
    w: u32,
    skip: usize,
    dst: &mut [T],
) -> Option<()> {
    let take = dst.len() / lanes;
    let mut rd = BitReader { bytes, bit: 0 };
    for l in 0..lanes {
        let first = rd.get_wide(w)?;
        let mut lane = Lane::new(flavor, first);
        if skip == 0 {
            dst[l] = T::from_bits(first);
        }
        // a lane is read to the group's end even past `take`: the next lane starts after it
        let last = if l + 1 == lanes { skip + take } else { g_count };
        for i in 1..last {
            let v = match flavor {
                Flavor::Gorilla => get_gorilla(&mut rd, &mut lane, w)?,
                Flavor::Chimp => get_chimp(&mut rd, &mut lane, w)?,
            };
            if i >= skip && i < skip + take {
                dst[(i - skip) * lanes + l] = T::from_bits(v);
            }
        }
    }
    Some(())
}
