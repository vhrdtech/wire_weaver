//! Tape pieces: one recording cut into complete files, and how a piece ends.
//!
//! A **piece** is a complete tape file: a [piece head](PieceHead) block, describe blocks, chunk, summary and record
//! blocks with checkpoint [index](crate::index) blocks between them, then at the finish a final index, a backup of
//! the describe blocks, a [finish](Finish) block and a fixed-size [tail](Tail) block. Any piece reads alone; pieces
//! written back to back are one valid tape. A file a writer started without a piece head is one piece too, with no
//! ids.
//!
//! A writer cuts a recording into pieces at chunk boundaries ([PieceChain]). The head names the
//! previous piece by its content hash and its id; the previous piece's tail names this piece's id, derived at the cut
//! from the previous piece's id and the content hash in its finish ([PieceId::after]), so the chain is checked both ways ([check_chain]). Each piece carries the result id, its
//! sequence number in the chain and, per track, where its items continue (head) and the items and ticks it holds
//! (finish). A chain whose first piece is gone reads as head cut, not as damage.
//!
//! The finish block is the last thing a writer places before the tail. Its content hash is BLAKE3 over the piece's
//! bytes before it; a piece without one is live or was cut off, and reads up to its last good block. The tail sits
//! at the very end of the file, [TAIL_LEN] bytes, so a reader finds it with one read and from it the finish, the
//! indexes and the start of the piece; the tail of the piece before is just before that start.
//!
//! Layouts after the 32-byte block header (the header's count: tracks), little endian:
//!
//! ```text
//! piece head (kind 10)
//!  32 [16] piece id                 48 [32] result id               80 u64 sequence number in the chain
//!  88 u32  flags: bit 0 has a previous piece                        92 u32 previous run tag (0: none or unknown)
//!  96 [32] previous piece's content hash                           128 [16] previous piece's id
//! 144 ...  tracks × 32: id u64, number u32, reserved u32, next item u64 (the first item number this piece holds),
//!          from tick i64 (no item of the track here starts before it)
//!
//! finish (kind 8)
//!  32 u8   state (1 complete, 2 continued in the next piece, 3 aborted)    33 [7] reserved
//!  40 [32] content hash: BLAKE3 of the piece's bytes before this block
//!  72 u64  newest checkpoint index offset    80 u64 last final index offset    88 u64 describe backup offset
//!          (u64::MAX for none)
//!  96 [16] piece id    112 [16] next piece id    128 [32] result id    160 u64 sequence number
//! 168 u32  flags: bit 0 the piece has a head    172 u32 reserved
//! 176 ...  tracks × 56: id u64, number u32, chunks u32, first item u64, items u64, first tick i64, last tick i64,
//!          latest end i64 (of the chunk blocks in the piece)
//!
//! tail (kind 9), TAIL_LEN bytes, the last of the piece
//!  32 u64  piece length, from the piece's start to the end of this block
//!  40 u64  finish offset    48 u32 finish length    52 u8 state    53 u8 flags (as the finish's)    54 u16 reserved
//!  56 [32] content hash    88 [16] piece id    104 [16] next piece id    120 [32] result id    152 u64 sequence
//! 160 u32  CRC
//! ```
//!
//! Offsets are from the start of the piece. Ids are opaque: a piece id is 16 bytes, a result id 32 (the producer's,
//! e.g. a SHA-256 of a run record); all zeros is none.

use crate::chunk::{get_u32, get_u64, put_u32};
use crate::index::{IndexError, NONE, seal};
use crate::{Block, BlockId, BlockKind, CRC_LEN, HEADER_LEN, RunTag, put_header};

/// Bytes of a tail block.
pub const TAIL_LEN: usize = 164;
/// Bytes of the fixed part of a piece head body.
const HEAD_FIXED_LEN: usize = 112;
const HEAD_TRACK_LEN: usize = 32;
/// Bytes of the fixed part of a finish body.
const FINISH_FIXED_LEN: usize = 144;
const FINISH_TRACK_LEN: usize = 56;

const FLAG_HAS_PREV: u32 = 1;
const FLAG_PIECE: u8 = 1;

/// Names a piece of a chain.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PieceId(pub [u8; 16]);

/// Names the result (producer run) a piece belongs to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ResultId(pub [u8; 32]);

/// BLAKE3 of a piece's bytes before its finish block.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ContentHash(pub [u8; 32]);

impl PieceId {
    /// No piece id: all zeros.
    pub const NONE: PieceId = PieceId([0; 16]);

    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// The id of the piece after one with `id` and `hash`: the first 16 bytes of BLAKE3 in key derivation mode
    /// ("ww_tape piece id") over the hash, then the id. The same chain gives the same ids.
    pub fn after(id: PieceId, hash: &ContentHash) -> PieceId {
        Self::derive("ww_tape piece id", &[&hash.0, &id.0])
    }

    /// The id of the first piece of chain 0 of `result`: [Self::first_of] `(result, 0)`.
    pub fn first(result: &ResultId) -> PieceId {
        Self::first_of(result, 0)
    }

    /// The id of the first piece of chain `chain` of `result` (several chains of one result, such as one per
    /// source): as [Self::after] over the result id and the chain number, u64 little endian ("ww_tape first
    /// piece").
    pub fn first_of(result: &ResultId, chain: u64) -> PieceId {
        Self::derive("ww_tape first piece", &[&result.0, &chain.to_le_bytes()])
    }

    /// The first 16 bytes of BLAKE3 in key derivation mode over `parts`; never all zeros.
    fn derive(context: &str, parts: &[&[u8]]) -> PieceId {
        let mut h = blake3::Hasher::new_derive_key(context);
        for p in parts {
            h.update(p);
        }
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.finalize().as_bytes()[..16]);
        if out == [0; 16] {
            out[0] = 1;
        }
        PieceId(out)
    }
}

/// How a piece ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FinishState {
    /// The recording ended here.
    Complete,
    /// The recording goes on in the next piece, the one the tail names.
    Continued,
    /// The writer stopped on purpose without completing it (the producer failed); everything written is good.
    Aborted,
}

impl FinishState {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(FinishState::Complete),
            2 => Some(FinishState::Continued),
            3 => Some(FinishState::Aborted),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            FinishState::Complete => 1,
            FinishState::Continued => 2,
            FinishState::Aborted => 3,
        }
    }
}

/// Where a piece sits in its chain, as its head, finish and tail say.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct PieceIdent {
    pub id: PieceId,
    pub result: ResultId,
    /// 0 for the first piece of a chain, +1 per piece.
    pub seq: u64,
}

/// Where a track's items continue in a piece, from its head.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackStart {
    pub track_id: u64,
    pub track_number: u32,
    /// The first item number of the track in this piece: one past the last in the piece before.
    pub next_item: u64,
    /// No item of the track in this piece starts before this tick: the last start in the piece before.
    pub from_tick: i64,
}

/// The items and ticks of one track's chunk blocks in a piece, from its finish.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackRange {
    pub track_id: u64,
    pub track_number: u32,
    /// Chunk blocks of the track in the piece.
    pub chunks: u32,
    pub first_item: u64,
    pub items: u64,
    pub first_tick: i64,
    pub last_tick: i64,
    pub latest_end: i64,
}

impl TrackRange {
    /// Item number past the last.
    pub fn end_item(&self) -> u64 {
        self.first_item.saturating_add(self.items)
    }
}

/// The fixed part of a piece head.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PieceHead {
    pub ident: PieceIdent,
    /// The previous piece's content hash and id; None for the first piece of a chain.
    pub prev: Option<(ContentHash, PieceId)>,
    /// The run of the block before this piece head on the medium: the run that ended where a writer appending to
    /// a medium found the end of data, or the piece's own run at a cut. [RunTag::NONE] when the writer started at
    /// the medium's start or can't know. On raw media a reader takes a new run only at a piece head naming the run
    /// just ended, so stale blocks behind the end of data never pass for a new run.
    pub prev_run: RunTag,
}

/// Bytes of a piece head block with `tracks`.
pub const fn piece_head_len(tracks: usize) -> usize {
    HEADER_LEN + HEAD_FIXED_LEN + tracks * HEAD_TRACK_LEN + CRC_LEN
}

/// Write a piece head block into `buf`; returns its length.
pub fn write_piece_head(
    buf: &mut [u8],
    id: BlockId,
    head: &PieceHead,
    tracks: &[TrackStart],
) -> Result<usize, IndexError> {
    let len = piece_head_len(tracks.len());
    if len > crate::MAX_BLOCK_LEN {
        return Err(IndexError::Invalid("piece head longer than a block"));
    }
    if buf.len() < len {
        return Err(IndexError::Full { needed: len });
    }
    let b = &mut buf[..len];
    put_header(b, BlockKind::PieceHead, len, tracks.len() as u32, id);
    b[HEADER_LEN..].fill(0);
    put_ident(b, 32, 48, 80, &head.ident);
    put_u32(b, 92, head.prev_run.get());
    if let Some((hash, id)) = head.prev {
        put_u32(b, 88, FLAG_HAS_PREV);
        b[96..128].copy_from_slice(&hash.0);
        b[128..144].copy_from_slice(&id.0);
    }
    for (i, t) in tracks.iter().enumerate() {
        let at = HEADER_LEN + HEAD_FIXED_LEN + i * HEAD_TRACK_LEN;
        b[at..at + 8].copy_from_slice(&t.track_id.to_le_bytes());
        put_u32(b, at + 8, t.track_number);
        b[at + 16..at + 24].copy_from_slice(&t.next_item.to_le_bytes());
        b[at + 24..at + 32].copy_from_slice(&t.from_tick.to_le_bytes());
    }
    seal(b);
    Ok(len)
}

fn put_ident(b: &mut [u8], id_at: usize, result_at: usize, seq_at: usize, ident: &PieceIdent) {
    b[id_at..id_at + 16].copy_from_slice(&ident.id.0);
    b[result_at..result_at + 32].copy_from_slice(&ident.result.0);
    b[seq_at..seq_at + 8].copy_from_slice(&ident.seq.to_le_bytes());
}

fn get_ident(b: &[u8], id_at: usize, result_at: usize, seq_at: usize) -> PieceIdent {
    PieceIdent {
        id: PieceId(arr(b, id_at)),
        result: ResultId(arr(b, result_at)),
        seq: get_u64(b, seq_at),
    }
}

fn arr<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b[at..at + N].try_into().expect("N bytes")
}

/// Check a good block is of `kind` and holds `fixed` bytes of body plus `per` bytes for each of its count; returns
/// the count.
fn body(block: &Block<'_>, kind: BlockKind, fixed: usize, per: usize) -> Result<usize, IndexError> {
    if block.kind != kind || !block.body_known() {
        return Err(IndexError::Kind);
    }
    let n = block.record_count as usize;
    let want = n
        .checked_mul(per)
        .and_then(|x| x.checked_add(HEADER_LEN + fixed + CRC_LEN));
    if want != Some(block.len()) {
        return Err(IndexError::BadHead);
    }
    Ok(n)
}

/// A piece head block read in place.
#[derive(Copy, Clone, Debug)]
pub struct PieceHeadBlock<'b> {
    bytes: &'b [u8],
    tracks: usize,
}

impl<'b> PieceHeadBlock<'b> {
    /// The piece head of a good block.
    pub fn read(block: &Block<'b>) -> Result<Self, IndexError> {
        let tracks = body(block, BlockKind::PieceHead, HEAD_FIXED_LEN, HEAD_TRACK_LEN)?;
        Ok(PieceHeadBlock {
            bytes: block.bytes(),
            tracks,
        })
    }

    pub fn head(&self) -> PieceHead {
        let b = self.bytes;
        let prev = (get_u32(b, 88) & FLAG_HAS_PREV != 0)
            .then(|| (ContentHash(arr(b, 96)), PieceId(arr(b, 128))));
        PieceHead {
            ident: get_ident(b, 32, 48, 80),
            prev,
            prev_run: RunTag::from_u32(get_u32(b, 92)),
        }
    }

    pub fn tracks(&self) -> impl Iterator<Item = TrackStart> + '_ {
        (0..self.tracks).map(|i| {
            let at = HEADER_LEN + HEAD_FIXED_LEN + i * HEAD_TRACK_LEN;
            let b = self.bytes;
            TrackStart {
                track_id: get_u64(b, at),
                track_number: get_u32(b, at + 8),
                next_item: get_u64(b, at + 16),
                from_tick: get_u64(b, at + 24) as i64,
            }
        })
    }
}

/// The fixed part of a finish block.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Finish {
    pub state: FinishState,
    pub hash: ContentHash,
    /// The newest checkpoint index block.
    pub checkpoint: Option<u64>,
    /// The last final index block (each points at the one before).
    pub final_index: Option<u64>,
    /// The first describe block of the backup before the finish.
    pub describe: Option<u64>,
    /// The piece's ident; None for a file written without a piece head.
    pub ident: Option<PieceIdent>,
    /// The next piece's id, chosen at the cut: some when [FinishState::Continued].
    pub next: Option<PieceId>,
}

/// Bytes of a finish block with `tracks`.
pub const fn finish_len(tracks: usize) -> usize {
    HEADER_LEN + FINISH_FIXED_LEN + tracks * FINISH_TRACK_LEN + CRC_LEN
}

fn opt(v: Option<u64>) -> [u8; 8] {
    v.unwrap_or(NONE).to_le_bytes()
}

fn get_opt(b: &[u8], at: usize) -> Option<u64> {
    Some(get_u64(b, at)).filter(|v| *v != NONE)
}

/// Write a finish block into `buf`; returns its length.
pub fn write_finish(
    buf: &mut [u8],
    id: BlockId,
    finish: &Finish,
    tracks: &[TrackRange],
) -> Result<usize, IndexError> {
    let len = finish_len(tracks.len());
    if len > crate::MAX_BLOCK_LEN {
        return Err(IndexError::Invalid("finish longer than a block"));
    }
    if buf.len() < len {
        return Err(IndexError::Full { needed: len });
    }
    let b = &mut buf[..len];
    put_header(b, BlockKind::Finish, len, tracks.len() as u32, id);
    b[HEADER_LEN..].fill(0);
    b[32] = finish.state.to_u8();
    b[40..72].copy_from_slice(&finish.hash.0);
    b[72..80].copy_from_slice(&opt(finish.checkpoint));
    b[80..88].copy_from_slice(&opt(finish.final_index));
    b[88..96].copy_from_slice(&opt(finish.describe));
    if let Some(ident) = &finish.ident {
        put_ident(b, 96, 128, 160, ident);
        put_u32(b, 168, u32::from(FLAG_PIECE));
    }
    b[112..128].copy_from_slice(&finish.next.unwrap_or(PieceId::NONE).0);
    for (i, t) in tracks.iter().enumerate() {
        let at = HEADER_LEN + FINISH_FIXED_LEN + i * FINISH_TRACK_LEN;
        b[at..at + 8].copy_from_slice(&t.track_id.to_le_bytes());
        put_u32(b, at + 8, t.track_number);
        put_u32(b, at + 12, t.chunks);
        b[at + 16..at + 24].copy_from_slice(&t.first_item.to_le_bytes());
        b[at + 24..at + 32].copy_from_slice(&t.items.to_le_bytes());
        b[at + 32..at + 40].copy_from_slice(&t.first_tick.to_le_bytes());
        b[at + 40..at + 48].copy_from_slice(&t.last_tick.to_le_bytes());
        b[at + 48..at + 56].copy_from_slice(&t.latest_end.to_le_bytes());
    }
    seal(b);
    Ok(len)
}

/// A finish block read in place.
#[derive(Copy, Clone, Debug)]
pub struct FinishBlock<'b> {
    bytes: &'b [u8],
    tracks: usize,
}

impl<'b> FinishBlock<'b> {
    /// The finish of a good block.
    pub fn read(block: &Block<'b>) -> Result<Self, IndexError> {
        let tracks = body(block, BlockKind::Finish, FINISH_FIXED_LEN, FINISH_TRACK_LEN)?;
        if FinishState::from_u8(block.bytes()[32]).is_none() {
            return Err(IndexError::BadHead);
        }
        Ok(FinishBlock {
            bytes: block.bytes(),
            tracks,
        })
    }

    pub fn finish(&self) -> Finish {
        let b = self.bytes;
        let piece = get_u32(b, 168) & u32::from(FLAG_PIECE) != 0;
        let next = PieceId(arr(b, 112));
        Finish {
            state: FinishState::from_u8(b[32]).expect("checked in read"),
            hash: ContentHash(arr(b, 40)),
            checkpoint: get_opt(b, 72),
            final_index: get_opt(b, 80),
            describe: get_opt(b, 88),
            ident: piece.then(|| get_ident(b, 96, 128, 160)),
            next: (!next.is_none()).then_some(next),
        }
    }

    pub fn tracks(&self) -> impl Iterator<Item = TrackRange> + '_ {
        (0..self.tracks).map(|i| {
            let at = HEADER_LEN + FINISH_FIXED_LEN + i * FINISH_TRACK_LEN;
            let b = self.bytes;
            TrackRange {
                track_id: get_u64(b, at),
                track_number: get_u32(b, at + 8),
                chunks: get_u32(b, at + 12),
                first_item: get_u64(b, at + 16),
                items: get_u64(b, at + 24),
                first_tick: get_u64(b, at + 32) as i64,
                last_tick: get_u64(b, at + 40) as i64,
                latest_end: get_u64(b, at + 48) as i64,
            }
        })
    }
}

/// What a tail block says.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tail {
    /// Bytes from the piece's start to the end of the tail.
    pub piece_len: u64,
    pub finish_offset: u64,
    pub finish_len: u32,
    pub state: FinishState,
    pub hash: ContentHash,
    pub ident: Option<PieceIdent>,
    pub next: Option<PieceId>,
}

impl Tail {
    /// Write the tail block into `buf` (at least [TAIL_LEN] bytes); returns [TAIL_LEN].
    ///
    /// # Panics
    ///
    /// When `buf` is shorter than [TAIL_LEN].
    pub fn write(&self, buf: &mut [u8], id: BlockId) -> usize {
        assert!(buf.len() >= TAIL_LEN, "a tail is {TAIL_LEN} bytes");
        let b = &mut buf[..TAIL_LEN];
        put_header(b, BlockKind::Tail, TAIL_LEN, 0, id);
        b[HEADER_LEN..].fill(0);
        b[32..40].copy_from_slice(&self.piece_len.to_le_bytes());
        b[40..48].copy_from_slice(&self.finish_offset.to_le_bytes());
        put_u32(b, 48, self.finish_len);
        b[52] = self.state.to_u8();
        b[56..88].copy_from_slice(&self.hash.0);
        if let Some(ident) = &self.ident {
            b[53] = FLAG_PIECE;
            put_ident(b, 88, 120, 152, ident);
        }
        b[104..120].copy_from_slice(&self.next.unwrap_or(PieceId::NONE).0);
        seal(b);
        TAIL_LEN
    }

    /// Read a tail from its [TAIL_LEN] bytes (a file's last ones), CRC checked.
    pub fn read(bytes: &[u8]) -> Result<Self, IndexError> {
        let block = Block::read(bytes)?;
        if block.kind != BlockKind::Tail || !block.body_known() {
            return Err(IndexError::Kind);
        }
        let b = block.bytes();
        if b.len() != TAIL_LEN || block.record_count != 0 {
            return Err(IndexError::BadHead);
        }
        let state = FinishState::from_u8(b[52]).ok_or(IndexError::BadHead)?;
        let piece_len = get_u64(b, 32);
        let finish_offset = get_u64(b, 40);
        let finish_len = get_u32(b, 48);
        let fits = finish_offset
            .checked_add(u64::from(finish_len))
            .and_then(|end| end.checked_add(TAIL_LEN as u64))
            .is_some_and(|end| end <= piece_len);
        if !fits {
            return Err(IndexError::BadHead);
        }
        let next = PieceId(arr(b, 104));
        Ok(Tail {
            piece_len,
            finish_offset,
            finish_len,
            state,
            hash: ContentHash(arr(b, 56)),
            ident: (b[53] & FLAG_PIECE != 0).then(|| get_ident(b, 88, 120, 152)),
            next: (!next.is_none()).then_some(next),
        })
    }
}

#[cfg(feature = "alloc")]
mod chain;
#[cfg(feature = "alloc")]
pub use chain::{ChainEnd, ChainLink, ChainReport, PieceSummary, check_chain};

#[cfg(feature = "std")]
mod writer;
#[cfg(feature = "std")]
pub use writer::{ChainOptions, CutPolicy, PieceChain, piece_name};

#[cfg(test)]
mod tests;
