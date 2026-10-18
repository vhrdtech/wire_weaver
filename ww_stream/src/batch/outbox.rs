//! [Outbox]: encoded batches of one trait level's observed properties, waiting to be packed.

use super::{Consumer, EncodeError, Scratch};
use wire_weaver::shrink_wrap::Error;
use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesPolicy, SeriesStore};

/// Bytes of an [Outbox] entry's header: property index, items and length, `u16` each.
pub const OUTBOX_ENTRY_HEADER: usize = 6;

/// Encoded batches of the observed properties of one trait level, waiting to be packed into `PropertyUpdate`s
/// (API-39): sizes are known before a presence header is written, and each batch is encoded once. Oldest first;
/// when it is full, the oldest batches are dropped to make room and their items counted as missed.
pub struct Outbox<const BYTES: usize> {
    buf: [u8; BYTES],
    used: usize,
    entries: usize,
}

/// A batch in an [Outbox].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxEntry<'b> {
    /// Property index in the level.
    pub k: u16,
    /// Items in the batch.
    pub items: u16,
    /// The `Series` bytes.
    pub bytes: &'b [u8],
}

/// What [Outbox::push_from] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxPush {
    /// Entries added (a batch that didn't fit the free room is split).
    pub batches: usize,
    /// Items added.
    pub items: usize,
    /// Items of older entries dropped to make room: missed samples.
    pub dropped: u64,
    /// Items the batcher lost or discarded (it was too slow, or one item is bigger than the outbox): missed too.
    pub lost: u64,
}

impl<const BYTES: usize> Outbox<BYTES> {
    pub const fn new() -> Self {
        const {
            assert!(
                BYTES > OUTBOX_ENTRY_HEADER && BYTES <= u16::MAX as usize,
                "Outbox size out of 7..=65535"
            )
        };
        Outbox {
            buf: [0; BYTES],
            used: 0,
            entries: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// Bytes in use, headers included.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    fn free(&self) -> usize {
        (BYTES - self.used).saturating_sub(OUTBOX_ENTRY_HEADER)
    }

    fn entry_at(&self, off: usize) -> (OutboxEntry<'_>, usize) {
        let h = &self.buf[off..off + OUTBOX_ENTRY_HEADER];
        let k = u16::from_le_bytes([h[0], h[1]]);
        let items = u16::from_le_bytes([h[2], h[3]]);
        let len = u16::from_le_bytes([h[4], h[5]]) as usize;
        let start = off + OUTBOX_ENTRY_HEADER;
        (
            OutboxEntry {
                k,
                items,
                bytes: &self.buf[start..start + len],
            },
            start + len,
        )
    }

    /// The entries, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = OutboxEntry<'_>> {
        let mut off = 0;
        core::iter::from_fn(move || {
            if off >= self.used {
                return None;
            }
            let (entry, next) = self.entry_at(off);
            off = next;
            Some(entry)
        })
    }

    /// The oldest entry of property `k`.
    pub fn first_of(&self, k: u16) -> Option<OutboxEntry<'_>> {
        self.iter().find(|e| e.k == k)
    }

    /// Removes the bytes `off..end`, one entry.
    fn remove_at(&mut self, off: usize, end: usize) {
        self.buf.copy_within(end..self.used, off);
        self.used -= end - off;
        self.entries -= 1;
    }

    /// Removes the oldest entry of property `k` (sent); returns whether there was one.
    pub fn remove_first_of(&mut self, k: u16) -> bool {
        let mut off = 0;
        while off < self.used {
            let (entry, next) = self.entry_at(off);
            if entry.k == k {
                self.remove_at(off, next);
                return true;
            }
            off = next;
        }
        false
    }

    /// Removes the oldest entry, returns its items.
    pub fn pop_oldest(&mut self) -> Option<u16> {
        if self.entries == 0 {
            return None;
        }
        let (entry, next) = self.entry_at(0);
        let items = entry.items;
        self.remove_at(0, next);
        Some(items)
    }

    /// Drops everything, returns the items dropped.
    pub fn clear(&mut self) -> u64 {
        let items = self.iter().map(|e| e.items as u64).sum();
        self.used = 0;
        self.entries = 0;
        items
    }

    /// Encodes the sealed batch of `batcher` (a [Consumer], [Batcher::consumer](super::Batcher::consumer) of a
    /// batcher used from one task) as entries of property `k`, making room by dropping entries that were here
    /// before (oldest first) where it doesn't fit; entries added by this call are never dropped by it. What still
    /// doesn't fit stays sealed in the batcher. A batch whose worst case fits the free room is encoded right away,
    /// else it is counted first, so the room made is what it needs.
    pub fn push_from<T, const N: usize, P, const B: usize>(
        &mut self,
        k: u16,
        batcher: &mut Consumer<'_, T, N, P>,
        scratch: &mut Scratch<B>,
    ) -> Result<OutboxPush, Error>
    where
        T: SeriesStore + for<'a> SeriesColumns<'a>,
        P: SeriesPolicy + Copy,
    {
        let mut done = OutboxPush::default();
        let mut droppable = self.entries;
        while batcher.has_sealed() {
            if batcher.sealed_bound().is_none_or(|max| max > self.free()) {
                let need = batcher
                    .sealed_len(scratch)?
                    .unwrap_or(0)
                    .min(BYTES - OUTBOX_ENTRY_HEADER);
                while self.free() < need && droppable > 0 {
                    done.dropped += self.pop_oldest().unwrap_or(0) as u64;
                    droppable -= 1;
                }
            }
            let start = self.used + OUTBOX_ENTRY_HEADER;
            let room = if start <= BYTES {
                &mut self.buf[start..]
            } else {
                &mut []
            };
            match batcher.encode_into(scratch, room) {
                Ok(Some(enc)) => {
                    let h = &mut self.buf[self.used..start];
                    h[0..2].copy_from_slice(&k.to_le_bytes());
                    h[2..4].copy_from_slice(&(enc.items as u16).to_le_bytes());
                    h[4..6].copy_from_slice(&(enc.len as u16).to_le_bytes());
                    self.used = start + enc.len;
                    self.entries += 1;
                    done.batches += 1;
                    done.items += enc.items;
                    done.lost += enc.lost_before;
                }
                Ok(None) => break,
                Err(EncodeError::NoRoom) if droppable > 0 => {
                    done.dropped += self.pop_oldest().unwrap_or(0) as u64;
                    droppable -= 1;
                }
                Err(EncodeError::NoRoom) if self.entries == 0 => {
                    // one item is bigger than the whole outbox: it can never be sent
                    let (items, lost_before) = batcher.discard_sealed();
                    done.lost += items as u64 + lost_before;
                }
                // the rest waits for room
                Err(EncodeError::NoRoom) => break,
                Err(EncodeError::Write(e)) => return Err(e),
            }
        }
        Ok(done)
    }
}

impl<const BYTES: usize> Default for Outbox<BYTES> {
    fn default() -> Self {
        Self::new()
    }
}
