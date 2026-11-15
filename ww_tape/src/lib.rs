//! Tape blocks: the unit a producer of records hands over, a live consumer reads and a log file is made of.
//!
//! A tape is the recording unit whatever it is recorded on: blocks handed over live, appended to a file, replayed.
//!
//! A **record** is an item (any ShrinkWrap type, or plain bytes) on a **track**, at a time: signed i64 ticks of the
//! track's **clock**. Clocks are described by a [ClockDef] (number, stable key, exact rate and resolution, origin),
//! tracks by a [TrackDef]: a small number used in records, a stable key, the clock, dimensions that are part of its
//! identity, the track's resource path in the producer's API (`ww_trait` ids, array indices included) and the item
//! type, so a reader knows from the API bundle what the items are. How clocks relate (the wall clock to a device's) is
//! said by [ClockMap]s. Nothing here knows any producer: a link tap, a device log or a simulator all write the same
//! blocks.
//!
//! A **block** is a header, a body and a CRC-32C (`CRC_32_ISCSI`):
//!
//! ```text
//! off size field
//!   0   4  magic "WWTP"
//!   4  u8  version, 2
//!   5  u8  kind: 1 records, 2 describe (BlockKind)
//!   6 u16  flags: bit 0 body coded (FLAG_BODY_CODED, records and describe blocks only), bit 1 copy (FLAG_COPY),
//!          bits 2 and 3 first and last block of a describe set (FLAG_SET_FIRST, FLAG_SET_LAST); others 0 (a
//!          reader ignores bits it doesn't know)
//!   8 u32  len: the whole block, header and CRC included
//!  12 u32  records in the block
//!  16 u64  seq: the block number in its run, from 0, +1 per block; a gap is lost blocks. Index, finish, tail and
//!          piece head blocks, and describe blocks a writer repeats, carry the seq of the block before them and are
//!          not counted (index::is_structural)
//!  24 u32  run tag (RunTag): the same in every block of one writer's unbroken append, never 0 nor FFFFFFFFh
//!  28 u32  head CRC in chunk and index blocks (their own docs), else 0
//!  32 ...  body
//! len-4 u32 crc over bytes 0..len-4
//! ```
//!
//! The body of a records or describe block is a clock table, then records back to back:
//!
//! ```text
//! uvar n, n × uvar clock number (ascending, 1 and up): the clocks of the block's records; slot i is the i-th
//! per record:
//!   uvar head: track << bits | slot, bits the fewest that hold n - 1 (0 for one clock)
//!   ivar time delta in ticks, against the previous record of the same slot in the block (the first against 0);
//!        absent for track 0
//!   uvar item length, the item's bytes
//! ```
//!
//! The item of a record on an events track whose [EventsDef] has spans or flags is an [EventRecord]: the duration
//! (uvar, spans only), the flags (uvar, only with flags), then the payload; the record's time is the start. A track
//! of plain instants carries the payload alone. A sticky record of an events track carries the same item.
//!
//! With [FLAG_BODY_CODED] that body is coded: a codec byte, its decoded length and the coded bytes ([body]).
//!
//! So time deltas are coded per clock, a block reads alone (its first time per clock is absolute) and one clock costs
//! no bit in the head. No wall clock is in a block: wall time is a clock of its own, mapped to the others.
//!
//! A record's clock is always its track's ([TrackDef::clock]). The slot repeats it on purpose, so a block's times read
//! without the describe set; a writer keeps the two in agreement, and a reader may take a record whose slot names
//! another clock than its track's for damage.
//!
//! The header and the CRC at the end are the same in every version, so any reader can check a block and skip it by its
//! length. A later version bumps the version byte and changes only the body; new block kinds and flag bits are added
//! the same way. A reader takes such a block as it is ([Block::body_known] false, no records) and goes on with the
//! next, instead of taking it for damage. [Describe] is an evolvable enum, so a describe record of a variant a reader
//! doesn't know reads as [Describe::Unknown] and is skipped.
//!
//! Numbers are little endian; `uvar` is LEB128 (7 bits per byte, low bits first, at most 10 bytes), `ivar` zigzag
//! then `uvar`, as in the logring format. Blocks are as long as their records, never padded: a file is blocks
//! appended back to back, a live feed sends one block per message.
//!
//! A **describe** block carries records on track 0 only, each a [Describe]: the [Producer], the API bundles, every
//! [ClockDef], [ClockMap] and [TrackDef] in use and typed items ([TypedItem]: a run record, a media reference), in
//! the canonical order of [Describe::rank]; after them, on their own tracks, the newest record of every *sticky*
//! track (state a reader joining late needs, e.g. a link's session). A producer writes a describe set at the start of
//! a file, when a track is added, when a live consumer joins and after blocks were lost, never by time, so record
//! blocks carry only records and a trickle of records costs a block header each, not a description.
//!
//! Describe blocks a writer writes again (after a piece's first data block, the repeats, the backup before a piece's
//! finish, the start of the next piece) are copies of the newest describe run, sticky records included, marked
//! [FLAG_COPY] and with the seq of the block before them.
//! Their sticky records are the state as of the run they copy, at its times, not at the copy's place in the file. A
//! reader that read on to a copy takes no sticky record from it: the records read since are newer. A reader starting
//! at a copy takes them as the state at their times.
//!
//! A describe set's first block is marked [FLAG_SET_FIRST] and its last [FLAG_SET_LAST]: a set is whole when a block
//! marked first is followed, back to back, by blocks up to one marked last; part of a set is never taken for one.
//!
//! **Gaps.** A [Layout] says how far a writer may leave a blank or bad stretch and go on after it ([Layout::gap_max])
//! and where it resumes ([Layout::resume_align]). A reader's cost per gap is bounded by the format: `gap_max` is at
//! most [GAP_MAX_MAX] (64 MiB) with at most [GAP_PROBES_MAX] (8192) resume places in it, else the layout record
//! doesn't decode; a reader reads the first non-blank byte's and each resume place's header one at a time, and checks
//! at most [GAP_CHECKS_MAX] (4) whole blocks whose header says they go on with the data. On a raw medium ([Medium])
//! blank space longer than `gap_max` is the end of data; a piece there is one run (`TapeWriter::adopt_block`).
//!
//! **Limits.** Every count, length, offset or layout value a reader takes from the bytes is bounded before it sizes
//! an allocation, a slice or a loop ([FromFile]): by a length already in hand (the block's, a table's) or by one of
//! these caps. A value past its cap is damage, refused with an error; no file a writer of this crate writes is.
//!
//! | Cap | Value | Bounds |
//! |---|---|---|
//! | [MAX_BLOCK_LEN] | 64 MiB | a block's length |
//! | [DECODED_BODY_MAX](body::DECODED_BODY_MAX) | 64 MiB - 36 | a coded body's decoded length |
//! | [LZ4_DECODED_MAX](lz4::LZ4_DECODED_MAX) | 64 MiB | an lz4 column's decoded length |
//! | [MAX_BLOCK_CLOCKS] | 16 | the clocks of one block's clock table |
//! | [MAX_RECORD_HEAD] | 25 | a record head's bytes |
//! | [MAX_CLOCK_TABLE] | 81 | a clock table's bytes |
//! | [MAX_COLUMNS](chunk::MAX_COLUMNS) | 255 | a chunk's columns |
//! | [SEGMENT_SHIFT_MAX](chunk::SEGMENT_SHIFT_MAX) | 24 | a chunk's segment shift (16 MiB segments) |
//! | [MAX_SEGMENTS](chunk::MAX_SEGMENTS) | 16384 | a chunk's segments |
//! | [Codec::SERIES_ITEMS_MAX](chunk::Codec::SERIES_ITEMS_MAX) | 65535 | the items of a chunk with a series payloads column |
//! | [WIDTH_MAX](bitpack::WIDTH_MAX) | 64 | a bit width ([bitpack::Width]) |
//! | [KERNEL_WIDTH_MAX](bitpack::KERNEL_WIDTH_MAX) | 32 | a bit width the 32-bit kernels unpack |
//! | [SLOPE_MAX](times::SLOPE_MAX) | 2^55 | a start group's slope (Q16 ticks per item) |
//! | [INDEX_LEN_MAX](index::INDEX_LEN_MAX) | 1 MiB | an index block a writer makes, and a reader looks for from the end |
//! | [CHECKPOINT_ENTRIES_MAX](index::CHECKPOINT_ENTRIES_MAX) | 14562 | entries a writer puts in a checkpoint index block |
//! | [NODE_SHIFT_MAX](summary::NODE_SHIFT_MAX) | 62 | a summary node's items, as a power of 2 |
//! | [LEVEL_MAX](summary::LEVEL_MAX) | 15 | a summary tree's level |
//! | [SCALE_LEN_MAX](summary::SCALE_LEN_MAX) | 64 | a series summary's scale bytes |
//! | [FACTOR_MAX](summary::FACTOR_MAX) | 2^53 | a factor to the common scale |
//! | [SeriesSummarySpec::BASE_SHIFT_MAX](summary::SeriesSummarySpec::BASE_SHIFT_MAX) | 24 | a series summary's base node, as a power of 2 |
//! | [SERIES_CHANNELS_MAX](series::SERIES_CHANNELS_MAX) | 65535 | a series track's channels |
//! | [GAP_MAX_MAX] | 64 MiB | a layout's gap |
//! | [GAP_PROBES_MAX] | 8192 | the resume places a reader probes in one gap |
//! | [GAP_CHECKS_MAX] | 4 | the blocks a reader checks whole past one gap |
//! | [DESCRIBE_SEEK_BLOCKS_MAX] | 64 | the blocks a reader skips looking for a describe copy |
//! | [DEFERRED_BYTES_MAX](writer::DEFERRED_BYTES_MAX) | 1 MiB | the deferred blocks a writer keeps |
//!
//! The same input gives the same bytes: blocks hold no wall time, describe records go in canonical order
//! ([DescribeSet] with `std`) and blocks are cut by size, not by time, unless the producer seals them on a timer.
//! One exception: an [lz4] payload column is identical only for one `lz4_flex` version on targets of the
//! same pointer width and endianness (decoding is unaffected).

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_code)]
#![cfg_attr(
    test,
    allow(
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
        reason = "tests fail by panicking and compute expected values freely"
    )
)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
pub use ww_version::FullVersionOwned;
pub use ww_version::{FullVersion, Version};

mod describe;
pub use describe::{
    ALIGNMENT, ClockDef, ClockMap, ClockOrigin, ClockRate, ClockResolution, Describe, Dimension,
    Ends, EventsDef, GAP_CHECKS_MAX, GAP_MAX_MAX, GAP_PROBES_MAX, Layout, MapSegment, NO_CLOCK,
    Producer, Rank, SD_GAP_MAX, SD_RESUME_ALIGN, TrackDef, TrackKind, TypeRef, TypedItem,
    UNIX_CLOCK_KEY, clock_key, segments_increasing, track_id,
};
#[cfg(feature = "std")]
pub use describe::{
    ClockDefOwned, ClockMapOwned, DescribeOwned, DimensionOwned, ProducerOwned, TrackDefOwned,
    TrackKindOwned, TypeRefOwned, TypedItemOwned,
};
pub use exact_units::{Magnitude, MagnitudeOf, Ratio64, Rounding};

pub mod summary;
pub use summary::{EventFields, EventNode, Summary, SummaryError, SummaryHead};

pub mod bitpack;
pub mod chunk;
pub mod times;
pub use chunk::{
    Chunk, ChunkError, ChunkHead, ChunkHeadError, ChunkWriter, Codec, ColumnKind, ColumnSpec,
    EndMode,
};

mod bytes;
pub mod from_file;
pub use from_file::{BoundError, FromFile};
pub mod lz4;
#[cfg(feature = "alloc")]
pub mod payloads;
pub mod samples;
pub use samples::{Element, Sample, SampleError, SampleShape, Samples};

pub mod series;
#[cfg(feature = "alloc")]
pub use series::SeriesTimeline;
pub use series::{MediaStream, SeriesDef, SeriesError, SeriesSegment, SeriesValue};
#[cfg(feature = "std")]
pub use series::{MediaStreamOwned, SeriesDefOwned};

pub mod index;
pub use index::{IndexBlock, IndexEntry, IndexError, IndexHead, IndexKind, IndexPolicy};
pub mod piece;
#[cfg(feature = "std")]
pub use piece::{ChainOptions, CutPolicy, PieceChain};
pub use piece::{
    ContentHash, Finish, FinishState, PieceHead, PieceId, PieceIdent, ResultId, TAIL_LEN, Tail,
    TrackRange, TrackStart,
};
#[cfg(feature = "alloc")]
pub mod reader;
#[cfg(feature = "alloc")]
pub use reader::{
    ByteSource, DESCRIBE_SEEK_BLOCKS_MAX, OpenOptions, PieceInfo, ReadError, TapeReader,
};

mod event;
pub use event::{EventRecord, EventRecordError};
pub mod body;
pub use body::{BodyCodec, CodeError, code_body, coded_len_max};
mod run;
pub use run::{BlockId, Medium, RunTag};
mod varint;
#[cfg(feature = "alloc")]
mod walk;
pub use varint::{ivar_len, uvar_len};

#[cfg(feature = "std")]
mod file;
#[cfg(feature = "std")]
pub use file::{BlockBuf, Cut, FileBlocks, FileError, Recovery};
#[cfg(feature = "std")]
mod set;
#[cfg(feature = "std")]
pub use set::{DescribeError, DescribeSet, Sticky};
#[cfg(feature = "std")]
pub mod writer;
#[cfg(feature = "std")]
pub use writer::{Finished, SyncPolicy, TapeWriter, WriteError};

/// `WWTP`
pub const MAGIC: [u8; 4] = *b"WWTP";
pub const VERSION: u8 = 2;
/// Bytes of the block header.
pub const HEADER_LEN: usize = 32;
/// Longest block a reader takes (64 MiB): a longer length is damage. Blocks are small (a producer hands over 64 KiB
/// ones); the largest are describe blocks with big API bundles.
pub const MAX_BLOCK_LEN: usize = 64 << 20;
const _: () = assert!(
    MAX_BLOCK_LEN >> 31 == 0,
    "a block length is a u32 in the header"
);
/// Header flag: the block is a copy a writer made of a block it wrote before (a describe block written again: a
/// repeat, the backup before a piece's finish, the start of the next piece). It carries the seq of the block before
/// it and takes no part in the seq order; a reader that read the original takes nothing new from it.
pub const FLAG_COPY: u16 = 1 << 1;
/// Header flag: the first block of a describe set. With [FLAG_SET_LAST] it tells a whole set from part of one: a set
/// is whole when a block marked first is followed, back to back, by blocks up to one marked last (the same block for
/// a set of one block). [DescribeSet::write_blocks] marks them.
pub const FLAG_SET_FIRST: u16 = 1 << 2;
/// Header flag: the last block of a describe set, see [FLAG_SET_FIRST].
pub const FLAG_SET_LAST: u16 = 1 << 3;
/// Header flag: the body of a records or describe block is coded ([body]); on another kind the body is one this
/// reader doesn't know. Off unless a writer codes the block with [code_body].
pub const FLAG_BODY_CODED: u16 = 1 << 0;
/// Bytes of the CRC at the end of a block.
pub const CRC_LEN: usize = 4;
/// Track 0: the producer's own, describe records.
pub const DESCRIBE_TRACK: u32 = 0;
/// Most clocks in one block's clock table; a producer writing on more starts another block.
pub const MAX_BLOCK_CLOCKS: usize = 16;
/// Longest record head: head, time delta and length varints.
pub const MAX_RECORD_HEAD: usize = 10 + 10 + 5;
/// Longest clock table.
pub const MAX_CLOCK_TABLE: usize = 1 + 5 * MAX_BLOCK_CLOCKS;

const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// Records of any tracks, interleaved.
    Records,
    Describe,
    /// One chunk of one event track ([chunk]).
    Events,
    /// One chunk of one fixed-rate series ([chunk]).
    Series,
    /// One batch of nodes of one level of a track's summary tree ([summary]).
    Summary,
    /// Reserved for parity blocks (one per group of blocks, for archives on poor media); none are written yet, and a
    /// reader skips them like a kind it doesn't know.
    Parity,
    /// A checkpoint or final index of the piece's blocks ([index]).
    Index,
    /// How a piece ended, with its content hash ([piece]).
    Finish,
    /// The fixed-size last block of a piece ([piece]).
    Tail,
    /// The first block of a piece of a chain ([piece]).
    PieceHead,
    /// A kind this reader doesn't know: its body isn't read.
    Other(u8),
}

impl BlockKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => BlockKind::Records,
            2 => BlockKind::Describe,
            3 => BlockKind::Events,
            4 => BlockKind::Series,
            5 => BlockKind::Parity,
            6 => BlockKind::Summary,
            7 => BlockKind::Index,
            8 => BlockKind::Finish,
            9 => BlockKind::Tail,
            10 => BlockKind::PieceHead,
            other => BlockKind::Other(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            BlockKind::Records => 1,
            BlockKind::Describe => 2,
            BlockKind::Events => 3,
            BlockKind::Series => 4,
            BlockKind::Parity => 5,
            BlockKind::Summary => 6,
            BlockKind::Index => 7,
            BlockKind::Finish => 8,
            BlockKind::Tail => 9,
            BlockKind::PieceHead => 10,
            BlockKind::Other(v) => v,
        }
    }
}

/// Whether `b` is a blank byte: 0x00 (zeros, preallocated space, a trimmed SD card or eMMC) or 0xFF (erased NAND,
/// an SD card erased to ones). A block starts with `W` (0x57), never with a blank byte, and a stretch of blank bytes
/// (any mix of the two) where a block would start is no block.
pub const fn is_blank(b: u8) -> bool {
    b == 0x00 || b == 0xFF
}

/// FNV-1a, 64 bit.
pub const fn fnv1a(mut h: u64, mut bytes: &[u8]) -> u64 {
    while let [b, rest @ ..] = bytes {
        #[expect(clippy::as_conversions, reason = "u8 to u64 widens; From is not const")]
        let b = *b as u64;
        h ^= b;
        h = h.wrapping_mul(0x100000001b3);
        bytes = rest;
    }
    h
}

pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// Names an API across its minor versions: FNV-1a of the crate name and the major version (u32 LE), as logring's
/// `api_root_id`.
pub const fn api_root_id(crate_name: &str, major: u32) -> u64 {
    fnv1a(
        fnv1a(FNV_OFFSET, crate_name.as_bytes()),
        &major.to_le_bytes(),
    )
}

/// Stable key of a track: FNV-1a of `api_root` (u64 LE), then of every path id (u32 LE), as logring's track key.
pub fn track_key(api_root: u64, path: &[u32]) -> u64 {
    let mut h = fnv1a(FNV_OFFSET, &api_root.to_le_bytes());
    for id in path {
        h = fnv1a(h, &id.to_le_bytes());
    }
    h
}

/// A record's time: ticks of a clock.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClockTime {
    /// [ClockDef::number].
    pub clock: u32,
    pub ticks: i64,
}

impl ClockTime {
    pub const fn new(clock: u32, ticks: i64) -> Self {
        ClockTime { clock, ticks }
    }
}

/// Bits of the slot in a record head for `n` clocks.
const fn slot_bits(n: usize) -> u32 {
    match n.checked_sub(1) {
        None | Some(0) => 0,
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "m is nonzero: at most usize::BITS - 1 leading zeros"
        )]
        Some(m) => usize::BITS - m.leading_zeros(),
    }
}

/// The clocks of a block and the last time on each.
#[derive(Copy, Clone, Debug)]
struct Slots {
    clocks: [u32; MAX_BLOCK_CLOCKS],
    last: [i64; MAX_BLOCK_CLOCKS],
    n: u8,
}

impl Slots {
    const NONE: Slots = Slots {
        clocks: [0; MAX_BLOCK_CLOCKS],
        last: [0; MAX_BLOCK_CLOCKS],
        n: 0,
    };

    /// Slots in use; `n` is at most [MAX_BLOCK_CLOCKS] where it is set.
    fn len(&self) -> usize {
        usize::from(self.n).min(MAX_BLOCK_CLOCKS)
    }

    fn clocks(&self) -> &[u32] {
        self.clocks.get(..self.len()).unwrap_or_default()
    }

    fn bits(&self) -> u32 {
        slot_bits(self.len())
    }

    fn slot(&self, clock: u32) -> Option<usize> {
        self.clocks().binary_search(&clock).ok()
    }
}

/// The block does not fit: nothing was written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Full;

/// Why [BlockWriter::new] can't start a block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockStartError {
    /// More clocks than [MAX_BLOCK_CLOCKS].
    TooManyClocks,
    /// The clocks are not strictly ascending.
    ClocksUnordered,
    /// One of the clocks is [NO_CLOCK].
    NoClock,
    /// The buffer doesn't hold the header, the clock table and the CRC: `needed` bytes.
    BufferTooSmall { needed: usize },
}

/// Why [BlockWriter::push] or [BlockWriter::push_with] wrote nothing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PushError {
    /// The record doesn't fit the block.
    Full,
    /// Track [DESCRIBE_TRACK] has no time: describe records go through [BlockWriter::push_describe].
    DescribeTrack,
    /// The record's clock is not one the block was started with.
    UnknownClock(u32),
}

impl From<Full> for PushError {
    fn from(_: Full) -> Self {
        PushError::Full
    }
}

/// Writes one block into a buffer: [BlockWriter::push] records, then [BlockWriter::finish].
pub struct BlockWriter<'b> {
    buf: &'b mut [u8],
    state: BlockState,
}

impl<'b> BlockWriter<'b> {
    /// Start a block in `buf` holding records on `clocks` (strictly ascending clock numbers, none of them
    /// [NO_CLOCK]; at most [MAX_BLOCK_CLOCKS]); at most [MAX_BLOCK_LEN] of `buf` is used. A `buf` that doesn't hold
    /// the header, the clock table and the CRC is refused.
    pub fn new(
        buf: &'b mut [u8],
        kind: BlockKind,
        id: BlockId,
        clocks: &[u32],
    ) -> Result<Self, BlockStartError> {
        if clocks.len() > MAX_BLOCK_CLOCKS {
            return Err(BlockStartError::TooManyClocks);
        }
        if !clocks.is_sorted_by(|a, b| a < b) {
            return Err(BlockStartError::ClocksUnordered);
        }
        if clocks.contains(&NO_CLOCK) {
            return Err(BlockStartError::NoClock);
        }
        let n = u8::try_from(clocks.len())
            .ok()
            .ok_or(BlockStartError::TooManyClocks)?;
        let table = || core::iter::once(u64::from(n)).chain(clocks.iter().map(|c| u64::from(*c)));
        // at most MAX_CLOCK_TABLE bytes: 16 clocks of 5 bytes and the count
        let needed = table()
            .map(uvar_len)
            .try_fold(HEADER_LEN + CRC_LEN, usize::checked_add)
            .ok_or(BlockStartError::TooManyClocks)?;
        let small = BlockStartError::BufferTooSmall { needed };
        let usable = buf.len().min(MAX_BLOCK_LEN);
        if usable < needed {
            return Err(small);
        }
        let buf = buf.get_mut(..usable).ok_or(small)?;
        put_header(buf, kind, 0, 0, id).ok_or(small)?;
        let mut pos = HEADER_LEN;
        for v in table() {
            let at = buf.get_mut(pos..).ok_or(small)?;
            pos = pos
                .checked_add(varint::put_uvar(at, v).ok_or(small)?)
                .ok_or(small)?;
        }
        let mut slots = Slots::NONE;
        slots.n = n;
        for (slot, c) in slots.clocks.iter_mut().zip(clocks) {
            *slot = *c;
        }
        Ok(BlockWriter {
            buf,
            state: BlockState {
                pos,
                body: pos,
                records: 0,
                slots,
            },
        })
    }

    /// Release the buffer, keeping what is needed to go on with [Self::from_state] on the same buffer.
    pub fn into_state(self) -> BlockState {
        self.state
    }

    /// Go on writing a block started in `buf` (the same bytes, unchanged since [Self::into_state]).
    ///
    /// # Panics
    ///
    /// When `buf` is shorter than the block written so far and its CRC: not the buffer of `state`.
    pub fn from_state(buf: &'b mut [u8], state: BlockState) -> Self {
        let usable = buf.len().min(MAX_BLOCK_LEN);
        assert!(
            state
                .pos
                .checked_add(CRC_LEN)
                .is_some_and(|end| end <= usable),
            "from_state takes the buffer the block was started in"
        );
        let (buf, _) = buf.split_at_mut(usable);
        BlockWriter { buf, state }
    }

    /// Bytes of records written so far.
    pub fn used(&self) -> usize {
        self.state.pos.saturating_sub(self.state.body)
    }

    pub fn records(&self) -> u32 {
        self.state.records
    }

    /// The clocks the block was started with.
    pub fn clocks(&self) -> &[u32] {
        self.state.slots.clocks()
    }

    /// Free bytes for records, the CRC left aside.
    pub fn free(&self) -> usize {
        // new and from_state hold pos + CRC_LEN <= buf.len(), put keeps it
        self.buf
            .len()
            .saturating_sub(CRC_LEN)
            .saturating_sub(self.state.pos)
    }

    /// Add a record on `track` at `at` whose item is `parts` back to back; `at`'s clock is the track's
    /// [TrackDef::clock]. Track [DESCRIBE_TRACK] (see [Self::push_describe]) and a clock that isn't one of the
    /// block's are refused.
    pub fn push(&mut self, track: u32, at: ClockTime, parts: &[&[u8]]) -> Result<(), PushError> {
        self.push_with(track, at, |buf| copy_parts(buf, parts))
    }

    /// Add a describe record (track 0, no time) whose item is `parts` back to back.
    pub fn push_describe(&mut self, parts: &[&[u8]]) -> Result<(), Full> {
        self.push_describe_with(|buf| copy_parts(buf, parts))
    }

    /// [Self::push] with an item `encode` writes into the free space and returns the length of, so a ShrinkWrap
    /// value is serialized in place without a copy. `encode` returning None (did not fit) leaves the block as it was.
    pub fn push_with(
        &mut self,
        track: u32,
        at: ClockTime,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), PushError> {
        if track == DESCRIBE_TRACK {
            return Err(PushError::DescribeTrack);
        }
        let slots = &self.state.slots;
        let slot = slots
            .slot(at.clock)
            .ok_or(PushError::UnknownClock(at.clock))?;
        // a track number of 32 bits shifted by at most 4 slot bits fits a u64
        let head = (u64::from(track) << slots.bits()) | bytes::u64_of(slot);
        let last = slots
            .last
            .get(slot)
            .ok_or(PushError::UnknownClock(at.clock))?;
        let dt = at.ticks.wrapping_sub(*last);
        self.put(head, Some(dt), encode)?;
        if let Some(last) = self.state.slots.last.get_mut(slot) {
            *last = at.ticks;
        }
        Ok(())
    }

    /// [Self::push_describe] with an item `encode` writes in place, as [Self::push_with].
    pub fn push_describe_with(
        &mut self,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), Full> {
        self.put(0, None, encode)
    }

    /// Write a record head, its time and an item `encode` writes; nothing is kept when any part doesn't fit.
    fn put(
        &mut self,
        head: u64,
        dt: Option<i64>,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), Full> {
        let records = self.state.records.checked_add(1).ok_or(Full)?;
        let fixed = uvar_len(head)
            .checked_add(dt.map_or(0, ivar_len))
            .ok_or(Full)?;
        // room for the item's length and at least one byte
        let room = self
            .free()
            .checked_sub(fixed)
            .filter(|r| *r > 0)
            .ok_or(Full)?;
        // the item goes after the longest length the free space can need, then moves down to its real place
        let max_len_len = uvar_len(bytes::u64_of(room));
        let item_at = self
            .state
            .pos
            .checked_add(fixed)
            .and_then(|a| a.checked_add(max_len_len))
            .ok_or(Full)?;
        let end = self.buf.len().checked_sub(CRC_LEN).ok_or(Full)?;
        let space = self.buf.get_mut(item_at..end).ok_or(Full)?;
        let space_len = space.len();
        let len = encode(space).filter(|l| *l <= space_len).ok_or(Full)?;
        let len_len = uvar_len(bytes::u64_of(len));
        let mut pos = self.state.pos;
        let n = varint::put_uvar(self.buf.get_mut(pos..).ok_or(Full)?, head).ok_or(Full)?;
        pos = pos.checked_add(n).ok_or(Full)?;
        if let Some(dt) = dt {
            let n = varint::put_ivar(self.buf.get_mut(pos..).ok_or(Full)?, dt).ok_or(Full)?;
            pos = pos.checked_add(n).ok_or(Full)?;
        }
        // pos + len_len <= item_at: the length takes at most max_len_len bytes
        let real_item_at = pos
            .checked_add(len_len)
            .filter(|a| *a <= item_at)
            .ok_or(Full)?;
        let item_end = item_at.checked_add(len).ok_or(Full)?;
        self.buf.get(item_at..item_end).ok_or(Full)?;
        self.buf.copy_within(item_at..item_end, real_item_at);
        let len_at = self.buf.get_mut(pos..real_item_at).ok_or(Full)?;
        varint::put_uvar(len_at, bytes::u64_of(len)).ok_or(Full)?;
        self.state.pos = real_item_at.checked_add(len).ok_or(Full)?;
        self.state.records = records;
        Ok(())
    }

    /// Write the header's length and count and the CRC; returns the block's length (`&buf[..len]` is the block).
    /// Set the header flags (0 when not set), before [Self::finish]: [FLAG_SET_FIRST] and [FLAG_SET_LAST] on the
    /// describe blocks of a set made without [DescribeSet::write_blocks].
    pub fn set_flags(&mut self, flags: u16) {
        // new wrote the header: buf holds HEADER_LEN bytes
        _ = bytes::put_u16(self.buf, 6, flags);
    }

    pub fn finish(self) -> usize {
        let pos = self.state.pos;
        let (body, rest) = self.buf.split_at_mut(pos.min(self.buf.len()));
        #[expect(
            clippy::expect_used,
            reason = "new and from_state hold pos + CRC_LEN <= buf.len() and pos <= MAX_BLOCK_LEN; put keeps it"
        )]
        let (end, len, crc_at) = pos
            .checked_add(CRC_LEN)
            .and_then(|end| {
                let len = u32::try_from(end).ok()?;
                Some((end, len, rest.first_chunk_mut::<CRC_LEN>()?))
            })
            .expect("room for the CRC after the records");
        // the header is in body: pos is at least HEADER_LEN
        _ = bytes::put_u32(body, 8, len);
        _ = bytes::put_u32(body, 12, self.state.records);
        *crc_at = CRC.checksum(body).to_le_bytes();
        end
    }
}

fn copy_parts(buf: &mut [u8], parts: &[&[u8]]) -> Option<usize> {
    let mut rest = buf;
    let mut len = 0usize;
    for p in parts {
        let (dst, tail) = rest.split_at_mut_checked(p.len())?;
        dst.copy_from_slice(p);
        rest = tail;
        len = len.checked_add(p.len())?;
    }
    Some(len)
}

/// A [BlockWriter] without its buffer, see [BlockWriter::into_state].
#[derive(Copy, Clone, Debug)]
pub struct BlockState {
    pos: usize,
    body: usize,
    records: u32,
    slots: Slots,
}

impl BlockState {
    pub fn records(&self) -> u32 {
        self.records
    }
}

/// Write the 32-byte header of a block of `kind`, `len` bytes and `count` records with `id` into `buf`: flags and
/// bytes 28..32 zero.
/// None, writing nothing, when `buf` is shorter than the header or `len` is over [MAX_BLOCK_LEN].
#[must_use = "None means no header was written"]
pub(crate) fn put_header(
    buf: &mut [u8],
    kind: BlockKind,
    len: usize,
    count: u32,
    id: BlockId,
) -> Option<()> {
    if len > MAX_BLOCK_LEN {
        return None;
    }
    let len = u32::try_from(len).ok()?;
    let h = buf.first_chunk_mut::<HEADER_LEN>()?;
    h.fill(0);
    bytes::put(h, 0, MAGIC)?;
    bytes::put(h, 4, [VERSION, kind.to_u8()])?;
    bytes::put_u32(h, 8, len)?;
    bytes::put_u32(h, 12, count)?;
    bytes::put_u64(h, 16, id.seq)?;
    bytes::put_u32(h, 24, id.run.get())
}

/// Why a block can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockError {
    /// Fewer bytes than the header or its length says.
    Short {
        needed: usize,
    },
    BadMagic,
    BadLength(u32),
    BadCrc,
    /// The head of a chunk block is bad.
    Chunk(ChunkHeadError),
    /// The head or body of a summary block is bad.
    Summary(SummaryError),
    /// The clock table or the records don't parse up to the end, or there are more or fewer than the header says.
    BadRecords,
    /// The records of a coded body are read from [Block::decode].
    Coded,
}

/// A block read from bytes, CRC checked. Records are read in place, see [Block::records].
#[derive(Copy, Clone, Debug)]
pub struct Block<'b> {
    /// The format version it was written with, [VERSION] or a later one.
    pub version: u8,
    pub kind: BlockKind,
    pub seq: u64,
    /// The run the block belongs to, as written: a reader decides whether it is the current one.
    pub run: RunTag,
    /// Header flags ([FLAG_COPY]); bits this reader doesn't know are kept, not read.
    pub flags: u16,
    pub record_count: u32,
    bytes: &'b [u8],
    /// The bytes the block CRC covers: all but the last [CRC_LEN].
    covered: &'b [u8],
    /// The body: between the header and the block CRC.
    body: &'b [u8],
    /// The block CRC as written.
    crc: u32,
    /// False for a chunk block handed out by [Block::salvage] whose payload doesn't match its CRCs.
    intact: bool,
}

/// A record of a [Block], borrowed from it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Record<'b> {
    pub track: u32,
    /// None for a describe record (track 0).
    pub at: Option<ClockTime>,
    pub item: &'b [u8],
}

/// Length of the block at the start of `bytes` from its header, without checking more than the magic; for readers
/// of a byte stream.
pub fn block_len(bytes: &[u8]) -> Result<usize, BlockError> {
    let short = BlockError::Short { needed: HEADER_LEN };
    let h = bytes.first_chunk::<HEADER_LEN>().ok_or(short)?;
    if bytes::array::<4>(h, 0) != Some(MAGIC) {
        return Err(BlockError::BadMagic);
    }
    let raw = bytes::le_u32(h, 8).ok_or(short)?;
    FromFile::new(raw)
        .at_most(MAX_BLOCK_LEN)
        .ok()
        .filter(|len| *len >= HEADER_LEN + CRC_LEN)
        .ok_or(BlockError::BadLength(raw))
}

/// Give the whole, good block `block` the run and seq of `id` and rewrite its CRCs (a chunk or index block's head CRC
/// too), so a writer that places blocks later than it made them (a summary block filling the gap before an aligned
/// chunk) keeps seqs ascending in the file and every block in its run.
pub fn set_id(block: &mut [u8], id: BlockId) -> Result<(), BlockError> {
    restamp(block, id, 0)
}

/// [set_id], and mark the block a [copy](FLAG_COPY) a writer made of a block written before.
pub fn mark_copy(block: &mut [u8], id: BlockId) -> Result<(), BlockError> {
    restamp(block, id, FLAG_COPY)
}

/// Give the good block `block` the id `id` and the header flags `set` too, its CRCs rewritten.
fn restamp(block: &mut [u8], id: BlockId, set: u16) -> Result<(), BlockError> {
    let b = Block::read(block)?;
    if b.len() != block.len() {
        return Err(BlockError::BadLength(
            u32::try_from(block.len()).unwrap_or(u32::MAX),
        ));
    }
    let chunk = b.is_chunk() && b.body_known();
    let index = b.kind == BlockKind::Index && b.body_known();
    let flags = b.flags | set;
    // a block read whole holds its header and CRC
    let short = BlockError::Short {
        needed: HEADER_LEN + CRC_LEN,
    };
    bytes::put_u16(block, 6, flags).ok_or(short)?;
    bytes::put_u64(block, 16, id.seq).ok_or(short)?;
    bytes::put_u32(block, 24, id.run.get()).ok_or(short)?;
    if chunk {
        chunk::restamp(block).map_err(BlockError::Chunk)?;
    } else {
        if index {
            index::restamp_head(block);
        }
        let (covered, crc) = block.split_last_chunk_mut::<CRC_LEN>().ok_or(short)?;
        *crc = CRC.checksum(covered).to_le_bytes();
    }
    Ok(())
}

/// The clock table at the start of a body: the slots and where the records start in `body`.
fn read_clock_table(body: &[u8]) -> Option<(Slots, usize)> {
    let mut rd = varint::Reader(body);
    let n = rd.uvar_file()?.at_most(MAX_BLOCK_CLOCKS).ok()?;
    let mut slots = Slots::NONE;
    slots.n = u8::try_from(n).ok()?;
    // strictly ascending from NO_CLOCK (0): no clock is 0 and none repeats
    let mut prev = NO_CLOCK;
    for slot in slots.clocks.iter_mut().take(n) {
        let c = u32::try_from(rd.uvar()?).ok()?;
        if c <= prev {
            return None;
        }
        *slot = c;
        prev = c;
    }
    Some((slots, body.len().checked_sub(rd.0.len())?))
}

impl<'b> Block<'b> {
    /// Read the block at the start of `bytes`; bytes after it are not looked at ([Block::len] says where it ends).
    ///
    /// A chunk block is checked by its head CRC, its segment CRCs and its block CRC, in one pass over the payload.
    pub fn read(bytes: &'b [u8]) -> Result<Self, BlockError> {
        let (block, chunk) = Self::read_unchecked(bytes)?;
        match chunk {
            Some(c) if c.check_payload() => Ok(block),
            Some(_) => Err(BlockError::BadCrc),
            None => {
                if CRC.checksum(block.covered) != block.crc {
                    return Err(BlockError::BadCrc);
                }
                block.check_records()?;
                if block.is_summary() {
                    Summary::read(block.bytes).map_err(BlockError::Summary)?;
                }
                Ok(block)
            }
        }
    }

    /// Read a block as [Self::read] does, but take a chunk block whose payload is damaged as long as its head is good
    /// (head CRC, ranges): [Self::is_intact] is then false and [Chunk::bad_segments] says what is lost.
    pub fn salvage(bytes: &'b [u8]) -> Result<Self, BlockError> {
        let (mut block, chunk) = Self::read_unchecked(bytes)?;
        match chunk {
            Some(c) => {
                block.intact = c.check_payload();
                Ok(block)
            }
            None => Self::read(bytes),
        }
    }

    /// The block's header, with no bytes: [Block::on] lends it again on the bytes it was read from, without a
    /// second check.
    #[cfg(feature = "alloc")]
    pub(crate) fn detach(&self) -> Block<'static> {
        Block {
            version: self.version,
            kind: self.kind,
            seq: self.seq,
            run: self.run,
            flags: self.flags,
            record_count: self.record_count,
            bytes: &[],
            covered: &[],
            body: &[],
            crc: self.crc,
            intact: self.intact,
        }
    }

    /// A [Block::detach]ed header on `bytes`, the whole block it was read from; None when `bytes` is too short for
    /// a block.
    #[cfg(feature = "alloc")]
    pub(crate) fn on<'c>(&self, bytes: &'c [u8]) -> Option<Block<'c>> {
        let (covered, _) = bytes.split_last_chunk::<CRC_LEN>()?;
        let (_, body) = covered.split_first_chunk::<HEADER_LEN>()?;
        Some(Block {
            version: self.version,
            kind: self.kind,
            seq: self.seq,
            run: self.run,
            flags: self.flags,
            record_count: self.record_count,
            bytes,
            covered,
            body,
            crc: self.crc,
            intact: self.intact,
        })
    }

    /// The header, and for a chunk block its head checked; no CRC over the payload yet.
    fn read_unchecked(bytes: &'b [u8]) -> Result<(Self, Option<Chunk<'b>>), BlockError> {
        let len = block_len(bytes)?;
        let short = BlockError::Short { needed: len };
        let (whole, _) = bytes.split_at_checked(len).ok_or(short)?;
        // block_len: len is at least HEADER_LEN + CRC_LEN
        let (covered, crc) = whole.split_last_chunk::<CRC_LEN>().ok_or(short)?;
        let (h, body) = covered.split_first_chunk::<HEADER_LEN>().ok_or(short)?;
        let block = Block {
            version: h[4],
            kind: BlockKind::from_u8(h[5]),
            record_count: bytes::le_u32(h, 12).ok_or(short)?,
            seq: bytes::le_u64(h, 16).ok_or(short)?,
            flags: bytes::le_u16(h, 6).ok_or(short)?,
            run: RunTag::from_u32(bytes::le_u32(h, 24).ok_or(short)?),
            bytes: whole,
            covered,
            body,
            crc: u32::from_le_bytes(*crc),
            intact: true,
        };
        if block.body_known() && block.is_chunk() {
            let chunk = Chunk::read_head(block.bytes).map_err(BlockError::Chunk)?;
            return Ok((block, Some(chunk)));
        }
        Ok((block, None))
    }

    /// The records parse to the end and are as many as the header says.
    fn check_records(&self) -> Result<(), BlockError> {
        let records = matches!(self.kind, BlockKind::Records | BlockKind::Describe);
        if self.version == VERSION && records && self.is_coded() && self.coded().is_none() {
            return Err(BlockError::BadRecords);
        }
        if !self.has_records() || self.plain_body().is_none() {
            return Ok(());
        }
        if self.clock_table().is_none() {
            return Err(BlockError::BadRecords);
        }
        // a record takes 2 bytes at least: no more of them than body bytes
        let mut n = 0u32;
        for r in self.records() {
            r?;
            n = n.checked_add(1).ok_or(BlockError::BadRecords)?;
        }
        if n != self.record_count {
            return Err(BlockError::BadRecords);
        }
        Ok(())
    }

    /// Whether the block is a [copy](FLAG_COPY) a writer made.
    pub fn is_copy(&self) -> bool {
        self.flags & FLAG_COPY != 0
    }

    /// The block's CRC, the last 4 bytes: with its length, what tells two blocks of one id apart.
    pub fn crc(&self) -> u32 {
        self.crc
    }

    /// Whether the block is good throughout. Only [Self::salvage] hands out one that isn't.
    pub fn is_intact(&self) -> bool {
        self.intact
    }

    fn is_chunk(&self) -> bool {
        matches!(self.kind, BlockKind::Events | BlockKind::Series)
    }

    fn is_summary(&self) -> bool {
        self.body_known() && self.kind == BlockKind::Summary
    }

    /// The summary of a summary block whose body this reader knows.
    pub fn summary(&self) -> Option<Summary<'b>> {
        if !self.is_summary() {
            return None;
        }
        Summary::read(self.bytes).ok()
    }

    fn has_records(&self) -> bool {
        self.body_known() && matches!(self.kind, BlockKind::Records | BlockKind::Describe)
    }

    /// The chunk of an events or series block whose body this reader knows.
    pub fn chunk(&self) -> Option<Chunk<'b>> {
        if !(self.body_known() && self.is_chunk()) {
            return None;
        }
        Chunk::read_head(self.bytes).ok()
    }

    /// The whole block, header and CRC included.
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.record_count == 0
    }

    /// The clocks of its records, ascending; none for a block whose body this reader doesn't know.
    pub fn clocks(&self) -> impl Iterator<Item = u32> {
        self.clock_table()
            .into_iter()
            .flat_map(|(s, _)| s.clocks.into_iter().take(s.len()))
    }

    /// The clock table and where the records start in the block; None when this reader doesn't know the body, the
    /// body is coded other than [stored](BodyCodec::STORED) or the table doesn't parse.
    fn clock_table(&self) -> Option<(Slots, usize)> {
        if !self.has_records() {
            return None;
        }
        let (body, at) = self.plain_body()?;
        let (slots, table) = read_clock_table(body)?;
        Some((slots, at.checked_add(table)?))
    }

    /// The body as stored and where it starts in the block: None for a body coded other than stored.
    fn plain_body(&self) -> Option<(&'b [u8], usize)> {
        if !self.is_coded() {
            return Some((self.body, HEADER_LEN));
        }
        let c = body::coded(self.body)?;
        if c.codec != BodyCodec::STORED {
            return None;
        }
        // a stored body ends where the block's body ends
        Some((c.bytes, self.covered.len().checked_sub(c.bytes.len())?))
    }

    fn is_coded(&self) -> bool {
        self.flags & FLAG_BODY_CODED != 0
    }

    /// The parts of a coded records or describe body; None for any other block, or one that doesn't parse.
    fn coded(&self) -> Option<body::Coded<'b>> {
        let records = matches!(self.kind, BlockKind::Records | BlockKind::Describe);
        if !(self.is_coded() && records) {
            return None;
        }
        body::coded(self.body)
    }

    /// The codec of a coded records or describe body ([FLAG_BODY_CODED]); None for a plain body.
    pub fn body_codec(&self) -> Option<BodyCodec> {
        self.coded().map(|c| c.codec)
    }

    /// The block with its records readable in place: itself, unless its body is coded with lz4, which is decoded
    /// into `buf` as a plain block (the flag cleared, its length and CRC new; the block's place in a file is the
    /// coded one's).
    #[cfg(feature = "alloc")]
    pub fn decode<'d>(&self, buf: &'d mut alloc::vec::Vec<u8>) -> Result<Block<'d>, BlockError>
    where
        'b: 'd,
    {
        match self.coded() {
            Some(c) if c.codec == BodyCodec::LZ4 && self.body_known() => {
                body::decode_into(self, &c, buf)
            }
            _ => Ok(*self),
        }
    }

    /// Whether this reader knows the block's body: its version and kind, and for a coded body the codec. A block it
    /// doesn't know has no [records](Self::records) for it and is skipped.
    pub fn body_known(&self) -> bool {
        self.version == VERSION
            && !matches!(self.kind, BlockKind::Other(_) | BlockKind::Parity)
            && (!self.is_coded() || self.coded().is_some_and(|c| c.codec.is_known()))
    }

    /// The records, none for a chunk block or a block whose body this reader doesn't know ([Self::body_known]). A
    /// body coded with lz4 gives [BlockError::Coded]: read the records of [Self::decode].
    pub fn records(&self) -> Records<'b> {
        let (slots, body) = self
            .clock_table()
            .and_then(|(slots, at)| Some((slots, self.covered.get(at..)?)))
            .unwrap_or((Slots::NONE, &[]));
        Records {
            body,
            slots,
            failed: false,
            coded: self.has_records() && self.plain_body().is_none(),
        }
    }
}

/// Iterator over the records of a block, see [Block::records].
pub struct Records<'b> {
    body: &'b [u8],
    slots: Slots,
    failed: bool,
    /// The body is coded: one [BlockError::Coded] and no records.
    coded: bool,
}

impl<'b> Records<'b> {
    fn read_one(&mut self) -> Option<Record<'b>> {
        let mut rd = varint::Reader(self.body);
        let head = rd.uvar()?;
        // bits is at most 4 (16 slots): the shifts stay in range
        let bits = self.slots.bits();
        let track = u32::try_from(head >> bits).ok()?;
        let slot = usize::try_from(head & ((1u64 << bits).wrapping_sub(1))).ok()?;
        let at = if track == DESCRIBE_TRACK {
            if slot != 0 {
                return None;
            }
            None
        } else {
            if slot >= self.slots.len() {
                return None;
            }
            let clock = *self.slots.clocks.get(slot)?;
            let last = self.slots.last.get_mut(slot)?;
            let ticks = last.wrapping_add(rd.ivar()?);
            *last = ticks;
            Some(ClockTime::new(clock, ticks))
        };
        let len = rd.uvar_file()?.within(rd.0.len()).ok()?;
        let item = rd.take(len)?;
        self.body = rd.0;
        Some(Record { track, at, item })
    }
}

impl<'b> Iterator for Records<'b> {
    type Item = Result<Record<'b>, BlockError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.coded {
            self.coded = false;
            self.failed = true;
            return Some(Err(BlockError::Coded));
        }
        if self.body.is_empty() || self.failed {
            return None;
        }
        match self.read_one() {
            Some(r) => Some(Ok(r)),
            None => {
                self.failed = true;
                Some(Err(BlockError::BadRecords))
            }
        }
    }
}

#[cfg(test)]
mod tests;
