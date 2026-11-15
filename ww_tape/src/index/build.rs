//! [Indexer]: what a writer keeps to place checkpoints and write the final index and the finish.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::{
    CHECKPOINT_ENTRIES_MAX, ENTRY_LEN, IndexEntry, IndexError, IndexHead, IndexKind, IndexPolicy,
    index_len, track_count, write_index,
};
use crate::bytes::le_u8;
use crate::chunk::len_u64;
use crate::piece::{TrackRange, TrackStart};
use crate::{Block, BlockKind, Chunk, HEADER_LEN, MAX_BLOCK_LEN, VERSION};

/// Most entries in one final index block.
const FINAL_ENTRIES_MAX: usize = (MAX_BLOCK_LEN >> 1) / ENTRY_LEN;

/// The blocks of a piece as they are placed: entries not yet in a checkpoint, every entry for the final index,
/// and the item and tick range of each track's chunks.
#[derive(Debug, Default)]
pub(crate) struct Indexer {
    policy: IndexPolicy,
    pending: Vec<IndexEntry>,
    all: Vec<IndexEntry>,
    /// Bytes placed since the last checkpoint
    since: u64,
    last_checkpoint: Option<u64>,
    tracks: BTreeMap<u64, TrackRange>,
}

impl Indexer {
    pub(crate) fn new(policy: IndexPolicy) -> Self {
        Indexer {
            policy,
            ..Default::default()
        }
    }

    pub(crate) fn policy(&self) -> IndexPolicy {
        self.policy
    }

    pub(crate) fn set_policy(&mut self, policy: IndexPolicy) {
        self.policy = policy;
    }

    /// Note the block `bytes` (exactly one block, written by this writer) placed at `offset` in the piece. Its
    /// CRC is not checked again: a chunk block's head is read by its head CRC.
    pub(crate) fn placed(&mut self, offset: u64, bytes: &[u8]) {
        self.placed_unlisted(bytes.len());
        let Some(e) = of_placed(offset, bytes) else {
            return;
        };
        if matches!(e.kind, BlockKind::Events | BlockKind::Series) {
            add_to_range(&mut self.tracks, &e);
        }
        self.pending.push(e);
        self.all.push(e);
    }

    /// Note `len` bytes placed that no index lists: a block written again with the same bytes, which readers drop.
    pub(crate) fn placed_unlisted(&mut self, len: usize) {
        // past u64::MAX bytes a checkpoint is due all the same
        self.since = self.since.saturating_add(len_u64(len));
    }

    /// Drop the entry of the block at `offset`: a later block with its id and other bytes takes its place, as
    /// readers take it. A checkpoint placed already keeps it; the final index doesn't.
    pub(crate) fn replaced(&mut self, offset: u64) {
        let Some(i) = self.all.iter().rposition(|e| e.offset == offset) else {
            return;
        };
        let gone = self.all.remove(i);
        self.pending.retain(|e| e.offset != offset);
        if matches!(gone.kind, BlockKind::Events | BlockKind::Series) {
            self.tracks.remove(&gone.track_id);
            for e in self.all.iter().filter(|e| e.track_id == gone.track_id) {
                add_to_range(&mut self.tracks, e);
            }
        }
    }

    /// Entries not yet listed by a checkpoint.
    pub(crate) fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Whether the policy's byte count, or the entry limit, asks for a checkpoint now.
    pub(crate) fn due(&self) -> bool {
        !self.pending.is_empty()
            && ((self.policy.bytes > 0 && self.since >= self.policy.bytes)
                || self.pending.len() >= CHECKPOINT_ENTRIES_MAX)
    }

    /// The checkpoint index block listing the pending entries, to be placed at `offset` as block `id`.
    pub(crate) fn checkpoint(
        &mut self,
        offset: u64,
        id: crate::BlockId,
    ) -> Result<Vec<u8>, IndexError> {
        let head = IndexHead {
            kind: IndexKind::Checkpoint,
            self_offset: offset,
            prev: self.last_checkpoint,
        };
        let mut buf = vec![0u8; index_len(self.pending.len(), 0)];
        write_index(&mut buf, id, &head, &self.pending)?;
        self.pending.clear();
        self.since = 0;
        self.last_checkpoint = Some(offset);
        Ok(buf)
    }

    /// The newest checkpoint's offset.
    pub(crate) fn last_checkpoint(&self) -> Option<u64> {
        self.last_checkpoint
    }

    /// Every entry, sorted for the final index, cut into the parts one final block each holds: whole tracks while
    /// they fit, a track longer than a block over several.
    pub(crate) fn final_parts(&self) -> Vec<Vec<IndexEntry>> {
        let mut all = self.all.clone();
        all.sort_by_key(|e| (e.track_id, e.offset));
        let mut parts: Vec<Vec<IndexEntry>> = Vec::new();
        for track in all.chunk_by(|a, b| a.track_id == b.track_id) {
            for run in track.chunks(FINAL_ENTRIES_MAX) {
                match parts.last_mut() {
                    Some(p)
                        if p.len()
                            .checked_add(run.len())
                            .is_some_and(|n| n <= FINAL_ENTRIES_MAX) =>
                    {
                        p.extend_from_slice(run)
                    }
                    _ => parts.push(run.to_vec()),
                }
            }
        }
        parts
    }

    /// The final index block of `part`, placed at `offset` as block `id`, after the one at `prev`.
    pub(crate) fn final_block(
        part: &[IndexEntry],
        offset: u64,
        prev: Option<u64>,
        id: crate::BlockId,
    ) -> Result<Vec<u8>, IndexError> {
        let head = IndexHead {
            kind: IndexKind::Final,
            self_offset: offset,
            prev,
        };
        let mut buf = vec![0u8; index_len(part.len(), track_count(part))];
        write_index(&mut buf, id, &head, part)?;
        Ok(buf)
    }

    /// The item and tick range of each track's chunks, ascending track id.
    pub(crate) fn track_ranges(&self) -> Vec<TrackRange> {
        self.tracks.values().copied().collect()
    }

    /// Where each track's items continue in a next piece: `prev` (this piece's own starts) moved on by its chunks.
    pub(crate) fn next_starts(&self, prev: &[TrackStart]) -> Vec<TrackStart> {
        let mut out: BTreeMap<u64, TrackStart> = prev.iter().map(|t| (t.track_id, *t)).collect();
        for t in self.tracks.values() {
            let from_tick = out
                .get(&t.track_id)
                .map_or(t.last_tick, |s| s.from_tick.max(t.last_tick));
            out.insert(
                t.track_id,
                TrackStart {
                    track_id: t.track_id,
                    track_number: t.track_number,
                    next_item: t.end_item(),
                    from_tick,
                },
            );
        }
        out.into_values().collect()
    }
}

/// Add chunk entry `e` to its track's item and tick range.
fn add_to_range(tracks: &mut BTreeMap<u64, TrackRange>, e: &IndexEntry) {
    tracks
        .entry(e.track_id)
        .and_modify(|t| {
            // a piece of more than u32::MAX chunks keeps the count at u32::MAX
            t.chunks = t.chunks.saturating_add(1);
            t.items = e.end_item().saturating_sub(t.first_item);
            t.first_tick = t.first_tick.min(e.first_tick);
            t.last_tick = t.last_tick.max(e.last_tick);
            t.latest_end = t.latest_end.max(e.latest_end);
        })
        .or_insert(TrackRange {
            track_id: e.track_id,
            track_number: e.track_number,
            chunks: 1,
            first_item: e.first_item,
            items: e.count,
            first_tick: e.first_tick,
            last_tick: e.last_tick,
            latest_end: e.latest_end,
        });
}

/// The entry of the block `bytes` (exactly one) a writer placed at `offset`: a chunk block's head is checked
/// by its head CRC alone, without a pass over the payload; other blocks by their CRC.
fn of_placed(offset: u64, bytes: &[u8]) -> Option<IndexEntry> {
    let chunk = bytes.len() > HEADER_LEN
        && le_u8(bytes, 4) == Some(VERSION)
        && le_u8(bytes, 5).is_some_and(|k| {
            matches!(BlockKind::from_u8(k), BlockKind::Events | BlockKind::Series)
        });
    if chunk {
        let c = Chunk::read_head(bytes).ok()?;
        return IndexEntry::of_chunk(offset, bytes.len(), &c.head());
    }
    IndexEntry::of_block(offset, &Block::read(bytes).ok()?)
}
