//! Chunk blocks: one chunk of one track, its columns read in place.
//!
//! An events chunk ([BlockKind::Events]) holds up to a few thousand items of an event track, a series chunk
//! ([BlockKind::Series]) a run of samples of a fixed-rate series. The block header's count (bytes 12..16) is the
//! chunk's item or sample count; bytes 28..32 hold a CRC of the head, and the body starts with where the payload
//! starts:
//!
//! ```text
//! off size field
//!  24 u32  session tag, as in every block
//!  28 u32  head CRC: CRC-32C over bytes 0..28, then 32..payload offset
//!  32 u32  payload offset from the block start (in an aligned file a multiple of the alignment in the file)
//!  36 u32  track number (TrackDef::number)
//!  40 u64  track id, the identity hash of the track (TrackDef::id): the chunk places itself without the describe set
//!  48 u64  first item number of the track (sample index in a series)
//!  56 i64  first tick: the first item's start (the first sample's tick)
//!  64 i64  last tick: the last item's start (the last sample's tick); at least the first
//!  72 i64  latest end: the largest end of any item, at least the last tick; the last tick when every item is an
//!          instant (EndMode::Absent) and in a series chunk. An overlap query and a rebuilt index need no decode of the
//!          ends column.
//!  80  u8  segment shift: the payload is checked in segments of 2^shift bytes (16: 64 KiB)
//!  81  u8  columns n, 1 and up
//!  82  u8  time mode, the time codec's (events; 0 in series chunks)
//!  83  u8  end mode (events: EndMode; 0 in series chunks)
//!  84 ...  n × column: kind u8, codec u8, param u16, offset u32 (from the payload start; the first 0, ascending)
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
//! Codec ids name how a column is coded: [Codec::RAW] is the column as plain little-endian values or bytes, ids 1 to
//! 3 are the event time codec's ([crate::times]), 4 payload lengths ([crate::payloads]), 16 to 47 the sample codecs'
//! ([crate::samples]), 48 lz4 payloads ([crate::lz4]), 49 [Codec::SERIES] reserved.
//!
//! A reader refuses a column whose kind or codec it doesn't know, and still reads the chunk's other columns: each
//! reader ([EventTimes](crate::times::EventTimes), [Payloads](crate::payloads::Payloads),
//! [Samples](crate::samples::Samples)) takes the columns of its kinds and leaves the rest. Kind 7 is reserved
//! ([ColumnKind::PAYLOAD_LEAF]).
//!
//! **Reserved, not built:** [Codec::SERIES] (49) is a [ColumnKind::Payloads] column holding one shrink_wrap
//! `Series<T>` of the chunk's items, so items of a fixed-size struct are stored as columns of their fields. Such a
//! column stands alone: a [ColumnKind::PayloadLengths] column beside it is an error, the chunk holds at most 65535
//! items and the series' count is the head's count. Until it is built, [Payloads](crate::payloads::Payloads)
//! refuses it as a codec it doesn't know.
//!
//! Bit 7 of the codec byte ([Codec::OUTER]) marks an outer-coded column: its bytes are an outer codec u8, the decoded
//! length (uvar) and the outer-coded bytes, and the inner codec (bits 0..6, [Codec::inner]) applies to the decoded
//! bytes. An outer-coded column is not read in place nor salvaged by segment, so it suits payload columns and
//! archived pieces, never a time column. No outer codec is assigned yet: a reader refuses every outer-coded column as
//! a column in a codec it doesn't know. Inner codec ids stop at 127.

use core::num::NonZeroUsize;

use crate::bytes::{file_u8, file_u32, le_i64, le_u8, le_u32, le_u64};
use crate::{BlockKind, BoundError, CRC_LEN, HEADER_LEN, MAX_BLOCK_LEN};

/// The CRC of chunk blocks: the same CRC-32C as every block, with a 16-entry table for the large payloads.
const CRC16: crc::Crc<u32, crc::Table<16>> =
    crc::Crc::<u32, crc::Table<16>>::new(&crc::CRC_32_ISCSI);

/// Bytes of the fixed part of the chunk head (32..84).
pub const CHUNK_FIXED_LEN: usize = 52;
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
/// Most columns in a chunk: the column count is a byte.
pub const MAX_COLUMNS: usize = 255;

/// Bytes of the head before the column entries: the block header and the fixed part.
const FIXED_END: usize = HEADER_LEN + CHUNK_FIXED_LEN;
/// Bytes of a segment writers use, 2^[SEGMENT_SHIFT].
const SEGMENT_BYTES: usize = 1 << SEGMENT_SHIFT;
/// Bytes of a segment CRC.
const SEGMENT_CRC_LEN: usize = 4;

const _: () = assert!(
    usize::BITS >= u32::BITS && usize::BITS <= u64::BITS,
    "usize is 32 to 64 bits: a u32 fits it, it fits a u64"
);

/// `n` as a u64.
pub(crate) const fn len_u64(n: usize) -> u64 {
    #[expect(
        clippy::as_conversions,
        reason = "usize is at most 64 bits (the const assert above); From is not const"
    )]
    let v = n as u64;
    v
}

/// `n` as a usize.
pub(crate) const fn u32_len(n: u32) -> usize {
    #[expect(
        clippy::as_conversions,
        reason = "usize is at least 32 bits (the const assert above); From is not const"
    )]
    let v = n as usize;
    v
}

/// How a column is coded. Ids other than [Codec::RAW] are assigned by the codecs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Codec(pub u8);

impl Codec {
    /// Plain little-endian values (starts and ends as i64, flags as u32, lengths as u32, samples in the track's
    /// element type) or plain bytes (payload).
    pub const RAW: Codec = Codec(0);
    /// Reserved: a payloads column holding one shrink_wrap `Series<T>` of the chunk's items (module docs).
    pub const SERIES: Codec = Codec(49);
    /// Most items of a chunk whose payloads column is in [Codec::SERIES].
    pub const SERIES_ITEMS_MAX: u32 = 65_535;
    /// Bit 7 of the codec byte: the column is outer-coded (module docs).
    pub const OUTER: u8 = 0x80;

    /// Whether the column is outer-coded: bit 7 set.
    pub const fn is_outer(self) -> bool {
        self.0 & Self::OUTER != 0
    }

    /// The codec of the column's bytes, or of the decoded bytes of an outer-coded column: bits 0..6.
    pub const fn inner(self) -> Codec {
        Codec(self.0 & !Self::OUTER)
    }
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
    /// Reserved, kind 7: one leaf of the items' payload struct, its param the leaf's index. Read as
    /// [ColumnKind::Other] until it is built, so a reader refuses it.
    pub const PAYLOAD_LEAF: ColumnKind = ColumnKind::Other(7);

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
    /// The end of item i is the start of item i + 1, the last item's end is the head's latest end: no ends column.
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
#[non_exhaustive]
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
/// before any alignment padding; saturating at `usize::MAX`, a length over [MAX_BLOCK_LEN] no writer takes.
pub const fn chunk_head_len(columns: usize, payload_len: usize) -> usize {
    let entries = columns.saturating_mul(COLUMN_ENTRY_LEN);
    let crcs = payload_len
        .saturating_add(PAYLOAD_PADDING)
        .div_ceil(SEGMENT_BYTES)
        .saturating_mul(SEGMENT_CRC_LEN);
    FIXED_END.saturating_add(entries).saturating_add(crcs)
}

/// Zero bytes to put between the head and the payload so the payload of a block written at `offset` in a file
/// lands on a multiple of `alignment` (0: no alignment, no padding); None when `alignment` is neither 0 nor a power
/// of two.
pub const fn chunk_padding(offset: u64, head_len: usize, alignment: u32) -> Option<usize> {
    if alignment == 0 {
        return Some(0);
    }
    if !alignment.is_power_of_two() {
        return None;
    }
    let Some(mask) = alignment.checked_sub(1) else {
        return None;
    };
    // only the low bits count: the end modulo 2^64 has them
    let end = offset.wrapping_add(len_u64(head_len));
    // alignment is a power of two: the distance to the next multiple is the low bits of -end
    let [b0, b1, b2, b3, ..] = end.wrapping_neg().to_le_bytes();
    Some(u32_len(u32::from_le_bytes([b0, b1, b2, b3]) & mask))
}

/// Bytes of a whole chunk block: head, `padding`, payload and CRC; saturating at `usize::MAX`, a length over
/// [MAX_BLOCK_LEN] no writer takes.
pub fn chunk_len(columns: &[ColumnSpec], padding: usize) -> usize {
    Parts::new(columns, padding)
        .and_then(|p| p.len())
        .unwrap_or(usize::MAX)
}

/// Bytes of the parts of a chunk block to write.
struct Parts {
    entries: usize,
    seg_crcs: usize,
    padding: usize,
    payload: usize,
}

impl Parts {
    /// None when a length overflows a usize.
    fn new(columns: &[ColumnSpec], padding: usize) -> Option<Self> {
        let columns_len = columns
            .iter()
            .try_fold(0usize, |n, c| n.checked_add(c.len))?;
        let payload = columns_len.checked_add(PAYLOAD_PADDING)?;
        Some(Parts {
            entries: columns.len().checked_mul(COLUMN_ENTRY_LEN)?,
            seg_crcs: payload
                .div_ceil(SEGMENT_BYTES)
                .checked_mul(SEGMENT_CRC_LEN)?,
            padding,
            payload,
        })
    }

    fn len(&self) -> Option<usize> {
        FIXED_END
            .checked_add(self.entries)?
            .checked_add(self.seg_crcs)?
            .checked_add(self.padding)?
            .checked_add(self.payload)?
            .checked_add(CRC_LEN)
    }
}

/// Bytes of the header before the head CRC (0..28).
const PRE_CRC_LEN: usize = 28;

/// Writes one chunk block into a buffer: the head at [Self::new], the columns in place ([Self::column_mut]), the
/// CRCs at [Self::finish].
pub struct ChunkWriter<'b> {
    len: usize,
    /// Header bytes 0..28
    pre: &'b mut [u8; PRE_CRC_LEN],
    head_crc: &'b mut [u8; 4],
    fixed: &'b mut [u8; CHUNK_FIXED_LEN],
    entries: &'b mut [u8],
    seg_crcs: &'b mut [u8],
    /// Zeros up to the payload offset
    padding: &'b mut [u8],
    /// The columns, then [PAYLOAD_PADDING] zeros
    payload: &'b mut [u8],
    crc: &'b mut [u8; CRC_LEN],
    /// Column offsets in the payload, and the payload's length without padding at the end
    offsets: [u32; MAX_COLUMNS + 1],
    columns: usize,
}

impl<'b> ChunkWriter<'b> {
    /// Write the head of a chunk with `columns` (their bytes zero until filled in) and `padding` zero bytes before
    /// the payload ([chunk_padding]).
    pub fn new(
        buf: &'b mut [u8],
        id: crate::BlockId,
        head: &ChunkHead,
        columns: &[ColumnSpec],
        padding: usize,
    ) -> Result<Self, ChunkError> {
        check_head(head, columns.iter().map(|c| c.kind))?;
        let column_count = u8::try_from(columns.len())
            .ok()
            .ok_or(ChunkError::Invalid("more than 255 columns"))?;
        let len = chunk_len(columns, padding);
        if len > MAX_BLOCK_LEN {
            return Err(ChunkError::TooLong { len });
        }
        let parts = Parts::new(columns, padding).ok_or(ChunkError::TooLong { len })?;
        let short = ChunkError::Full { needed: len };
        let buf = buf.get_mut(..len).ok_or(short)?;
        buf.fill(0);
        let (pre, rest) = buf.split_first_chunk_mut::<PRE_CRC_LEN>().ok_or(short)?;
        let (head_crc, rest) = rest.split_first_chunk_mut::<4>().ok_or(short)?;
        let (fixed, rest) = rest
            .split_first_chunk_mut::<CHUNK_FIXED_LEN>()
            .ok_or(short)?;
        let (entries, rest) = rest.split_at_mut_checked(parts.entries).ok_or(short)?;
        let (seg_crcs, rest) = rest.split_at_mut_checked(parts.seg_crcs).ok_or(short)?;
        let (gap, rest) = rest.split_at_mut_checked(parts.padding).ok_or(short)?;
        let (payload, crc) = rest.split_last_chunk_mut::<CRC_LEN>().ok_or(short)?;
        // the payload starts after the head and the padding, within a block of at most MAX_BLOCK_LEN
        let payload_at = len
            .checked_sub(payload.len())
            .and_then(|n| n.checked_sub(CRC_LEN))
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(ChunkError::TooLong { len })?;

        let mut header = [0u8; HEADER_LEN];
        crate::put_header(&mut header, head.kind, len, head.count, id)
            .ok_or(ChunkError::TooLong { len })?;
        let [header_pre @ .., _, _, _, _] = header;
        *pre = header_pre;
        let fields = payload_at
            .to_le_bytes()
            .into_iter()
            .chain(head.track_number.to_le_bytes())
            .chain(head.track_id.to_le_bytes())
            .chain(head.first_item.to_le_bytes())
            .chain(head.first_tick.to_le_bytes())
            .chain(head.last_tick.to_le_bytes())
            .chain(head.latest_end.to_le_bytes())
            .chain([
                SEGMENT_SHIFT,
                column_count,
                head.time_mode,
                head.end_mode.to_u8(),
            ]);
        for (d, s) in fixed.iter_mut().zip(fields) {
            *d = s;
        }

        let mut offsets = [0u32; MAX_COLUMNS + 1];
        let mut off = 0u32;
        let (entry_slots, _) = entries.as_chunks_mut::<COLUMN_ENTRY_LEN>();
        for ((slot, entry), c) in offsets.iter_mut().zip(entry_slots).zip(columns) {
            *slot = off;
            let [p0, p1] = c.param.to_le_bytes();
            let [o0, o1, o2, o3] = off.to_le_bytes();
            *entry = [c.kind.to_u8(), c.codec.0, p0, p1, o0, o1, o2, o3];
            // the columns sum to less than the block's length, at most MAX_BLOCK_LEN
            off = u32::try_from(c.len)
                .ok()
                .and_then(|n| off.checked_add(n))
                .ok_or(ChunkError::TooLong { len })?;
        }
        *offsets
            .get_mut(columns.len())
            .ok_or(ChunkError::Invalid("more than 255 columns"))? = off;
        Ok(ChunkWriter {
            len,
            pre,
            head_crc,
            fixed,
            entries,
            seg_crcs,
            padding: gap,
            payload,
            crc,
            offsets,
            columns: columns.len(),
        })
    }

    /// The bytes of column `i`, to fill in; None when there is no column `i`.
    pub fn column_mut(&mut self, i: usize) -> Option<&mut [u8]> {
        if i >= self.columns {
            return None;
        }
        let from = u32_len(*self.offsets.get(i)?);
        let to = u32_len(*self.offsets.get(i.checked_add(1)?)?);
        self.payload.get_mut(from..to)
    }

    /// Write the segment CRCs, the head CRC and the block CRC; returns the block's length.
    pub fn finish(self) -> usize {
        let mut payload_crc: Option<u32> = None;
        let (slots, _) = self.seg_crcs.as_chunks_mut::<SEGMENT_CRC_LEN>();
        for (segment, slot) in self.payload.chunks(SEGMENT_BYTES).zip(slots) {
            let c = CRC16.checksum(segment);
            payload_crc = Some(match payload_crc {
                None => c,
                Some(p) => crc32c_combine(p, c, len_u64(segment.len())),
            });
            *slot = c.to_le_bytes();
        }
        let after_crc = [
            self.fixed.as_slice(),
            self.entries,
            self.seg_crcs,
            self.padding,
        ];
        let mut d = CRC16.digest();
        d.update(self.pre);
        after_crc.iter().for_each(|p| d.update(p));
        *self.head_crc = d.finalize().to_le_bytes();
        let mut d = CRC16.digest();
        d.update(self.pre);
        d.update(self.head_crc);
        after_crc.iter().for_each(|p| d.update(p));
        let head_all = d.finalize();
        let block_crc = match payload_crc {
            Some(p) => crc32c_combine(head_all, p, len_u64(self.payload.len())),
            None => head_all,
        };
        *self.crc = block_crc.to_le_bytes();
        self.len
    }
}

/// Rewrite the head CRC and the block CRC of a good chunk block whose header changed (its seq), from the segment
/// CRCs it holds: no pass over the payload. An error says why the block's head is out of range (its head CRC is not
/// checked: the header changed); nothing is written then.
pub(crate) fn restamp(block: &mut [u8]) -> Result<(), ChunkHeadError> {
    let payload_at = payload_offset(block)?;
    let l = Layout::of(block, payload_at)?;
    let payload = block
        .get(payload_at..l.payload_end)
        .ok_or(ChunkHeadError::BadHead)?;
    let seg_crcs = block
        .get(l.table_end..l.seg_crcs_end)
        .ok_or(ChunkHeadError::BadHead)?;
    let mut payload_crc: Option<u32> = None;
    for (segment, c) in payload
        .chunks(l.seg_len.get())
        .zip(seg_crcs.as_chunks::<SEGMENT_CRC_LEN>().0)
    {
        let c = u32::from_le_bytes(*c);
        payload_crc = Some(match payload_crc {
            None => c,
            Some(p) => crc32c_combine(p, c, len_u64(segment.len())),
        });
    }
    let payload_len = len_u64(payload.len());
    let h = head_crc(block, payload_at).ok_or(ChunkHeadError::BadHead)?;
    let (pre, rest) = block
        .split_first_chunk_mut::<PRE_CRC_LEN>()
        .ok_or(ChunkHeadError::BadHead)?;
    let (head_crc_slot, rest) = rest
        .split_first_chunk_mut::<4>()
        .ok_or(ChunkHeadError::BadHead)?;
    *head_crc_slot = h.to_le_bytes();
    let head_rest = rest
        .get(..payload_at.saturating_sub(PRE_CRC_LEN + 4))
        .ok_or(ChunkHeadError::BadHead)?;
    let mut d = CRC16.digest();
    d.update(pre);
    d.update(head_crc_slot);
    d.update(head_rest);
    let head_all = d.finalize();
    let crc = match payload_crc {
        Some(p) => crc32c_combine(head_all, p, payload_len),
        None => head_all,
    };
    let (_, crc_slot) = block
        .split_last_chunk_mut::<CRC_LEN>()
        .ok_or(ChunkHeadError::BadHead)?;
    *crc_slot = crc.to_le_bytes();
    Ok(())
}

/// Write a whole chunk block with `columns` copied in; returns its length.
pub fn write_chunk(
    buf: &mut [u8],
    id: crate::BlockId,
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
    let specs = specs
        .get_mut(..columns.len())
        .ok_or(ChunkError::Invalid("more than 255 columns"))?;
    for (slot, (spec, bytes)) in specs.iter_mut().zip(columns) {
        if spec.len != bytes.len() {
            return Err(ChunkError::Invalid("column length differs from its bytes"));
        }
        *slot = *spec;
    }
    let mut w = ChunkWriter::new(buf, id, head, specs, padding)?;
    for (i, (_, bytes)) in columns.iter().enumerate() {
        w.column_mut(i)
            .ok_or(ChunkError::Invalid("a column the writer was not made with"))?
            .copy_from_slice(bytes);
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
    let mut any = false;
    for k in kinds {
        if !k.fits(head.kind) {
            return Err(ChunkError::Invalid("column of another chunk kind"));
        }
        any = true;
    }
    if !any {
        return Err(ChunkError::Invalid("no columns"));
    }
    Ok(())
}

/// CRC-32C over bytes 0..28 and 32..`payload_at` of a chunk block; None when the block is shorter.
fn head_crc(block: &[u8], payload_at: usize) -> Option<u32> {
    let mut d = CRC16.digest();
    d.update(block.get(..PRE_CRC_LEN)?);
    d.update(block.get(PRE_CRC_LEN + 4..payload_at)?);
    Some(d.finalize())
}

/// The polynomial of CRC-32C, reflected.
const POLY: u32 = 0x82f6_3b78;

/// `a · b` modulo the polynomial, both reflected (zlib's `multmodp`, over all 32 bits of `a`).
const fn multmodp(a: u32, mut b: u32) -> u32 {
    let mut p = 0u32;
    let mut m = 1u32 << 31;
    while m != 0 {
        if a & m != 0 {
            p ^= b;
        }
        m = m.wrapping_shr(1);
        b = if b & 1 != 0 {
            b.wrapping_shr(1) ^ POLY
        } else {
            b.wrapping_shr(1)
        };
    }
    p
}

/// `x^(2^k)` modulo the polynomial, k = 0..32.
const X2N: [u32; 32] = {
    let mut t = [0u32; 32];
    let mut p = 1u32 << 30; // x^1
    let mut k = 0;
    while k < 32 {
        #[expect(clippy::indexing_slicing, reason = "k is under 32, the table's length")]
        let slot = &mut t[k];
        *slot = p;
        p = multmodp(p, p);
        k += 1;
    }
    t
};

/// `x^(8·n)` modulo the polynomial.
const fn x8nmodp(mut n: u64) -> u32 {
    let mut p = 1u32 << 31; // x^0
    // x^(2^k) repeats with period 32 in k: X2N is indexed modulo 32
    let mut k = 3usize;
    while n != 0 {
        if n & 1 != 0 {
            #[expect(
                clippy::indexing_slicing,
                reason = "k % 32 is under 32, the table's length"
            )]
            let x = X2N[k % 32];
            p = multmodp(x, p);
        }
        n = n.wrapping_shr(1);
        k = k.wrapping_add(1) % 32;
    }
    p
}

/// The CRC-32C of `A` then `B` from the CRC of `A`, the CRC of `B` and the length of `B`.
pub const fn crc32c_combine(crc_a: u32, crc_b: u32, len_b: u64) -> u32 {
    multmodp(x8nmodp(len_b), crc_a) ^ crc_b
}

/// Why a chunk block's head can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChunkHeadError {
    /// The head CRC doesn't match: nothing in the block can be trusted, its length neither.
    BadHeadCrc,
    /// The head is out of range: payload offset, segment shift, columns, count, ticks or column offsets.
    BadHead,
    /// A layout value of the head (segment shift, column count, column offset) is over its bound.
    Bound(BoundError),
}

/// Where the parts of a chunk block lie, from its head's ranges alone (no CRC).
#[derive(Copy, Clone, Debug)]
struct Layout {
    seg_len: NonZeroUsize,
    columns: usize,
    /// End of the column entries
    table_end: usize,
    /// End of the segment CRCs, at most the payload offset
    seg_crcs_end: usize,
    /// End of the payload: the block's length less its CRC
    payload_end: usize,
}

/// The payload offset of a chunk block (bytes 32..36): after the fixed head, with room for the padding and the CRC
/// after it. Read before the head CRC can be checked, so out of range is [ChunkHeadError::BadHeadCrc].
fn payload_offset(block: &[u8]) -> Result<usize, ChunkHeadError> {
    let Some(max) = block.len().checked_sub(PAYLOAD_PADDING + CRC_LEN) else {
        return Err(ChunkHeadError::BadHeadCrc);
    };
    let Some(at) = file_u32(block, 32) else {
        return Err(ChunkHeadError::BadHeadCrc);
    };
    match at.within(max) {
        Ok(at) if at >= FIXED_END => Ok(at),
        _ => Err(ChunkHeadError::BadHeadCrc),
    }
}

impl Layout {
    /// The layout of `block` whose payload starts at `payload_at` ([payload_offset]).
    fn of(block: &[u8], payload_at: usize) -> Result<Self, ChunkHeadError> {
        let payload_end = block
            .len()
            .checked_sub(CRC_LEN)
            .ok_or(ChunkHeadError::BadHead)?;
        let payload_len = payload_end
            .checked_sub(payload_at)
            .ok_or(ChunkHeadError::BadHead)?;
        let shift = file_u8(block, 80)
            .ok_or(ChunkHeadError::BadHead)?
            .bounded(SEGMENT_SHIFT_MAX)
            .map_err(ChunkHeadError::Bound)?;
        if shift < SEGMENT_SHIFT_MIN {
            return Err(ChunkHeadError::BadHead);
        }
        let columns = file_u8(block, 81)
            .ok_or(ChunkHeadError::BadHead)?
            .at_most(MAX_COLUMNS)
            .map_err(ChunkHeadError::Bound)?;
        if columns == 0 {
            return Err(ChunkHeadError::BadHead);
        }
        let seg_len = 1usize
            .checked_shl(u32::from(shift))
            .and_then(NonZeroUsize::new)
            .ok_or(ChunkHeadError::BadHead)?;
        // at most MAX_SEGMENTS: the payload is under MAX_BLOCK_LEN, a segment at least 2^SEGMENT_SHIFT_MIN
        let segments = payload_len.div_ceil(seg_len.get());
        let table_end = columns
            .checked_mul(COLUMN_ENTRY_LEN)
            .and_then(|n| n.checked_add(FIXED_END))
            .ok_or(ChunkHeadError::BadHead)?;
        let seg_crcs_end = segments
            .checked_mul(SEGMENT_CRC_LEN)
            .and_then(|n| n.checked_add(table_end))
            .filter(|end| *end <= payload_at)
            .ok_or(ChunkHeadError::BadHead)?;
        Ok(Layout {
            seg_len,
            columns,
            table_end,
            seg_crcs_end,
            payload_end,
        })
    }
}

/// The fixed part of the head of `block` of `kind`; None when the block is shorter or the end mode unknown.
fn read_fixed(block: &[u8], kind: BlockKind) -> Option<ChunkHead> {
    Some(ChunkHead {
        kind,
        track_id: le_u64(block, 40)?,
        track_number: le_u32(block, 36)?,
        first_item: le_u64(block, 48)?,
        count: le_u32(block, 12)?,
        first_tick: le_i64(block, 56)?,
        last_tick: le_i64(block, 64)?,
        latest_end: le_i64(block, 72)?,
        time_mode: le_u8(block, 82)?,
        end_mode: EndMode::from_u8(le_u8(block, 83)?)?,
    })
}

/// The chunk of a chunk block, read in place.
#[derive(Copy, Clone, Debug)]
pub struct Chunk<'b> {
    head: ChunkHead,
    /// Bytes 0..payload offset
    head_bytes: &'b [u8],
    /// The column entries, [COLUMN_ENTRY_LEN] bytes each, offsets checked ascending within the columns
    entries: &'b [[u8; COLUMN_ENTRY_LEN]],
    /// One CRC per segment of the payload
    seg_crcs: &'b [[u8; SEGMENT_CRC_LEN]],
    /// The columns, then [PAYLOAD_PADDING] bytes
    payload: &'b [u8],
    block_crc: u32,
    seg_len: NonZeroUsize,
}

impl<'b> Chunk<'b> {
    /// Read and check the head of the chunk block `block` (exactly its bytes, length checked by the caller): head
    /// CRC, ranges. The payload is not checked here.
    pub(crate) fn read_head(block: &'b [u8]) -> Result<Self, ChunkHeadError> {
        let kind = BlockKind::from_u8(le_u8(block, 5).ok_or(ChunkHeadError::BadHeadCrc)?);
        let payload_at = payload_offset(block)?;
        if head_crc(block, payload_at) != le_u32(block, 28) {
            return Err(ChunkHeadError::BadHeadCrc);
        }
        let l = Layout::of(block, payload_at)?;
        let head = read_fixed(block, kind).ok_or(ChunkHeadError::BadHead)?;
        let entries = block
            .get(FIXED_END..l.table_end)
            .ok_or(ChunkHeadError::BadHead)?
            .as_chunks::<COLUMN_ENTRY_LEN>()
            .0;
        if entries.len() != l.columns
            || check_head(&head, entries.iter().map(|[k, ..]| ColumnKind::from_u8(*k))).is_err()
        {
            return Err(ChunkHeadError::BadHead);
        }
        let payload = block
            .get(payload_at..l.payload_end)
            .ok_or(ChunkHeadError::BadHead)?;
        let columns_len = payload
            .len()
            .checked_sub(PAYLOAD_PADDING)
            .ok_or(ChunkHeadError::BadHead)?;
        let mut prev = 0usize;
        for (i, e) in entries.iter().enumerate() {
            let off = column_offset(e)
                .within(columns_len)
                .map_err(ChunkHeadError::Bound)?;
            if (i == 0 && off != 0) || off < prev {
                return Err(ChunkHeadError::BadHead);
            }
            prev = off;
        }
        let (seg_crcs, _) = block
            .get(l.table_end..l.seg_crcs_end)
            .ok_or(ChunkHeadError::BadHead)?
            .as_chunks::<SEGMENT_CRC_LEN>();
        // padding between the head and the payload is zeros
        let gap = block
            .get(l.seg_crcs_end..payload_at)
            .ok_or(ChunkHeadError::BadHead)?;
        if gap.iter().any(|b| *b != 0) {
            return Err(ChunkHeadError::BadHead);
        }
        Ok(Chunk {
            head,
            head_bytes: block.get(..payload_at).ok_or(ChunkHeadError::BadHead)?,
            entries,
            seg_crcs,
            payload,
            block_crc: le_u32(block, l.payload_end).ok_or(ChunkHeadError::BadHead)?,
            seg_len: l.seg_len,
        })
    }

    /// The fixed part of the head.
    pub fn head(&self) -> ChunkHead {
        self.head
    }

    /// Where the payload starts in the block.
    pub fn payload_offset(&self) -> usize {
        self.head_bytes.len()
    }

    /// The payload: the columns, then [PAYLOAD_PADDING] bytes (zeros in an intact block).
    pub fn payload(&self) -> &'b [u8] {
        self.payload
    }

    pub fn column_count(&self) -> usize {
        self.entries.len()
    }

    /// Column `i`, None past the last.
    pub fn column(&self, i: usize) -> Option<Column<'b>> {
        let entry = self.entries.get(i)?;
        let columns_len = self.payload.len().checked_sub(PAYLOAD_PADDING)?;
        // offsets were checked ascending and within the columns when the head was read
        let offset = column_offset(entry).within(columns_len).ok()?;
        let end = match self.entries.get(i.checked_add(1)?) {
            Some(next) => column_offset(next).within(columns_len).ok()?,
            None => columns_len,
        };
        let [kind, codec, p0, p1, ..] = *entry;
        Some(Column {
            kind: ColumnKind::from_u8(kind),
            codec: Codec(codec),
            param: u16::from_le_bytes([p0, p1]),
            offset,
            bytes: self.payload.get(offset..end)?,
        })
    }

    pub fn columns(&self) -> impl Iterator<Item = Column<'b>> + '_ {
        (0..self.entries.len()).filter_map(|i| self.column(i))
    }

    /// The first column of `kind` (and `param`, for samples).
    pub fn find(&self, kind: ColumnKind, param: u16) -> Option<Column<'b>> {
        self.columns().find(|c| c.kind == kind && c.param == param)
    }

    /// Bytes of a segment, 2^shift.
    pub fn segment_len(&self) -> usize {
        self.seg_len.get()
    }

    pub fn segment_count(&self) -> usize {
        self.seg_crcs.len()
    }

    /// The segments with their CRCs, in order.
    fn segments(&self) -> impl Iterator<Item = (&'b [u8], u32)> + '_ {
        self.payload
            .chunks(self.seg_len.get())
            .zip(self.seg_crcs.iter().map(|c| u32::from_le_bytes(*c)))
    }

    /// Segment `i` of the payload, None past the last.
    fn segment(&self, i: usize) -> Option<&'b [u8]> {
        self.payload.chunks(self.seg_len.get()).nth(i)
    }

    /// Whether segment `i` doesn't match its CRC; false past the last segment.
    fn segment_bad(&self, i: usize) -> bool {
        match (self.segment(i), self.seg_crcs.get(i)) {
            (Some(s), Some(c)) => CRC16.checksum(s) != u32::from_le_bytes(*c),
            _ => false,
        }
    }

    /// Check every segment against its CRC and the block CRC against them: true when all match. One pass over the
    /// payload.
    pub(crate) fn check_payload(&self) -> bool {
        let mut combined = CRC16.checksum(self.head_bytes);
        for (s, crc) in self.segments() {
            let c = CRC16.checksum(s);
            if c != crc {
                return false;
            }
            combined = crc32c_combine(combined, c, len_u64(s.len()));
        }
        combined == self.block_crc
            && self
                .payload
                .last_chunk::<PAYLOAD_PADDING>()
                .is_some_and(|p| p.iter().all(|b| *b == 0))
    }

    /// The segments whose bytes don't match their CRC, ascending. Empty for an intact block.
    pub fn bad_segments(&self) -> impl Iterator<Item = usize> + '_ {
        self.segments()
            .enumerate()
            .filter(|(_, (s, c))| CRC16.checksum(s) != *c)
            .map(|(i, _)| i)
    }

    /// Whether any byte in `range` of the payload lies in a bad segment.
    pub fn range_damaged(&self, range: core::ops::Range<usize>) -> bool {
        if range.is_empty() {
            return false;
        }
        let Some(last_byte) = range.end.checked_sub(1) else {
            return false;
        };
        let first = range.start / self.seg_len;
        let last = last_byte / self.seg_len;
        (first..=last)
            .take_while(|i| *i < self.segment_count())
            .any(|i| self.segment_bad(i))
    }

    /// Whether any byte of `column`, or of the [PAYLOAD_PADDING] bytes a codec may load past its end, is in a bad
    /// segment.
    pub fn column_damaged(&self, column: &Column<'_>) -> bool {
        // past the payload there is nothing to be damaged: the end stops there
        let end = column
            .offset
            .saturating_add(column.bytes.len())
            .saturating_add(PAYLOAD_PADDING)
            .min(self.payload.len());
        self.range_damaged(column.offset..end)
    }
}

/// The offset field of a column entry.
fn column_offset(entry: &[u8; COLUMN_ENTRY_LEN]) -> crate::FromFile<u32> {
    let [.., o0, o1, o2, o3] = *entry;
    crate::FromFile::new(u32::from_le_bytes([o0, o1, o2, o3]))
}

#[cfg(test)]
mod tests;
