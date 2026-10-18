//! Tape blocks: the unit a producer of records hands over, a live consumer reads and a log file is made of.
//!
//! A tape is the recording unit whatever it is recorded on: blocks handed over live, appended to a file, replayed.
//!
//! A **record** is an item (any ShrinkWrap type, or plain bytes) on a **track**, with a time. Tracks are described
//! once by a [TrackDef]: a small number used in records, a stable key, the track's resource path in the producer's
//! API (`ww_trait` ids, array indices included) and the item type, so a reader knows from the API bundle what the
//! items are. Nothing here knows any producer: a link tap, a device log or a simulator all write the same blocks.
//!
//! A **block** is a header, records back to back and a CRC-32C (`CRC_32_ISCSI`):
//!
//! ```text
//! off size field
//!   0   4  magic "WWTP"
//!   4  u8  version, 1
//!   5  u8  kind: 1 data, 2 describe (BlockKind)
//!   6 u16  flags, 0 (a reader ignores bits it doesn't know)
//!   8 u32  len: the whole block, header and CRC included
//!  12 u32  records in the block
//!  16 u64  seq: the producer's block number, +1 per block; a gap is lost blocks
//!  24 u64  t_base: the producer's clock (ns) the record times are counted from
//!  32 ...  records: uvar track, ivar time delta (ns, against the previous record, the first against t_base),
//!          uvar item length, the item's bytes
//! len-4 u32 crc over bytes 0..len-4
//! ```
//!
//! The header and the CRC at the end are the same in every version, so any reader can check a block and skip it by its
//! length. A later version bumps the version byte and changes only the body (per-track chunks, an index,
//! compression, ..); new block kinds and flag bits are added the same way. A reader takes such a block as it is
//! ([Block::body_known] false, no records) and goes on with the next, instead of taking it for damage. [Describe] is an
//! evolvable enum, so a describe record of a variant a reader doesn't know reads as [Describe::Unknown] and is skipped.
//!
//! Numbers are little endian; `uvar` is LEB128 (7 bits per byte, low bits first, at most 10 bytes), `ivar` zigzag
//! then `uvar`, as in the logring format. Blocks are as long as their records, never padded: a file is blocks
//! appended back to back, a live feed sends one block per message.
//!
//! A **describe** block carries records on track 0 only, each a [Describe]: the [Producer], every [TrackDef] in use,
//! and the API bundles the items need ([Describe::Bundle]); after them, on their own tracks, the newest record of
//! every *sticky* track (state a reader joining late needs, e.g. a link's session). A producer writes one at the start
//! of a file, when a track is added, when a live consumer joins, after blocks were lost and every few seconds, so data
//! blocks carry only records and a trickle of records costs a block header each, not a description.
#![cfg_attr(not(feature = "std"), no_std)]

use shrink_wrap::prelude::*;

#[cfg(feature = "std")]
pub use ww_version::FullVersionOwned;
pub use ww_version::{FullVersion, Version};

mod varint;
pub use varint::{ivar_len, uvar_len};

#[cfg(feature = "std")]
mod file;
#[cfg(feature = "std")]
pub use file::{BlockBuf, Cut, FileBlocks, FileError, Recovery};
#[cfg(feature = "std")]
pub mod writer;
#[cfg(feature = "std")]
pub use writer::{SyncPolicy, TapeWriter, WriteError};

/// `WWTP`
pub const MAGIC: [u8; 4] = *b"WWTP";
pub const VERSION: u8 = 1;
/// Bytes of the block header.
pub const HEADER_LEN: usize = 32;
/// Longest block a reader takes (64 MiB): a longer length is damage. Blocks are small (a producer hands over 64 KiB
/// ones); the largest are describe blocks with big API bundles.
pub const MAX_BLOCK_LEN: usize = 64 << 20;
/// Bytes of the CRC at the end of a block.
pub const CRC_LEN: usize = 4;
/// Track 0: the producer's own, describe records.
pub const DESCRIBE_TRACK: u32 = 0;
/// Longest record head: track, time delta and length varints.
pub const MAX_RECORD_HEAD: usize = 5 + 10 + 5;

const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Data,
    Describe,
    /// A kind this reader doesn't know: its body isn't read.
    Other(u8),
}

impl BlockKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => BlockKind::Data,
            2 => BlockKind::Describe,
            other => BlockKind::Other(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            BlockKind::Data => 1,
            BlockKind::Describe => 2,
            BlockKind::Other(v) => v,
        }
    }
}

/// What a describe block says, one per record on track 0.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum Describe<'i> {
    Producer(Producer<'i>),
    Track(TrackDef<'i>),
    /// An API bundle (`ww_self` `ApiBundle` in its ShrinkWrap encoding) by its hash, the hash as the producer and
    /// [TrackDef::bundle] know it (wire_weaver's `ApiHash`). Written once per hash in a file; a live feed may leave it
    /// out and let the consumer look the hash up.
    Bundle {
        hash: RefVec<'i, u8>,
        bytes: RefVec<'i, u8>,
    },
}

/// Who writes the blocks.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Producer<'i> {
    /// What it is, for people (`ww link tap`).
    pub name: &'i str,
    /// Wall clock (ns since the Unix epoch) at the producer's clock time 0: block times plus this are wall time.
    pub wall_ns_at_zero: u64,
}

/// One track.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct TrackDef<'i> {
    /// Number in records, 1 and up (0 is [DESCRIBE_TRACK]); stays the same for the producer's life.
    pub number: UNib32,
    /// Stable key: the same track across files and producer runs, see [track_key].
    pub key: u64,
    /// The API the track's resource is in.
    pub api: FullVersion<'i>,
    /// Hash of that API's bundle, as in [Describe::Bundle]; empty when not known.
    pub bundle: RefVec<'i, u8>,
    /// Resource path of the track's stream in the API: `ww_trait` ids, array indices included.
    pub path: RefVec<'i, UNib32>,
    /// A name for people (`link[1].rx.frames`); readers go by `path`.
    pub label: &'i str,
    /// The newest record of this track is repeated in every describe block.
    pub sticky: bool,
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

/// The block does not fit: nothing was written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Full;

/// Writes one block into a buffer: [BlockWriter::push] records, then [BlockWriter::finish].
pub struct BlockWriter<'b> {
    buf: &'b mut [u8],
    pos: usize,
    records: u32,
    t_prev: u64,
}

impl<'b> BlockWriter<'b> {
    /// Start a block in `buf`, which must hold at least the header and the CRC; at most [MAX_BLOCK_LEN] of it is
    /// used.
    pub fn new(buf: &'b mut [u8], kind: BlockKind, seq: u64, t_base: u64) -> Self {
        assert!(buf.len() >= HEADER_LEN + CRC_LEN);
        let usable = buf.len().min(MAX_BLOCK_LEN);
        let buf = &mut buf[..usable];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4] = VERSION;
        buf[5] = kind.to_u8();
        buf[6..8].copy_from_slice(&0u16.to_le_bytes());
        buf[16..24].copy_from_slice(&seq.to_le_bytes());
        buf[24..32].copy_from_slice(&t_base.to_le_bytes());
        BlockWriter {
            buf,
            pos: HEADER_LEN,
            records: 0,
            t_prev: t_base,
        }
    }

    /// Release the buffer, keeping what is needed to go on with [Self::from_state] on the same buffer.
    pub fn into_state(self) -> BlockState {
        BlockState {
            pos: self.pos,
            records: self.records,
            t_prev: self.t_prev,
        }
    }

    /// Go on writing a block started in `buf` (the same bytes, unchanged since [Self::into_state]).
    pub fn from_state(buf: &'b mut [u8], state: BlockState) -> Self {
        let usable = buf.len().min(MAX_BLOCK_LEN);
        BlockWriter {
            buf: &mut buf[..usable],
            pos: state.pos,
            records: state.records,
            t_prev: state.t_prev,
        }
    }

    /// Bytes of records written so far.
    pub fn used(&self) -> usize {
        self.pos - HEADER_LEN
    }

    pub fn records(&self) -> u32 {
        self.records
    }

    /// Free bytes for records, the CRC left aside.
    pub fn free(&self) -> usize {
        self.buf.len() - CRC_LEN - self.pos
    }

    /// Add a record whose item is `parts` back to back.
    pub fn push(&mut self, track: u32, t: u64, parts: &[&[u8]]) -> Result<(), Full> {
        let len: usize = parts.iter().map(|p| p.len()).sum();
        let dt = t.wrapping_sub(self.t_prev) as i64;
        let head = uvar_len(track as u64) + ivar_len(dt) + uvar_len(len as u64);
        if head + len > self.free() {
            return Err(Full);
        }
        let mut pos = self.pos;
        pos += varint::put_uvar(&mut self.buf[pos..], track as u64);
        pos += varint::put_ivar(&mut self.buf[pos..], dt);
        pos += varint::put_uvar(&mut self.buf[pos..], len as u64);
        for p in parts {
            self.buf[pos..pos + p.len()].copy_from_slice(p);
            pos += p.len();
        }
        self.pos = pos;
        self.records += 1;
        self.t_prev = t;
        Ok(())
    }

    /// Add a record whose item `encode` writes into the free space (at most `max_len` bytes) and returns the length
    /// of, so a ShrinkWrap value is serialized in place without a copy. `encode` returning None (did not fit)
    /// leaves the block as it was.
    pub fn push_with(
        &mut self,
        track: u32,
        t: u64,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<(), Full> {
        let dt = t.wrapping_sub(self.t_prev) as i64;
        let fixed = uvar_len(track as u64) + ivar_len(dt);
        let free = self.free();
        if fixed + 1 > free {
            return Err(Full);
        }
        // the item goes after the longest length the free space can need, then moves down to its real place
        let max_len_len = uvar_len((free - fixed) as u64);
        let item_at = self.pos + fixed + max_len_len;
        let end = self.buf.len() - CRC_LEN;
        let len = encode(&mut self.buf[item_at..end]).ok_or(Full)?;
        let len_len = uvar_len(len as u64);
        let mut pos = self.pos;
        pos += varint::put_uvar(&mut self.buf[pos..], track as u64);
        pos += varint::put_ivar(&mut self.buf[pos..], dt);
        let real_item_at = pos + len_len;
        self.buf.copy_within(item_at..item_at + len, real_item_at);
        varint::put_uvar(&mut self.buf[pos..], len as u64);
        self.pos = real_item_at + len;
        self.records += 1;
        self.t_prev = t;
        Ok(())
    }

    /// Write the header's length and count and the CRC; returns the block's length (`&buf[..len]` is the block).
    pub fn finish(self) -> usize {
        let len = self.pos + CRC_LEN;
        self.buf[8..12].copy_from_slice(&(len as u32).to_le_bytes());
        self.buf[12..16].copy_from_slice(&self.records.to_le_bytes());
        let crc = CRC.checksum(&self.buf[..self.pos]);
        self.buf[self.pos..len].copy_from_slice(&crc.to_le_bytes());
        len
    }
}

/// A [BlockWriter] without its buffer, see [BlockWriter::into_state].
#[derive(Copy, Clone, Debug)]
pub struct BlockState {
    pos: usize,
    records: u32,
    t_prev: u64,
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
    /// The records don't parse up to the end, or there are more or fewer than the header says.
    BadRecords,
}

/// A block read from bytes, CRC checked. Records are read in place, see [Block::records].
#[derive(Copy, Clone, Debug)]
pub struct Block<'b> {
    /// The format version it was written with, [VERSION] or a later one.
    pub version: u8,
    pub kind: BlockKind,
    pub seq: u64,
    pub t_base: u64,
    pub record_count: u32,
    bytes: &'b [u8],
}

/// A record of a [Block], borrowed from it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Record<'b> {
    pub track: u32,
    /// Producer clock, ns.
    pub t: u64,
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
    let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if (len as usize) < HEADER_LEN + CRC_LEN || len as usize > MAX_BLOCK_LEN {
        return Err(BlockError::BadLength(len));
    }
    Ok(len as usize)
}

impl<'b> Block<'b> {
    /// Read the block at the start of `bytes`; bytes after it are not looked at ([Block::len] says where it ends).
    pub fn read(bytes: &'b [u8]) -> Result<Self, BlockError> {
        let len = block_len(bytes)?;
        if bytes.len() < len {
            return Err(BlockError::Short { needed: len });
        }
        let kind = BlockKind::from_u8(bytes[5]);
        let crc = u32::from_le_bytes(bytes[len - CRC_LEN..len].try_into().unwrap());
        if CRC.checksum(&bytes[..len - CRC_LEN]) != crc {
            return Err(BlockError::BadCrc);
        }
        let block = Block {
            version: bytes[4],
            kind,
            record_count: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            seq: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            t_base: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            bytes: &bytes[..len],
        };
        if !block.body_known() {
            return Ok(block);
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

    /// Whether this reader knows the block's body: its version and kind. A block it doesn't know has no
    /// [records](Self::records) for it and is skipped.
    pub fn body_known(&self) -> bool {
        self.version == VERSION && !matches!(self.kind, BlockKind::Other(_))
    }

    /// The records, none for a block whose body this reader doesn't know ([Self::body_known]).
    pub fn records(&self) -> Records<'b> {
        let body = if self.body_known() {
            &self.bytes[HEADER_LEN..self.bytes.len() - CRC_LEN]
        } else {
            &[]
        };
        Records {
            body,
            t: self.t_base,
            failed: false,
        }
    }
}

/// Iterator over the records of a block, see [Block::records].
pub struct Records<'b> {
    body: &'b [u8],
    t: u64,
    failed: bool,
}

impl<'b> Iterator for Records<'b> {
    type Item = Result<Record<'b>, BlockError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.body.is_empty() || self.failed {
            return None;
        }
        let mut rd = varint::Reader(self.body);
        let r = (|| {
            let track = u32::try_from(rd.uvar()?).ok()?;
            let dt = rd.ivar()?;
            let len = usize::try_from(rd.uvar()?).ok()?;
            let item = rd.take(len)?;
            Some((track, dt, item))
        })();
        match r {
            Some((track, dt, item)) => {
                self.t = self.t.wrapping_add(dt as u64);
                self.body = rd.0;
                Some(Ok(Record {
                    track,
                    t: self.t,
                    item,
                }))
            }
            None => {
                self.failed = true;
                Some(Err(BlockError::BadRecords))
            }
        }
    }
}

#[cfg(test)]
mod tests;
