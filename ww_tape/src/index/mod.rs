//! Index blocks: where the blocks of a tape piece are, without a scan.
//!
//! An index block ([BlockKind::Index]) is a checkpoint: it lists every block placed since the previous index block
//! of the piece, chunk, summary, record and describe blocks alike, and points back at that previous one, so the
//! chain from the newest checkpoint reaches every block. A writer places one every few MiB ([IndexPolicy]); after a
//! crash a reader finds the newest one from the end of the file and scans only the stretch after it. At the finish
//! a **final** index lists every block of the piece again, sorted by track, with a directory of tracks in its head:
//! a reader of one track out of thousands reads the head and that track's entries, each list checked by its own CRC.
//!
//! Offsets are counted from the start of the piece (its [piece head](crate::piece), or where its writer started), so
//! a piece keeps its index when pieces are concatenated. The block header's count is the number of entries; bytes
//! 28..32 hold a CRC of the head, and the body starts with where the entries start, as in a chunk block:
//!
//! ```text
//! off size field
//!  24 u32  run tag, as in every block
//!  28 u32  head CRC: CRC-32C over bytes 0..28, then 32..entries offset
//!  32 u32  entries offset from the block start
//!  36 u32  tracks n in the directory, 0 in a checkpoint
//!  40 u64  self offset: where this block starts in the piece
//!  48 u64  previous index block of the piece (a checkpoint's previous checkpoint, a final block's previous final
//!          block), u64::MAX for none
//!  56 u16  flags: bit 0 final (entries sorted by track id, then offset; a directory of tracks follows)
//!  58 u16  entry length, 72 (a reader takes longer entries and reads their first 72 bytes)
//!  60 ...  n × track: id u64, first entry u32, entry count u32, CRC-32C of its entries u32, reserved u32
//!  ...     entries, each:
//!            0 u64 offset of the block in the piece    8 u32 its length        12 u8 its block kind
//!           13 u8  summary level (0 for other kinds)   14 u8 summary tree      15 u8 reserved
//!           16 u64 track id (0: none)                 24 u32 track number      28 u32 reserved
//!           32 u64 first item number                  40 u64 items (samples, records)
//!           48 i64 first tick                         56 i64 last tick         64 i64 latest end
//! len-4 u32 CRC over bytes 0..len-4, as in every block
//! ```
//!
//! Record and describe blocks have no track id, item numbers or ticks (their records may be on many tracks and
//! clocks); their entry says where they are and how many records they hold. The final index lists them under track
//! id 0. A final index longer than [INDEX_LEN_MAX] is several final blocks, chained like checkpoints, each holding
//! whole tracks in ascending track id order.

use crate::chunk::{get_u16, get_u32, get_u64, put_u32};
use crate::{
    Block, BlockError, BlockId, BlockKind, CRC, CRC_LEN, ChunkHead, HEADER_LEN, VERSION, put_header,
};

/// Bytes of one index entry.
pub const ENTRY_LEN: usize = 72;
/// Bytes of one track in a final index's directory.
pub const TRACK_LEN: usize = 24;
/// Bytes of the fixed part of an index head (32..60).
pub const INDEX_FIXED_LEN: usize = 28;
/// Bytes at the start of an index block that say how long its head is ([IndexBlock::head_len]): the header and the
/// entries offset.
pub const HEAD_LEN_PREFIX: usize = HEADER_LEN + 4;
/// Longest index block a writer makes (1 MiB) and a reader scanning back from the end of a file looks for.
pub const INDEX_LEN_MAX: usize = 1 << 20;
/// Most entries in a checkpoint index block.
pub const CHECKPOINT_ENTRIES_MAX: usize =
    (INDEX_LEN_MAX - HEADER_LEN - INDEX_FIXED_LEN - CRC_LEN) / ENTRY_LEN;
/// Offset that means none.
pub const NONE: u64 = u64::MAX;

const FLAG_FINAL: u16 = 1;

/// Whether blocks of `kind` belong to the file structure (index, finish, tail, piece head): they carry the seq of
/// the last counted block before them and take no part in the seq order, so seq stays the producer's count of its
/// own blocks. A reader checks neither order nor gaps on them. Describe blocks a writer repeats are stamped the
/// same way.
pub fn is_structural(kind: BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::Index | BlockKind::Finish | BlockKind::Tail | BlockKind::PieceHead
    )
}

/// Where a block is in its piece and what it holds.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// Where the block starts, from the start of the piece.
    pub offset: u64,
    /// The whole block's length.
    pub len: u32,
    pub kind: BlockKind,
    /// A summary block's level, else 0.
    pub level: u8,
    /// A summary block's tree (1 events, 2 series), else 0.
    pub tree: u8,
    /// The track's identity hash ([TrackDef::id](crate::TrackDef::id)); 0 for record and describe blocks.
    pub track_id: u64,
    pub track_number: u32,
    /// First item number (sample index) the block holds or covers.
    pub first_item: u64,
    /// Items (samples) the block holds or covers; records of a record or describe block.
    pub count: u64,
    pub first_tick: i64,
    pub last_tick: i64,
    pub latest_end: i64,
}

impl IndexEntry {
    /// The entry of the good block `bytes` at `offset` in its piece; None for blocks an index doesn't list (index,
    /// finish, tail, piece head and parity blocks).
    pub fn of_block(offset: u64, block: &Block<'_>) -> Option<Self> {
        let bytes = block.bytes();
        let mut e = IndexEntry {
            offset,
            len: bytes.len() as u32,
            kind: block.kind,
            level: 0,
            tree: 0,
            track_id: 0,
            track_number: 0,
            first_item: 0,
            count: u64::from(block.record_count),
            first_tick: 0,
            last_tick: 0,
            latest_end: 0,
        };
        match block.kind {
            BlockKind::Index
            | BlockKind::Finish
            | BlockKind::Tail
            | BlockKind::PieceHead
            | BlockKind::Parity => return None,
            BlockKind::Events | BlockKind::Series => {
                if let Some(c) = block.chunk() {
                    return Some(Self::of_chunk(offset, bytes.len(), &c.head()));
                }
            }
            BlockKind::Summary => {
                if let Some(s) = block.summary() {
                    let h = s.head();
                    e.track_id = h.track_id;
                    e.track_number = h.track_number;
                    e.tree = h.tree.to_u8();
                    e.level = h.level;
                    e.first_item = h.first_item;
                    e.count = h.items;
                    e.first_tick = h.first_tick;
                    e.last_tick = h.last_tick;
                    e.latest_end = h.latest_end;
                }
            }
            _ => {}
        }
        Some(e)
    }

    /// The entry of a chunk block of `len` bytes at `offset` with `head`.
    pub fn of_chunk(offset: u64, len: usize, head: &ChunkHead) -> Self {
        IndexEntry {
            offset,
            len: len as u32,
            kind: head.kind,
            level: 0,
            tree: 0,
            track_id: head.track_id,
            track_number: head.track_number,
            first_item: head.first_item,
            count: u64::from(head.count),
            first_tick: head.first_tick,
            last_tick: head.last_tick,
            latest_end: head.latest_end,
        }
    }

    /// Item numbers past the last one the block holds.
    pub fn end_item(&self) -> u64 {
        self.first_item.saturating_add(self.count)
    }

    fn encode(&self, out: &mut [u8]) {
        out[..ENTRY_LEN].fill(0);
        out[0..8].copy_from_slice(&self.offset.to_le_bytes());
        put_u32(out, 8, self.len);
        out[12] = self.kind.to_u8();
        out[13] = self.level;
        out[14] = self.tree;
        out[16..24].copy_from_slice(&self.track_id.to_le_bytes());
        put_u32(out, 24, self.track_number);
        out[32..40].copy_from_slice(&self.first_item.to_le_bytes());
        out[40..48].copy_from_slice(&self.count.to_le_bytes());
        out[48..56].copy_from_slice(&self.first_tick.to_le_bytes());
        out[56..64].copy_from_slice(&self.last_tick.to_le_bytes());
        out[64..72].copy_from_slice(&self.latest_end.to_le_bytes());
    }

    /// Read an entry from its first [ENTRY_LEN] bytes.
    ///
    /// # Panics
    ///
    /// When `b` is shorter than [ENTRY_LEN].
    pub fn decode(b: &[u8]) -> Self {
        assert!(b.len() >= ENTRY_LEN, "an entry is {ENTRY_LEN} bytes");
        IndexEntry {
            offset: get_u64(b, 0),
            len: get_u32(b, 8),
            kind: BlockKind::from_u8(b[12]),
            level: b[13],
            tree: b[14],
            track_id: get_u64(b, 16),
            track_number: get_u32(b, 24),
            first_item: get_u64(b, 32),
            count: get_u64(b, 40),
            first_tick: get_u64(b, 48) as i64,
            last_tick: get_u64(b, 56) as i64,
            latest_end: get_u64(b, 64) as i64,
        }
    }
}

/// A checkpoint or a final index.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IndexKind {
    /// The blocks since the previous checkpoint, in file order.
    Checkpoint,
    /// Blocks sorted by track id, then offset, with a directory of tracks.
    Final,
}

/// The fixed part of an index head.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IndexHead {
    pub kind: IndexKind,
    /// Where this block starts in the piece.
    pub self_offset: u64,
    /// The previous index block of the same kind in the piece.
    pub prev: Option<u64>,
}

/// One track of a final index's directory.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TrackList {
    pub track_id: u64,
    /// Index of its first entry in the block.
    pub first: u32,
    pub count: u32,
    /// CRC-32C of its entries' bytes.
    pub crc: u32,
}

/// Why an index, finish, tail or piece head block can't be read or written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IndexError {
    Block(BlockError),
    /// The block is of another kind or version.
    Kind,
    /// The head CRC doesn't match.
    BadHeadCrc,
    /// The head or the body is out of range.
    BadHead,
    /// A track's entries don't match their CRC.
    BadEntries,
    /// The buffer is shorter than the block.
    Full {
        needed: usize,
    },
    /// The entries break a rule of the format (a final index not sorted by track, too many entries).
    Invalid(&'static str),
}

impl From<BlockError> for IndexError {
    fn from(e: BlockError) -> Self {
        IndexError::Block(e)
    }
}

/// Tracks a final index over `entries` (sorted by track id) has.
pub fn track_count(entries: &[IndexEntry]) -> usize {
    entries
        .iter()
        .enumerate()
        .filter(|(i, e)| *i == 0 || entries[i - 1].track_id != e.track_id)
        .count()
}

/// Bytes of an index block with `entries` and `tracks` directory tracks.
pub const fn index_len(entries: usize, tracks: usize) -> usize {
    HEADER_LEN + INDEX_FIXED_LEN + tracks * TRACK_LEN + entries * ENTRY_LEN + CRC_LEN
}

/// Write the CRC of the block `buf` (exactly its bytes).
pub(crate) fn seal(buf: &mut [u8]) {
    let len = buf.len();
    let crc = CRC.checksum(&buf[..len - CRC_LEN]);
    buf[len - CRC_LEN..].copy_from_slice(&crc.to_le_bytes());
}

/// Rewrite the head CRC of the good index block `block` after its header changed (its seq).
pub(crate) fn restamp_head(block: &mut [u8]) {
    if let Ok(at) = IndexBlock::head_len(block) {
        let crc = head_crc(block, at);
        put_u32(block, 28, crc);
    }
}

fn head_crc(b: &[u8], entries_at: usize) -> u32 {
    let mut d = CRC.digest();
    d.update(&b[..28]);
    d.update(&b[32..entries_at]);
    d.finalize()
}

/// Write an index block listing `entries` into `buf`; returns its length ([index_len]). A final index's entries
/// must be sorted by track id, then offset.
pub fn write_index(
    buf: &mut [u8],
    id: BlockId,
    head: &IndexHead,
    entries: &[IndexEntry],
) -> Result<usize, IndexError> {
    let tracks = match head.kind {
        IndexKind::Checkpoint => 0,
        IndexKind::Final => {
            if entries
                .windows(2)
                .any(|w| (w[0].track_id, w[0].offset) >= (w[1].track_id, w[1].offset))
            {
                return Err(IndexError::Invalid(
                    "final index not sorted by track and offset",
                ));
            }
            track_count(entries)
        }
    };
    let len = index_len(entries.len(), tracks);
    if len > crate::MAX_BLOCK_LEN || u32::try_from(entries.len()).is_err() {
        return Err(IndexError::Invalid("index longer than a block"));
    }
    if buf.len() < len {
        return Err(IndexError::Full { needed: len });
    }
    let buf = &mut buf[..len];
    put_header(buf, BlockKind::Index, len, entries.len() as u32, id);
    let entries_at = HEADER_LEN + INDEX_FIXED_LEN + tracks * TRACK_LEN;
    put_u32(buf, 32, entries_at as u32);
    put_u32(buf, 36, tracks as u32);
    buf[40..48].copy_from_slice(&head.self_offset.to_le_bytes());
    buf[48..56].copy_from_slice(&head.prev.unwrap_or(NONE).to_le_bytes());
    let flags = match head.kind {
        IndexKind::Checkpoint => 0,
        IndexKind::Final => FLAG_FINAL,
    };
    buf[56..58].copy_from_slice(&flags.to_le_bytes());
    buf[58..60].copy_from_slice(&(ENTRY_LEN as u16).to_le_bytes());
    for (i, e) in entries.iter().enumerate() {
        let at = entries_at + i * ENTRY_LEN;
        e.encode(&mut buf[at..at + ENTRY_LEN]);
    }
    let mut t = 0;
    let mut i = 0;
    while tracks > 0 && i < entries.len() {
        let id = entries[i].track_id;
        let n = entries[i..].iter().take_while(|e| e.track_id == id).count();
        let from = entries_at + i * ENTRY_LEN;
        let crc = CRC.checksum(&buf[from..from + n * ENTRY_LEN]);
        let at = HEADER_LEN + INDEX_FIXED_LEN + t * TRACK_LEN;
        buf[at..at + 8].copy_from_slice(&id.to_le_bytes());
        put_u32(buf, at + 8, i as u32);
        put_u32(buf, at + 12, n as u32);
        put_u32(buf, at + 16, crc);
        put_u32(buf, at + 20, 0);
        t += 1;
        i += n;
    }
    let hc = head_crc(buf, entries_at);
    put_u32(buf, 28, hc);
    seal(buf);
    Ok(len)
}

/// An index block read in place: from a whole block ([Self::read]), or from its head alone ([Self::read_head]) for
/// a reader that fetches only the entries of one track.
#[derive(Copy, Clone, Debug)]
pub struct IndexBlock<'b> {
    /// The block's bytes, or its head's alone
    bytes: &'b [u8],
    len: usize,
    entries_at: usize,
    entry_len: usize,
    entries: usize,
    tracks: usize,
    kind: IndexKind,
}

impl<'b> IndexBlock<'b> {
    /// The index of a good block (CRC checked).
    pub fn read(block: &Block<'b>) -> Result<Self, IndexError> {
        if block.kind != BlockKind::Index || !block.body_known() {
            return Err(IndexError::Kind);
        }
        let ix = Self::read_head(block.bytes())?;
        Ok(IndexBlock {
            bytes: block.bytes(),
            ..ix
        })
    }

    /// Bytes of the head of the index block whose first [HEAD_LEN_PREFIX] bytes are `header`: what [Self::read_head]
    /// needs.
    pub fn head_len(header: &[u8]) -> Result<usize, IndexError> {
        let len = crate::block_len(header)?;
        if header.len() < HEAD_LEN_PREFIX {
            return Err(IndexError::Block(BlockError::Short {
                needed: HEAD_LEN_PREFIX,
            }));
        }
        if header[4] != VERSION || BlockKind::from_u8(header[5]) != BlockKind::Index {
            return Err(IndexError::Kind);
        }
        let at = get_u32(header, 32) as usize;
        if at < HEADER_LEN + INDEX_FIXED_LEN || at.checked_add(CRC_LEN).is_none_or(|end| end > len)
        {
            return Err(IndexError::BadHeadCrc);
        }
        Ok(at)
    }

    /// Read the head of an index block from `bytes`, its first [Self::head_len] bytes or more, head CRC checked.
    /// Entries are only read from a whole block or through [Self::track_entries].
    pub fn read_head(bytes: &'b [u8]) -> Result<Self, IndexError> {
        if bytes.len() < HEADER_LEN {
            return Err(IndexError::Block(BlockError::Short { needed: HEADER_LEN }));
        }
        let entries_at = Self::head_len(bytes)?;
        if bytes.len() < entries_at {
            return Err(IndexError::Block(BlockError::Short { needed: entries_at }));
        }
        if head_crc(bytes, entries_at) != get_u32(bytes, 28) {
            return Err(IndexError::BadHeadCrc);
        }
        let len = get_u32(bytes, 8) as usize;
        let flags = get_u16(bytes, 56);
        let entry_len = get_u16(bytes, 58) as usize;
        let tracks = get_u32(bytes, 36) as usize;
        let entries = get_u32(bytes, 12) as usize;
        let kind = if flags & FLAG_FINAL != 0 {
            IndexKind::Final
        } else {
            IndexKind::Checkpoint
        };
        let dir_end = tracks
            .checked_mul(TRACK_LEN)
            .and_then(|d| d.checked_add(HEADER_LEN + INDEX_FIXED_LEN));
        let body = entries
            .checked_mul(entry_len)
            .and_then(|b| b.checked_add(entries_at + CRC_LEN));
        if entry_len < ENTRY_LEN
            || dir_end != Some(entries_at)
            || body != Some(len)
            || (kind == IndexKind::Checkpoint && tracks != 0)
        {
            return Err(IndexError::BadHead);
        }
        let ix = IndexBlock {
            bytes,
            len,
            entries_at,
            entry_len,
            entries,
            tracks,
            kind,
        };
        let mut next = 0u64;
        let mut prev_id = None;
        for t in ix.tracks() {
            if u64::from(t.first) != next || prev_id.is_some_and(|p| p >= t.track_id) {
                return Err(IndexError::BadHead);
            }
            next += u64::from(t.count);
            prev_id = Some(t.track_id);
        }
        if kind == IndexKind::Final && next != entries as u64 {
            return Err(IndexError::BadHead);
        }
        Ok(ix)
    }

    pub fn kind(&self) -> IndexKind {
        self.kind
    }

    /// Where this block starts in its piece.
    pub fn self_offset(&self) -> u64 {
        get_u64(self.bytes, 40)
    }

    /// The previous index block of the same kind in the piece.
    pub fn prev(&self) -> Option<u64> {
        Some(get_u64(self.bytes, 48)).filter(|o| *o != NONE)
    }

    /// The block's whole length.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    pub fn entry_count(&self) -> usize {
        self.entries
    }

    /// Where entry `i` starts in the block.
    pub fn entry_offset(&self, i: usize) -> usize {
        self.entries_at + i * self.entry_len
    }

    /// Bytes of one entry in this block.
    pub fn entry_len(&self) -> usize {
        self.entry_len
    }

    /// Entry `i`, None past the last or when only the head was read.
    pub fn entry(&self, i: usize) -> Option<IndexEntry> {
        let at = self.entry_offset(i);
        if i >= self.entries || self.bytes.len() < at + self.entry_len {
            return None;
        }
        Some(IndexEntry::decode(&self.bytes[at..]))
    }

    /// Every entry; none when only the head was read.
    pub fn entries(&self) -> impl Iterator<Item = IndexEntry> + '_ {
        (0..self.entries).map_while(|i| self.entry(i))
    }

    /// The directory of a final index, ascending track id.
    pub fn tracks(&self) -> impl Iterator<Item = TrackList> + '_ {
        (0..self.tracks).map(|i| {
            let at = HEADER_LEN + INDEX_FIXED_LEN + i * TRACK_LEN;
            TrackList {
                track_id: get_u64(self.bytes, at),
                first: get_u32(self.bytes, at + 8),
                count: get_u32(self.bytes, at + 12),
                crc: get_u32(self.bytes, at + 16),
            }
        })
    }

    /// The directory entry of `track_id`, by binary search.
    pub fn track(&self, track_id: u64) -> Option<TrackList> {
        let (mut lo, mut hi) = (0, self.tracks);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = HEADER_LEN + INDEX_FIXED_LEN + mid * TRACK_LEN;
            let id = get_u64(self.bytes, at);
            match id.cmp(&track_id) {
                core::cmp::Ordering::Equal => return self.tracks().nth(mid),
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }

    /// Where `list`'s entries are in the block: offset and length in bytes.
    pub fn track_range(&self, list: &TrackList) -> (usize, usize) {
        (
            self.entry_offset(list.first as usize),
            list.count as usize * self.entry_len,
        )
    }

    /// The entries of `list` from their bytes ([Self::track_range]), checked against the list's CRC.
    pub fn track_entries<'e>(
        &self,
        list: &TrackList,
        bytes: &'e [u8],
    ) -> Result<impl Iterator<Item = IndexEntry> + 'e, IndexError> {
        let (_, n) = self.track_range(list);
        if bytes.len() != n || CRC.checksum(bytes) != list.crc {
            return Err(IndexError::BadEntries);
        }
        Ok(bytes.chunks_exact(self.entry_len).map(IndexEntry::decode))
    }
}

/// When a writer places a checkpoint index block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IndexPolicy {
    /// After at least this many bytes of blocks since the previous checkpoint (the rescan a crash costs); 0: never.
    pub bytes: u64,
    /// Also when blocks wait unlisted and the previous checkpoint is this long ago, at a block or a writer's tick;
    /// None by default, since a checkpoint placed by time makes the bytes depend on it.
    pub interval: Option<core::time::Duration>,
}

impl IndexPolicy {
    /// No checkpoints; the final index is still written at the finish.
    pub const NONE: IndexPolicy = IndexPolicy {
        bytes: 0,
        interval: None,
    };
}

impl Default for IndexPolicy {
    fn default() -> Self {
        IndexPolicy {
            bytes: DEFAULT_CHECKPOINT_BYTES,
            interval: None,
        }
    }
}

/// Default of [IndexPolicy::bytes]: 4 MiB.
pub const DEFAULT_CHECKPOINT_BYTES: u64 = 4 << 20;

#[cfg(feature = "std")]
mod build;
#[cfg(feature = "std")]
pub(crate) use build::Indexer;

#[cfg(test)]
mod tests;
