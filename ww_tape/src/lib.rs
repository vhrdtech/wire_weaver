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
//!   6 u16  flags, 0 (a reader ignores bits it doesn't know)
//!   8 u32  len: the whole block, header and CRC included
//!  12 u32  records in the block
//!  16 u64  seq: the producer's block number, +1 per block; a gap is lost blocks
//!  24 u64  reserved, 0
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
//! So time deltas are coded per clock, a block reads alone (its first time per clock is absolute) and one clock costs
//! no bit in the head. No wall clock is in a block: wall time is a clock of its own, mapped to the others.
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
//! The same input gives the same bytes: blocks hold no wall time, describe records go in canonical order
//! ([DescribeSet] with `std`) and blocks are cut by size, not by time, unless the producer seals them on a timer.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub use ww_version::FullVersionOwned;
pub use ww_version::{FullVersion, Version};

mod describe;
pub use describe::{
    ClockDef, ClockMap, ClockOrigin, Describe, Dimension, MapSegment, NO_CLOCK, Producer, Rank,
    TrackDef, TypeRef, TypedItem, UNIX_CLOCK_KEY, clock_key, track_id,
};
#[cfg(feature = "std")]
pub use describe::{
    ClockDefOwned, ClockMapOwned, DescribeOwned, DimensionOwned, ProducerOwned, TrackDefOwned,
    TypeRefOwned, TypedItemOwned,
};
pub use exact_units::Magnitude;

mod varint;
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
pub use writer::{SyncPolicy, TapeWriter, WriteError};

/// `WWTP`
pub const MAGIC: [u8; 4] = *b"WWTP";
pub const VERSION: u8 = 2;
/// Bytes of the block header.
pub const HEADER_LEN: usize = 32;
/// Longest block a reader takes (64 MiB): a longer length is damage. Blocks are small (a producer hands over 64 KiB
/// ones); the largest are describe blocks with big API bundles.
pub const MAX_BLOCK_LEN: usize = 64 << 20;
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
    /// A kind this reader doesn't know: its body isn't read.
    Other(u8),
}

impl BlockKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => BlockKind::Records,
            2 => BlockKind::Describe,
            other => BlockKind::Other(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            BlockKind::Records => 1,
            BlockKind::Describe => 2,
            BlockKind::Other(v) => v,
        }
    }
}

/// FNV-1a, 64 bit.
pub const fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
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
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
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

    fn clocks(&self) -> &[u32] {
        &self.clocks[..self.n as usize]
    }

    fn bits(&self) -> u32 {
        slot_bits(self.n as usize)
    }

    fn slot(&self, clock: u32) -> Option<usize> {
        self.clocks().binary_search(&clock).ok()
    }
}

/// The block does not fit: nothing was written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Full;

/// Writes one block into a buffer: [BlockWriter::push] records, then [BlockWriter::finish].
pub struct BlockWriter<'b> {
    buf: &'b mut [u8],
    state: BlockState,
}

impl<'b> BlockWriter<'b> {
    /// Start a block in `buf` holding records on `clocks` (strictly ascending clock numbers, none of them
    /// [NO_CLOCK]; at most [MAX_BLOCK_CLOCKS]); at most [MAX_BLOCK_LEN] of `buf` is used.
    ///
    /// # Panics
    ///
    /// When `clocks` breaks those rules or `buf` doesn't hold the header, the clock table and the CRC.
    pub fn new(buf: &'b mut [u8], kind: BlockKind, seq: u64, clocks: &[u32]) -> Self {
        assert!(
            clocks.len() <= MAX_BLOCK_CLOCKS,
            "at most {MAX_BLOCK_CLOCKS} clocks in a block"
        );
        assert!(
            clocks.windows(2).all(|w| w[0] < w[1]),
            "block clocks strictly ascending"
        );
        assert!(!clocks.contains(&NO_CLOCK), "clock 0 is no clock");
        let table: usize = uvar_len(clocks.len() as u64)
            + clocks
                .iter()
                .map(|c| uvar_len(u64::from(*c)))
                .sum::<usize>();
        assert!(
            buf.len() >= HEADER_LEN + table + CRC_LEN,
            "block buffer holds header, clock table and CRC"
        );
        let usable = buf.len().min(MAX_BLOCK_LEN);
        let buf = &mut buf[..usable];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4] = VERSION;
        buf[5] = kind.to_u8();
        buf[6..8].copy_from_slice(&0u16.to_le_bytes());
        buf[16..24].copy_from_slice(&seq.to_le_bytes());
        buf[24..32].copy_from_slice(&0u64.to_le_bytes());
        let mut pos = HEADER_LEN;
        pos += varint::put_uvar(&mut buf[pos..], clocks.len() as u64);
        let mut slots = Slots {
            clocks: [0; MAX_BLOCK_CLOCKS],
            last: [0; MAX_BLOCK_CLOCKS],
            n: clocks.len() as u8,
        };
        for (i, c) in clocks.iter().enumerate() {
            pos += varint::put_uvar(&mut buf[pos..], u64::from(*c));
            slots.clocks[i] = *c;
        }
        BlockWriter {
            buf,
            state: BlockState {
                pos,
                body: pos,
                records: 0,
                slots,
            },
        }
    }

    /// Release the buffer, keeping what is needed to go on with [Self::from_state] on the same buffer.
    pub fn into_state(self) -> BlockState {
        self.state
    }

    /// Go on writing a block started in `buf` (the same bytes, unchanged since [Self::into_state]).
    pub fn from_state(buf: &'b mut [u8], state: BlockState) -> Self {
        let usable = buf.len().min(MAX_BLOCK_LEN);
        BlockWriter {
            buf: &mut buf[..usable],
            state,
        }
    }

    /// Bytes of records written so far.
    pub fn used(&self) -> usize {
        self.state.pos - self.state.body
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
        self.buf.len() - CRC_LEN - self.state.pos
    }

    /// Add a record on `track` at `at` whose item is `parts` back to back.
    ///
    /// # Panics
    ///
    /// When `track` is [DESCRIBE_TRACK] (see [Self::push_describe]) or `at`'s clock isn't one of the block's.
    pub fn push(&mut self, track: u32, at: ClockTime, parts: &[&[u8]]) -> Result<(), Full> {
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
    ) -> Result<(), Full> {
        assert!(track != DESCRIBE_TRACK, "describe records have no time");
        let slots = &self.state.slots;
        let slot = slots
            .slot(at.clock)
            .expect("the record's clock is one the block was started with");
        let head = (u64::from(track) << slots.bits()) | slot as u64;
        let dt = at.ticks.wrapping_sub(slots.last[slot]);
        self.put(head, Some(dt), encode)?;
        self.state.slots.last[slot] = at.ticks;
        Ok(())
    }

    /// [Self::push_describe] with an item `encode` writes in place, as [Self::push_with].
    pub fn push_describe_with(
        &mut self,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), Full> {
        self.put(0, None, encode)
    }

    fn put(
        &mut self,
        head: u64,
        dt: Option<i64>,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), Full> {
        let fixed = uvar_len(head) + dt.map_or(0, ivar_len);
        let free = self.free();
        if fixed + 1 > free {
            return Err(Full);
        }
        // the item goes after the longest length the free space can need, then moves down to its real place
        let max_len_len = uvar_len((free - fixed) as u64);
        let item_at = self.state.pos + fixed + max_len_len;
        let end = self.buf.len() - CRC_LEN;
        let len = encode(&mut self.buf[item_at..end]).ok_or(Full)?;
        let len_len = uvar_len(len as u64);
        let mut pos = self.state.pos;
        pos += varint::put_uvar(&mut self.buf[pos..], head);
        if let Some(dt) = dt {
            pos += varint::put_ivar(&mut self.buf[pos..], dt);
        }
        let real_item_at = pos + len_len;
        self.buf.copy_within(item_at..item_at + len, real_item_at);
        varint::put_uvar(&mut self.buf[pos..], len as u64);
        self.state.pos = real_item_at + len;
        self.state.records += 1;
        Ok(())
    }

    /// Write the header's length and count and the CRC; returns the block's length (`&buf[..len]` is the block).
    pub fn finish(self) -> usize {
        let pos = self.state.pos;
        let len = pos + CRC_LEN;
        self.buf[8..12].copy_from_slice(&(len as u32).to_le_bytes());
        self.buf[12..16].copy_from_slice(&self.state.records.to_le_bytes());
        let crc = CRC.checksum(&self.buf[..pos]);
        self.buf[pos..len].copy_from_slice(&crc.to_le_bytes());
        len
    }
}

fn copy_parts(buf: &mut [u8], parts: &[&[u8]]) -> Option<usize> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    if len > buf.len() {
        return None;
    }
    let mut at = 0;
    for p in parts {
        buf[at..at + p.len()].copy_from_slice(p);
        at += p.len();
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

/// Why a block can't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockError {
    /// Fewer bytes than the header or its length says.
    Short {
        needed: usize,
    },
    BadMagic,
    BadLength(u32),
    BadCrc,
    /// The clock table or the records don't parse up to the end, or there are more or fewer than the header says.
    BadRecords,
}

/// A block read from bytes, CRC checked. Records are read in place, see [Block::records].
#[derive(Copy, Clone, Debug)]
pub struct Block<'b> {
    /// The format version it was written with, [VERSION] or a later one.
    pub version: u8,
    pub kind: BlockKind,
    pub seq: u64,
    pub record_count: u32,
    bytes: &'b [u8],
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
    if bytes.len() < HEADER_LEN {
        return Err(BlockError::Short { needed: HEADER_LEN });
    }
    if bytes[0..4] != MAGIC {
        return Err(BlockError::BadMagic);
    }
    let len = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
    if (len as usize) < HEADER_LEN + CRC_LEN || len as usize > MAX_BLOCK_LEN {
        return Err(BlockError::BadLength(len));
    }
    Ok(len as usize)
}

/// The clock table at the start of a body: the slots and where the records start in `body`.
fn read_clock_table(body: &[u8]) -> Option<(Slots, usize)> {
    let mut rd = varint::Reader(body);
    let n = usize::try_from(rd.uvar()?).ok()?;
    if n > MAX_BLOCK_CLOCKS {
        return None;
    }
    let mut slots = Slots {
        clocks: [0; MAX_BLOCK_CLOCKS],
        last: [0; MAX_BLOCK_CLOCKS],
        n: n as u8,
    };
    for i in 0..n {
        let c = u32::try_from(rd.uvar()?).ok()?;
        if c == NO_CLOCK || (i > 0 && c <= slots.clocks[i - 1]) {
            return None;
        }
        slots.clocks[i] = c;
    }
    Some((slots, body.len() - rd.0.len()))
}

impl<'b> Block<'b> {
    /// Read the block at the start of `bytes`; bytes after it are not looked at ([Block::len] says where it ends).
    pub fn read(bytes: &'b [u8]) -> Result<Self, BlockError> {
        let len = block_len(bytes)?;
        if bytes.len() < len {
            return Err(BlockError::Short { needed: len });
        }
        let crc = u32::from_le_bytes(bytes[len - CRC_LEN..len].try_into().expect("4 bytes"));
        if CRC.checksum(&bytes[..len - CRC_LEN]) != crc {
            return Err(BlockError::BadCrc);
        }
        let block = Block {
            version: bytes[4],
            kind: BlockKind::from_u8(bytes[5]),
            record_count: u32::from_le_bytes(bytes[12..16].try_into().expect("4 bytes")),
            seq: u64::from_le_bytes(bytes[16..24].try_into().expect("8 bytes")),
            bytes: &bytes[..len],
        };
        if !block.body_known() {
            return Ok(block);
        }
        if block.clock_table().is_none() {
            return Err(BlockError::BadRecords);
        }
        let mut n = 0u32;
        for r in block.records() {
            r?;
            n += 1;
        }
        if n != block.record_count {
            return Err(BlockError::BadRecords);
        }
        Ok(block)
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
        let slots = self.clock_table().map(|(s, _)| s);
        (0..slots.map_or(0, |s| s.n as usize)).filter_map(move |i| Some(slots?.clocks[i]))
    }

    /// The clock table and where the records start in the block; None when this reader doesn't know the body or
    /// the table doesn't parse.
    fn clock_table(&self) -> Option<(Slots, usize)> {
        if !self.body_known() {
            return None;
        }
        let (slots, at) = read_clock_table(&self.bytes[HEADER_LEN..self.bytes.len() - CRC_LEN])?;
        Some((slots, HEADER_LEN + at))
    }

    /// Whether this reader knows the block's body: its version and kind. A block it doesn't know has no
    /// [records](Self::records) for it and is skipped.
    pub fn body_known(&self) -> bool {
        self.version == VERSION && !matches!(self.kind, BlockKind::Other(_))
    }

    /// The records, none for a block whose body this reader doesn't know ([Self::body_known]).
    pub fn records(&self) -> Records<'b> {
        let (slots, body) = match self.clock_table() {
            Some((slots, at)) => (slots, &self.bytes[at..self.bytes.len() - CRC_LEN]),
            None => (Slots::NONE, &[][..]),
        };
        Records {
            body,
            slots,
            failed: false,
        }
    }
}

/// Iterator over the records of a block, see [Block::records].
pub struct Records<'b> {
    body: &'b [u8],
    slots: Slots,
    failed: bool,
}

impl<'b> Records<'b> {
    fn read_one(&mut self) -> Option<Record<'b>> {
        let mut rd = varint::Reader(self.body);
        let head = rd.uvar()?;
        let bits = self.slots.bits();
        let track = u32::try_from(head >> bits).ok()?;
        let slot = (head & ((1u64 << bits) - 1)) as usize;
        let at = if track == DESCRIBE_TRACK {
            if slot != 0 {
                return None;
            }
            None
        } else {
            if slot >= self.slots.n as usize {
                return None;
            }
            let ticks = self.slots.last[slot].wrapping_add(rd.ivar()?);
            self.slots.last[slot] = ticks;
            Some(ClockTime::new(self.slots.clocks[slot], ticks))
        };
        let len = usize::try_from(rd.uvar()?).ok()?;
        let item = rd.take(len)?;
        self.body = rd.0;
        Some(Record { track, at, item })
    }
}

impl<'b> Iterator for Records<'b> {
    type Item = Result<Record<'b>, BlockError>;

    fn next(&mut self) -> Option<Self::Item> {
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
