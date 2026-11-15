//! Structure-aware inputs: blocks and files whose structure is right but whose counts, lengths, offsets, widths,
//! levels and layout fields take hostile values drawn from the fuzzer's input.
//!
//! Each builder makes a valid block with ww_tape's own writers, sized by the input (at most about 1 KiB), then
//! writes values over fields at their documented offsets: 0, 1, the largest value of the field's type and one less,
//! the signed extremes, each named cap of the crate (`*_MAX`) and one more, the field's own value and the block's
//! length, each plus and minus one, or random bits. The CRCs are computed again afterwards (block CRC, chunk head
//! and segment CRCs, index head and track CRCs, a piece's content hash), so a decoder's checks past the CRC run;
//! a draw may leave a chunk's segment CRCs stale on purpose, which sends the chunk down the salvage path.
//!
//! The same input gives the same bytes: nothing here reads a clock or a random source.

mod chunk;
mod file;
mod index;
mod payloads;
mod records;
mod samples;
mod series;
mod summary;
mod times;

pub use chunk::chunk;
pub use file::file;
pub use index::index;
pub use payloads::payloads;
pub use records::records;
pub use samples::samples;
pub use series::series;
pub use summary::summary;
pub use times::times;

use ww_tape::{BlockId, BlockKind, BlockWriter, ClockTime, RunTag};

/// The run every hostile block is written in.
pub(crate) const RUN: RunTag = RunTag::writable(0x5eed);

const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);

/// Bytes of the block header.
pub(crate) const HEADER_LEN: usize = ww_tape::HEADER_LEN;

/// Values drawn from the fuzzer's input; past its end every draw is 0.
pub(crate) struct Draw<'a> {
    data: &'a [u8],
}

impl<'a> Draw<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Draw { data }
    }

    pub(crate) fn u8(&mut self) -> u8 {
        match self.data.split_first() {
            Some((&b, rest)) => {
                self.data = rest;
                b
            }
            None => 0,
        }
    }

    pub(crate) fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }

    pub(crate) fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }

    pub(crate) fn u64(&mut self) -> u64 {
        u64::from(self.u32()) | (u64::from(self.u32()) << 32)
    }

    pub(crate) fn bool(&mut self) -> bool {
        self.u8() & 1 == 1
    }

    /// A number below `n` (0 for an `n` of 0).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::from(self.u16()) % n
        }
    }

    /// A source of bulk values seeded by this draw.
    pub(crate) fn bits(&mut self) -> Bits {
        Bits(self.u64() | 1)
    }

    /// A number in `lo..=hi`.
    pub(crate) fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    /// A hostile value of a field of `bits` bits: see the [module](self) docs. `caps` are the named caps that bound
    /// the field, `own` the values it is checked against (its own value, the block's length).
    pub(crate) fn value(&mut self, bits: u32, caps: &[u64], own: &[u64]) -> u64 {
        let max = if bits >= 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        let pick = |d: &mut Self, of: &[u64]| of.get(d.below(of.len())).copied().unwrap_or(0);
        let v = match self.u8() % 14 {
            0 => pick(self, own),
            1 => 1,
            2 => max,
            3 => max - 1,
            4 => 1u64 << (bits.min(64) - 1),
            5 => (1u64 << (bits.min(64) - 1)) - 1,
            6 => pick(self, caps),
            7 => pick(self, caps).wrapping_add(1),
            8 => pick(self, caps).wrapping_sub(1),
            9 => 0,
            10 => pick(self, own).wrapping_add(1),
            11 => pick(self, own).wrapping_sub(1),
            12 => u64::from(self.u8()),
            _ => self.u64(),
        };
        v & max
    }
}

/// Bulk values from a xorshift seeded by one draw: many items cost the input 8 bytes, so the draws after them still
/// come from the input.
pub(crate) struct Bits(u64);

impl Bits {
    pub(crate) fn u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub(crate) fn u8(&mut self) -> u8 {
        self.u64() as u8
    }

    /// A number below `n` (0 for an `n` of 0).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.u64() % n as u64) as usize
        }
    }
}

/// A field of a block: `width` bytes, little endian, at `at`, bounded by `caps`.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Field<'c> {
    pub(crate) at: usize,
    pub(crate) width: usize,
    pub(crate) caps: &'c [u64],
}

impl<'c> Field<'c> {
    pub(crate) const fn new(at: usize, width: usize, caps: &'c [u64]) -> Self {
        Field { at, width, caps }
    }
}

/// The value of `width` bytes at `at`, 0 past the end.
pub(crate) fn get(b: &[u8], at: usize, width: usize) -> u64 {
    let mut v = [0u8; 8];
    if let Some(src) = b.get(at..at + width) {
        v[..width].copy_from_slice(src);
    }
    u64::from_le_bytes(v)
}

/// Write the low `width` bytes of `v` at `at`, if the field is inside `b`.
pub(crate) fn put(b: &mut [u8], at: usize, width: usize, v: u64) {
    if let Some(dst) = b.get_mut(at..at + width) {
        dst.copy_from_slice(&v.to_le_bytes()[..width]);
    }
}

/// Write a hostile value over `field` of `b`.
pub(crate) fn hit(d: &mut Draw<'_>, b: &mut [u8], field: Field<'_>) {
    let own = [get(b, field.at, field.width), b.len() as u64];
    let v = d.value(8 * field.width as u32, field.caps, &own);
    put(b, field.at, field.width, v);
}

/// Write hostile values over one to three of `fields`.
pub(crate) fn hit_some(d: &mut Draw<'_>, b: &mut [u8], fields: &[Field<'_>]) {
    if fields.is_empty() {
        return;
    }
    for _ in 0..1 + d.below(3) {
        let f = fields[d.below(fields.len())];
        hit(d, b, f);
    }
}

/// The header fields every block has, but the magic and the length: version, kind, flags, count, seq, run.
pub(crate) const HEADER_FIELDS: [Field<'static>; 6] = [
    Field::new(4, 1, &[ww_tape::VERSION as u64]),
    Field::new(5, 1, &[]),
    Field::new(6, 2, &[]),
    Field::new(12, 4, &[]),
    Field::new(16, 8, &[]),
    Field::new(24, 4, &[]),
];

/// The block length field: rarely hit, since every other length then fails the block at its first check.
pub(crate) const LEN_FIELD: Field<'static> = Field::new(
    8,
    4,
    &[ww_tape::MAX_BLOCK_LEN as u64, (HEADER_LEN + 4) as u64],
);

/// CRC-32C of `parts`, one after the other.
pub(crate) fn crc32c(parts: &[&[u8]]) -> u32 {
    let mut d = CRC.digest();
    for p in parts {
        d.update(p);
    }
    d.finalize()
}

/// Write the block CRC over all of `b` but its last 4 bytes.
pub(crate) fn seal(b: &mut [u8]) {
    if b.len() < HEADER_LEN + 4 {
        return;
    }
    let end = b.len() - 4;
    let crc = crc32c(&[&b[..end]]);
    b[end..].copy_from_slice(&crc.to_le_bytes());
}

/// Bytes `from..to` of `b`, when both are inside it and in order.
pub(crate) fn span(b: &[u8], from: usize, to: usize) -> Option<&[u8]> {
    b.get(from..to)
}

/// A describe block holding `items` as describe records, marked a whole set.
pub(crate) fn describe_block(id: BlockId, items: &[&[u8]]) -> Vec<u8> {
    let mut buf = vec![0u8; 4096];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, id, &[]).expect("a describe block");
    for item in items {
        w.push_describe(&[item]).expect("items fit 4 KiB");
    }
    w.set_flags(ww_tape::FLAG_SET_FIRST | ww_tape::FLAG_SET_LAST);
    let n = w.finish();
    buf.truncate(n);
    buf
}

/// A records block of `records` (track, tick, item) on clock 1.
pub(crate) fn records_block(id: BlockId, records: &[(u32, i64, &[u8])]) -> Vec<u8> {
    let mut buf = vec![0u8; 4096];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]).expect("a records block");
    for (track, tick, item) in records {
        if w.push(*track, ClockTime::new(1, *tick), &[item]).is_err() {
            break;
        }
    }
    let n = w.finish();
    buf.truncate(n);
    buf
}
