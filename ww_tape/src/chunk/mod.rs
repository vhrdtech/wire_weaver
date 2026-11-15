//! Chunk blocks: one chunk of one track, its columns read in place.
//!
//! An events chunk ([BlockKind::Events]) holds up to a few thousand items of an event track, a series chunk
//! ([BlockKind::Series]) a run of samples of a fixed-rate series. The block header's count (bytes 12..16) is the
//! chunk's item or sample count; bytes 24..32 hold where the payload starts and a CRC of the head:
//!
//! ```text
//! off size field
//!  24 u32  payload offset from the block start (in an aligned file a multiple of the alignment in the file)
//!  28 u32  head CRC: CRC-32C over bytes 0..28, then 32..payload offset
//!  32 u64  track id, the identity hash of the track (TrackDef::id): the chunk places itself without the describe set
//!  40 u32  track number (TrackDef::number)
//!  44  u8  segment shift: the payload is checked in segments of 2^shift bytes (16: 64 KiB)
//!  45  u8  columns n, 1 and up
//!  46  u8  time mode, the time codec's (events; 0 in series chunks)
//!  47  u8  end mode (events: EndMode; 0 in series chunks)
//!  48 u64  first item number of the track (sample index in a series)
//!  56 i64  first tick: the first item's start (the first sample's tick)
//!  64 i64  last tick: the last item's start (the last sample's tick); at least the first
//!  72 i64  latest end: the largest end of any item, at least the last tick; the last tick when every item is an
//!          instant (EndMode::Absent) and in a series chunk. An overlap query and a rebuilt index need no decode of the
//!          ends column.
//!  80 ...  n × column: kind u8, codec u8, param u16, offset u32 (from the payload start; the first 0, ascending)
//!     ...  segment CRCs: one u32 per segment of the payload, CRC-32C of its bytes
//!     ...  zeros up to the payload offset
//!  payload: the columns back to back, then 16 zero bytes, so an unaligned 16-byte load at any column byte stays in
//!           the block
//! len-4 u32 CRC over bytes 0..len-4, as in every block
//! ```
//!
//! A column runs from its offset to the next one's (the last to the padding). The head CRC lets a reader trust the
//! head, and so the block length, the chunk's place and the segment CRCs, when the payload is damaged: a reader in
//! [Recovery::Resync](crate::Recovery) skips such a block by its length and hands it out with the bad segments
//! marked ([Chunk::bad_segments]), so a flipped bit in a 4 MiB series chunk costs one 64 KiB segment. Event chunks are
//! usually a single segment; a codec may still salvage items from it by checking its own group headers against
//! [Chunk::column_damaged].
//!
//! Codec ids name how a column is coded; this crate reads and writes the block, not the codecs. [Codec::RAW] is the
//! column as plain little-endian values or bytes.

use crate::{BlockKind, CRC_LEN, HEADER_LEN, MAGIC, MAX_BLOCK_LEN, VERSION};

/// The CRC of chunk blocks: the same CRC-32C as every block, with a 16-entry table for the large payloads.
const CRC16: crc::Crc<u32, crc::Table<16>> =
    crc::Crc::<u32, crc::Table<16>>::new(&crc::CRC_32_ISCSI);

/// Bytes of the fixed part of the chunk head (32..80).
pub const CHUNK_FIXED_LEN: usize = 48;
/// Bytes of one column entry.
pub const COLUMN_ENTRY_LEN: usize = 8;
/// Zero bytes after the columns.
pub const PAYLOAD_PADDING: usize = 16;
/// Segment shift writers use: 64 KiB segments.
pub const SEGMENT_SHIFT: u8 = 16;
/// Smallest segment shift a reader takes (4 KiB segments; the largest, 16 MiB, is [SEGMENT_SHIFT_MAX]).
pub const SEGMENT_SHIFT_MIN: u8 = 12;
/// Largest segment shift a reader takes.
pub const SEGMENT_SHIFT_MAX: u8 = 24;
/// Most segments a chunk can have: [MAX_BLOCK_LEN] in segments of 2^[SEGMENT_SHIFT_MIN].
pub const MAX_SEGMENTS: usize = MAX_BLOCK_LEN >> SEGMENT_SHIFT_MIN;
/// Most columns in a chunk.
pub const MAX_COLUMNS: usize = u8::MAX as usize;

/// How a column is coded. Ids other than [Codec::RAW] are assigned by the codecs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Codec(pub u8);

impl Codec {
    /// Plain little-endian values (starts and ends as i64, flags as u32, lengths as u32, samples in the track's
    /// element type) or plain bytes (payload).
    pub const RAW: Codec = Codec(0);
}

/// What a column holds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    /// Item start ticks (events).
    Starts,
    /// Item ends, as the end mode says (events).
    Ends,
    /// A u32 flags word per item (events).
    Flags,
    /// Length of each item's payload (events).
    PayloadLengths,
    /// Item payloads back to back (events).
    Payloads,
    /// Samples of one channel, or of the channels a codec interleaves; the param names the (first) channel (series).
    Samples,
    /// A kind this reader doesn't know.
    Other(u8),
}

impl ColumnKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ColumnKind::Starts,
            2 => ColumnKind::Ends,
            3 => ColumnKind::Flags,
            4 => ColumnKind::PayloadLengths,
            5 => ColumnKind::Payloads,
            6 => ColumnKind::Samples,
            other => ColumnKind::Other(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            ColumnKind::Starts => 1,
            ColumnKind::Ends => 2,
            ColumnKind::Flags => 3,
            ColumnKind::PayloadLengths => 4,
            ColumnKind::Payloads => 5,
            ColumnKind::Samples => 6,
            ColumnKind::Other(v) => v,
        }
    }

    /// Whether the kind belongs in a chunk of `block` kind; unknown kinds belong anywhere.
    fn fits(self, block: BlockKind) -> bool {
        match self {
            ColumnKind::Samples => block == BlockKind::Series,
            ColumnKind::Other(_) => true,
            _ => block == BlockKind::Events,
        }
    }
}

/// How an events chunk stores ends.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EndMode {
    /// Every item is an instant: no ends column.
    Absent,
    /// The end of item i is the start of item i + 1; the ends column holds only the last item's end.
    NextStart,
    /// The ends column holds each item's duration.
    Durations,
}

impl EndMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(EndMode::Absent),
            1 => Some(EndMode::NextStart),
            2 => Some(EndMode::Durations),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            EndMode::Absent => 0,
            EndMode::NextStart => 1,
            EndMode::Durations => 2,
        }
    }
}

/// What a chunk is: which track, which items, which ticks. The fixed part of the head.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChunkHead {
    /// [BlockKind::Events] or [BlockKind::Series].
    pub kind: BlockKind,
    /// [TrackDef::id](crate::TrackDef::id).
    pub track_id: u64,
    /// [TrackDef::number](crate::TrackDef::number).
    pub track_number: u32,
    /// Item number of the first item in the track (sample index in a series).
    pub first_item: u64,
    /// Items (samples) in the chunk, 1 and up.
    pub count: u32,
    pub first_tick: i64,
    pub last_tick: i64,
    /// The largest end of any item, at least [Self::last_tick]; equal to it when every item is an instant
    /// ([EndMode::Absent]) and in a series chunk.
    pub latest_end: i64,
    /// The time codec's mode (events), 0 in a series chunk.
    pub time_mode: u8,
    /// [EndMode] (events); [EndMode::Absent] in a series chunk.
    pub end_mode: EndMode,
}

/// A column to write: what it is and how long its bytes are.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    pub kind: ColumnKind,
    pub codec: Codec,
    /// The channel of a [ColumnKind::Samples] column, else 0.
    pub param: u16,
    pub len: usize,
}

/// A column of a chunk read from a block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Column<'b> {
    pub kind: ColumnKind,
    pub codec: Codec,
    pub param: u16,
    /// Offset of the column in the payload.
    pub offset: usize,
    /// The column's bytes; the 16 bytes after them are still in the block.
    pub bytes: &'b [u8],
}

/// Why a chunk can't be written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChunkError {
    /// The buffer is shorter than the block ([chunk_len]).
    Full { needed: usize },
    /// The block would be longer than [MAX_BLOCK_LEN].
    TooLong { len: usize },
    /// The head or the columns break a rule of the format (no columns, count 0, last tick before the first, a column
    /// of another chunk kind, the kind not a chunk kind).
    Invalid(&'static str),
}

/// Bytes of the head (block header included) of a chunk with `columns` columns and `payload_len` bytes of columns,
/// before any alignment padding.
pub const fn chunk_head_len(columns: usize, payload_len: usize) -> usize {
    HEADER_LEN
        + CHUNK_FIXED_LEN
        + columns * COLUMN_ENTRY_LEN
        + 4 * segments(payload_len + PAYLOAD_PADDING, SEGMENT_SHIFT)
}

/// Zero bytes to put between the head and the payload so the payload of a block written at `offset` in a file
/// lands on a multiple of `alignment` (0: no alignment, no padding).
///
/// # Panics
///
/// When `alignment` is neither 0 nor a power of two.
pub const fn chunk_padding(offset: u64, head_len: usize, alignment: u32) -> usize {
    assert!(
        alignment == 0 || alignment.is_power_of_two(),
        "alignment is 0 or a power of two"
    );
    if alignment == 0 {
        return 0;
    }
    let a = alignment as u64;
    let end = offset + head_len as u64;
    ((a - end % a) % a) as usize
}

/// Bytes of a whole chunk block: head, `padding`, payload and CRC.
pub fn chunk_len(columns: &[ColumnSpec], padding: usize) -> usize {
    let payload: usize = columns.iter().map(|c| c.len).sum();
    chunk_head_len(columns.len(), payload) + padding + payload + PAYLOAD_PADDING + CRC_LEN
}

const fn segments(payload_len: usize, shift: u8) -> usize {
    payload_len.div_ceil(1 << shift)
}

pub(crate) fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn get_u16(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(buf[at..at + 2].try_into().expect("2 bytes"))
}

pub(crate) fn get_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(buf[at..at + 4].try_into().expect("4 bytes"))
}

pub(crate) fn get_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().expect("8 bytes"))
}

/// Writes one chunk block into a buffer: the head at [Self::new], the columns in place ([Self::column_mut]), the
/// CRCs at [Self::finish].
pub struct ChunkWriter<'b> {
    buf: &'b mut [u8],
    payload_at: usize,
    /// Column offsets in the payload, and the payload's length without padding at the end
    offsets: [u32; MAX_COLUMNS + 1],
    columns: usize,
}

impl<'b> ChunkWriter<'b> {
    /// Write the head of a chunk with `columns` (their bytes zero until filled in) and `padding` zero bytes before
    /// the payload ([chunk_padding]).
    pub fn new(
        buf: &'b mut [u8],
        seq: u64,
        head: &ChunkHead,
        columns: &[ColumnSpec],
        padding: usize,
    ) -> Result<Self, ChunkError> {
        check_head(head, columns.iter().map(|c| c.kind))?;
        if columns.len() > MAX_COLUMNS {
            return Err(ChunkError::Invalid("more than 255 columns"));
        }
        let len = chunk_len(columns, padding);
        if len > MAX_BLOCK_LEN {
            return Err(ChunkError::TooLong { len });
        }
        if buf.len() < len {
            return Err(ChunkError::Full { needed: len });
        }
        let buf = &mut buf[..len];
        let payload: usize = columns.iter().map(|c| c.len).sum();
        let head_len = chunk_head_len(columns.len(), payload);
        let payload_at = head_len + padding;
        buf.fill(0);
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4] = VERSION;
        buf[5] = head.kind.to_u8();
        put_u32(buf, 8, len as u32);
        put_u32(buf, 12, head.count);
        buf[16..24].copy_from_slice(&seq.to_le_bytes());
        put_u32(buf, 24, payload_at as u32);
        buf[32..40].copy_from_slice(&head.track_id.to_le_bytes());
        put_u32(buf, 40, head.track_number);
        buf[44] = SEGMENT_SHIFT;
        buf[45] = columns.len() as u8;
        buf[46] = head.time_mode;
        buf[47] = head.end_mode.to_u8();
        buf[48..56].copy_from_slice(&head.first_item.to_le_bytes());
        buf[56..64].copy_from_slice(&head.first_tick.to_le_bytes());
        buf[64..72].copy_from_slice(&head.last_tick.to_le_bytes());
        buf[72..80].copy_from_slice(&head.latest_end.to_le_bytes());
        let mut offsets = [0u32; MAX_COLUMNS + 1];
        let mut at = HEADER_LEN + CHUNK_FIXED_LEN;
        let mut off = 0usize;
        for (i, c) in columns.iter().enumerate() {
            buf[at] = c.kind.to_u8();
            buf[at + 1] = c.codec.0;
            buf[at + 2..at + 4].copy_from_slice(&c.param.to_le_bytes());
            put_u32(buf, at + 4, off as u32);
            offsets[i] = off as u32;
            off += c.len;
            at += COLUMN_ENTRY_LEN;
        }
        offsets[columns.len()] = off as u32;
        Ok(ChunkWriter {
            buf,
            payload_at,
            offsets,
            columns: columns.len(),
        })
    }

    /// The bytes of column `i`, to fill in.
    ///
    /// # Panics
    ///
    /// When there is no column `i`.
    pub fn column_mut(&mut self, i: usize) -> &mut [u8] {
        assert!(i < self.columns, "column {i} of {}", self.columns);
        let from = self.payload_at + self.offsets[i] as usize;
        let to = self.payload_at + self.offsets[i + 1] as usize;
        &mut self.buf[from..to]
    }

    /// Write the segment CRCs, the head CRC and the block CRC; returns the block's length.
    pub fn finish(self) -> usize {
        let len = self.buf.len();
        let payload_end = len - CRC_LEN;
        let seg = 1usize << SEGMENT_SHIFT;
        let table = HEADER_LEN + CHUNK_FIXED_LEN + self.columns * COLUMN_ENTRY_LEN;
        let n = segments(payload_end - self.payload_at, SEGMENT_SHIFT);
        assert!(
            table + 4 * n <= self.payload_at,
            "segment CRCs fit the head"
        );
        let mut payload_crc: Option<u32> = None;
        for i in 0..n {
            let from = self.payload_at + i * seg;
            let to = payload_end.min(from + seg);
            let c = CRC16.checksum(&self.buf[from..to]);
            payload_crc = Some(match payload_crc {
                None => c,
                Some(p) => crc32c_combine(p, c, (to - from) as u64),
            });
            put_u32(self.buf, table + 4 * i, c);
        }
        let head_crc = head_crc(self.buf, self.payload_at);
        put_u32(self.buf, 28, head_crc);
        let head_all = CRC16.checksum(&self.buf[..self.payload_at]);
        let block_crc = match payload_crc {
            Some(p) => crc32c_combine(head_all, p, (len - CRC_LEN - self.payload_at) as u64),
            None => head_all,
        };
        put_u32(self.buf, len - CRC_LEN, block_crc);
        len
    }
}

/// Rewrite the head CRC and the block CRC of a good chunk block whose header changed (its seq), from the segment
/// CRCs it holds: no pass over the payload.
pub(crate) fn restamp(block: &mut [u8]) {
    let len = block.len();
    let payload_at = get_u32(block, 24) as usize;
    let columns = block[45] as usize;
    let shift = block[44];
    let table = HEADER_LEN + CHUNK_FIXED_LEN + columns * COLUMN_ENTRY_LEN;
    let payload_len = len - CRC_LEN - payload_at;
    let seg = 1usize << shift;
    let mut payload_crc: Option<u32> = None;
    for i in 0..segments(payload_len, shift) {
        let c = get_u32(block, table + 4 * i);
        let n = (payload_len - i * seg).min(seg);
        payload_crc = Some(match payload_crc {
            None => c,
            Some(p) => crc32c_combine(p, c, n as u64),
        });
    }
    let h = head_crc(block, payload_at);
    put_u32(block, 28, h);
    let head_all = CRC16.checksum(&block[..payload_at]);
    let crc = match payload_crc {
        Some(p) => crc32c_combine(head_all, p, payload_len as u64),
        None => head_all,
    };
    put_u32(block, len - CRC_LEN, crc);
}

/// Write a whole chunk block with `columns` copied in; returns its length.
pub fn write_chunk(
    buf: &mut [u8],
    seq: u64,
    head: &ChunkHead,
    columns: &[(ColumnSpec, &[u8])],
    padding: usize,
) -> Result<usize, ChunkError> {
    let mut specs = [ColumnSpec {
        kind: ColumnKind::Other(0),
        codec: Codec::RAW,
        param: 0,
        len: 0,
    }; MAX_COLUMNS];
    if columns.len() > MAX_COLUMNS {
        return Err(ChunkError::Invalid("more than 255 columns"));
    }
    for (i, (spec, bytes)) in columns.iter().enumerate() {
        if spec.len != bytes.len() {
            return Err(ChunkError::Invalid("column length differs from its bytes"));
        }
        specs[i] = *spec;
    }
    let mut w = ChunkWriter::new(buf, seq, head, &specs[..columns.len()], padding)?;
    for (i, (_, bytes)) in columns.iter().enumerate() {
        w.column_mut(i).copy_from_slice(bytes);
    }
    Ok(w.finish())
}

fn check_head(head: &ChunkHead, kinds: impl Iterator<Item = ColumnKind>) -> Result<(), ChunkError> {
    if !matches!(head.kind, BlockKind::Events | BlockKind::Series) {
        return Err(ChunkError::Invalid("not a chunk block kind"));
    }
    if head.count == 0 {
        return Err(ChunkError::Invalid("no items"));
    }
    if head.first_item.checked_add(u64::from(head.count)).is_none() {
        return Err(ChunkError::Invalid("item numbers past u64"));
    }
    if head.last_tick < head.first_tick {
        return Err(ChunkError::Invalid("last tick before the first"));
    }
    if head.latest_end < head.last_tick {
        return Err(ChunkError::Invalid("latest end before the last tick"));
    }
    if (head.kind == BlockKind::Series || head.end_mode == EndMode::Absent)
        && head.latest_end != head.last_tick
    {
        return Err(ChunkError::Invalid(
            "latest end not the last tick in a chunk of instants or samples",
        ));
    }
    if head.kind == BlockKind::Series && (head.time_mode != 0 || head.end_mode != EndMode::Absent) {
        return Err(ChunkError::Invalid(
            "a series chunk has no time or end mode",
        ));
    }
    let mut n = 0;
    for k in kinds {
        if !k.fits(head.kind) {
            return Err(ChunkError::Invalid("column of another chunk kind"));
        }
        n += 1;
    }
    if n == 0 {
        return Err(ChunkError::Invalid("no columns"));
    }
    Ok(())
}

/// CRC-32C over bytes 0..28 and 32..`payload_at` of a chunk block.
fn head_crc(block: &[u8], payload_at: usize) -> u32 {
    let mut d = CRC16.digest();
    d.update(&block[..28]);
    d.update(&block[32..payload_at]);
    d.finalize()
}

/// The polynomial of CRC-32C, reflected.
const POLY: u32 = 0x82f6_3b78;

/// `a · b` modulo the polynomial, both reflected (zlib's `multmodp`).
const fn multmodp(a: u32, mut b: u32) -> u32 {
    let mut m = 1u32 << 31;
    let mut p = 0u32;
    loop {
        if a & m != 0 {
            p ^= b;
            if a & (m - 1) == 0 {
                break;
            }
        }
        m >>= 1;
        b = if b & 1 != 0 { (b >> 1) ^ POLY } else { b >> 1 };
    }
    p
}

/// `x^(2^k)` modulo the polynomial, k = 0..32.
const X2N: [u32; 32] = {
    let mut t = [0u32; 32];
    let mut p = 1u32 << 30; // x^1
    t[0] = p;
    let mut k = 1;
    while k < 32 {
        p = multmodp(p, p);
        t[k] = p;
        k += 1;
    }
    t
};

/// `x^(8·n)` modulo the polynomial.
const fn x8nmodp(mut n: u64) -> u32 {
    let mut p = 1u32 << 31; // x^0
    let mut k = 3;
    while n != 0 {
        if n & 1 != 0 {
            p = multmodp(X2N[k & 31], p);
        }
        n >>= 1;
        k += 1;
    }
    p
}

/// The CRC-32C of `A` then `B` from the CRC of `A`, the CRC of `B` and the length of `B`.
pub const fn crc32c_combine(crc_a: u32, crc_b: u32, len_b: u64) -> u32 {
    multmodp(x8nmodp(len_b), crc_a) ^ crc_b
}

/// Why a chunk block's head can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChunkHeadError {
    /// The head CRC doesn't match: nothing in the block can be trusted, its length neither.
    BadHeadCrc,
    /// The head is out of range: payload offset, segment shift, columns, count, ticks or column offsets.
    BadHead,
}

/// The chunk of a chunk block, read in place.
#[derive(Copy, Clone, Debug)]
pub struct Chunk<'b> {
    block: &'b [u8],
    kind: BlockKind,
    payload_at: usize,
    columns: usize,
    shift: u8,
}

impl<'b> Chunk<'b> {
    /// Read and check the head of the chunk block `block` (exactly its bytes, length checked by the caller): head
    /// CRC, ranges. The payload is not checked here.
    pub(crate) fn read_head(block: &'b [u8]) -> Result<Self, ChunkHeadError> {
        let len = block.len();
        let kind = BlockKind::from_u8(block[5]);
        let min = HEADER_LEN + CHUNK_FIXED_LEN;
        let payload_at = get_u32(block, 24) as usize;
        if payload_at < min || payload_at + PAYLOAD_PADDING + CRC_LEN > len {
            return Err(ChunkHeadError::BadHeadCrc);
        }
        if head_crc(block, payload_at) != get_u32(block, 28) {
            return Err(ChunkHeadError::BadHeadCrc);
        }
        let shift = block[44];
        let columns = block[45] as usize;
        if !(SEGMENT_SHIFT_MIN..=SEGMENT_SHIFT_MAX).contains(&shift) || columns == 0 {
            return Err(ChunkHeadError::BadHead);
        }
        let payload_len = len - CRC_LEN - payload_at;
        let table = min + columns * COLUMN_ENTRY_LEN;
        if table + 4 * segments(payload_len, shift) > payload_at {
            return Err(ChunkHeadError::BadHead);
        }
        let chunk = Chunk {
            block,
            kind,
            payload_at,
            columns,
            shift,
        };
        let head = chunk.head();
        let column_kinds =
            (0..columns).map(|i| ColumnKind::from_u8(block[min + i * COLUMN_ENTRY_LEN]));
        if EndMode::from_u8(block[47]).is_none() || check_head(&head, column_kinds).is_err() {
            return Err(ChunkHeadError::BadHead);
        }
        let mut prev = 0usize;
        for i in 0..columns {
            let off = get_u32(block, min + i * COLUMN_ENTRY_LEN + 4) as usize;
            if (i == 0 && off != 0) || off < prev {
                return Err(ChunkHeadError::BadHead);
            }
            prev = off;
        }
        if prev > payload_len - PAYLOAD_PADDING {
            return Err(ChunkHeadError::BadHead);
        }
        // padding between the head and the payload is zeros
        if block[table + 4 * segments(payload_len, shift)..payload_at]
            .iter()
            .any(|b| *b != 0)
        {
            return Err(ChunkHeadError::BadHead);
        }
        Ok(chunk)
    }

    /// The fixed part of the head.
    pub fn head(&self) -> ChunkHead {
        let b = self.block;
        ChunkHead {
            kind: self.kind,
            track_id: get_u64(b, 32),
            track_number: get_u32(b, 40),
            first_item: get_u64(b, 48),
            count: get_u32(b, 12),
            first_tick: get_u64(b, 56) as i64,
            last_tick: get_u64(b, 64) as i64,
            latest_end: get_u64(b, 72) as i64,
            time_mode: b[46],
            end_mode: EndMode::from_u8(b[47]).unwrap_or(EndMode::Absent),
        }
    }

    /// Where the payload starts in the block.
    pub fn payload_offset(&self) -> usize {
        self.payload_at
    }

    /// The payload: the columns, then [PAYLOAD_PADDING] bytes (zeros in an intact block).
    pub fn payload(&self) -> &'b [u8] {
        &self.block[self.payload_at..self.block.len() - CRC_LEN]
    }

    pub fn column_count(&self) -> usize {
        self.columns
    }

    /// Column `i`, None past the last.
    pub fn column(&self, i: usize) -> Option<Column<'b>> {
        if i >= self.columns {
            return None;
        }
        let at = HEADER_LEN + CHUNK_FIXED_LEN + i * COLUMN_ENTRY_LEN;
        let b = self.block;
        let offset = get_u32(b, at + 4) as usize;
        let payload = self.payload();
        let end = if i + 1 < self.columns {
            get_u32(b, at + COLUMN_ENTRY_LEN + 4) as usize
        } else {
            payload.len() - PAYLOAD_PADDING
        };
        Some(Column {
            kind: ColumnKind::from_u8(b[at]),
            codec: Codec(b[at + 1]),
            param: get_u16(b, at + 2),
            offset,
            bytes: &payload[offset..end],
        })
    }

    pub fn columns(&self) -> impl Iterator<Item = Column<'b>> + '_ {
        (0..self.columns).filter_map(|i| self.column(i))
    }

    /// The first column of `kind` (and `param`, for samples).
    pub fn find(&self, kind: ColumnKind, param: u16) -> Option<Column<'b>> {
        self.columns().find(|c| c.kind == kind && c.param == param)
    }

    /// Bytes of a segment, 2^shift.
    pub fn segment_len(&self) -> usize {
        1 << self.shift
    }

    pub fn segment_count(&self) -> usize {
        segments(self.payload().len(), self.shift)
    }

    fn segment_crc(&self, i: usize) -> u32 {
        get_u32(
            self.block,
            HEADER_LEN + CHUNK_FIXED_LEN + self.columns * COLUMN_ENTRY_LEN + 4 * i,
        )
    }

    /// Segment `i` of the payload.
    fn segment(&self, i: usize) -> &'b [u8] {
        let p = self.payload();
        let from = i << self.shift;
        &p[from..p.len().min(from + self.segment_len())]
    }

    /// Check every segment against its CRC and the block CRC against them: true when all match. One pass over the
    /// payload.
    pub(crate) fn check_payload(&self) -> bool {
        let mut combined = CRC16.checksum(&self.block[..self.payload_at]);
        for i in 0..self.segment_count() {
            let s = self.segment(i);
            let c = CRC16.checksum(s);
            if c != self.segment_crc(i) {
                return false;
            }
            combined = crc32c_combine(combined, c, s.len() as u64);
        }
        let len = self.block.len();
        combined == get_u32(self.block, len - CRC_LEN)
            && self.payload()[self.payload().len() - PAYLOAD_PADDING..]
                .iter()
                .all(|b| *b == 0)
    }

    /// The segments whose bytes don't match their CRC, ascending. Empty for an intact block.
    pub fn bad_segments(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.segment_count())
            .filter(|&i| CRC16.checksum(self.segment(i)) != self.segment_crc(i))
    }

    /// Whether any byte in `range` of the payload lies in a bad segment.
    pub fn range_damaged(&self, range: core::ops::Range<usize>) -> bool {
        if range.is_empty() {
            return false;
        }
        let first = range.start >> self.shift;
        let last = (range.end - 1) >> self.shift;
        (first..=last.min(self.segment_count().saturating_sub(1)))
            .any(|i| CRC16.checksum(self.segment(i)) != self.segment_crc(i))
    }

    /// Whether any byte of `column`, or of the [PAYLOAD_PADDING] bytes a codec may load past its end, is in a bad
    /// segment.
    pub fn column_damaged(&self, column: &Column<'_>) -> bool {
        let end = (column.offset + column.bytes.len() + PAYLOAD_PADDING).min(self.payload().len());
        self.range_damaged(column.offset..end)
    }
}

#[cfg(test)]
mod tests;
