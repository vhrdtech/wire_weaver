//! [Outbox]: encoded batches of one trait level's samples, waiting to be packed.

use super::Scratch;
use super::level::{Columns, LevelRow, MAX_COLUMNS, RowsSource};
use core::ops::Range;
use wire_weaver::shrink_wrap::Error;
use wire_weaver::shrink_wrap::series::SeriesPolicy;

/// Bytes of an [Outbox] entry's header: slot, items and length, `u16` each.
pub const OUTBOX_ENTRY_HEADER: usize = 6;

/// Slot of the time column's entry of a level batch: it starts the batch.
const TIME: u16 = u16::MAX;

/// Encoded batches of the samples of one trait level, waiting to be packed into `PropertyUpdate`s (API-39): sizes
/// are known before a presence header is written, and each batch is encoded once. A level batch is the time column
/// (`Series<Second>`, one item per row) followed by the column of each property observed in `Samples` mode
/// (`Series<T>`, as many items), and is sent whole, in one update. Oldest first; when it is full, the oldest batches
/// are dropped to make room and their samples counted as missed.
pub struct Outbox<const BYTES: usize> {
    buf: [u8; BYTES],
    used: usize,
    entries: usize,
    /// Level batches in `buf`
    batches: usize,
    /// End of the oldest level batch, which starts at 0 (0 when empty)
    oldest_end: usize,
}

/// One column of a batch in an [Outbox].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxEntry<'b> {
    /// Slot of the property in its level's observe table.
    pub k: u16,
    /// Samples in the column: the batch's rows.
    pub items: u16,
    /// The `Series` bytes.
    pub bytes: &'b [u8],
}

/// The oldest level batch in an [Outbox].
#[derive(Debug, Clone, Copy)]
pub struct LevelBatch<'b> {
    /// Rows: items of every column.
    pub rows: u16,
    /// The time column's `Series<Second>` bytes.
    pub time: &'b [u8],
    /// The value columns' entries, `rows` items each.
    columns: &'b [u8],
}

impl<'b> LevelBatch<'b> {
    /// The value columns, in the order they were encoded (slot order).
    pub fn columns(&self) -> impl Iterator<Item = OutboxEntry<'b>> + Clone + 'b {
        let buf = self.columns;
        let mut off = 0;
        core::iter::from_fn(move || {
            if off >= buf.len() {
                return None;
            }
            let (entry, next) = entry_at(buf, off);
            off = next;
            Some(entry)
        })
    }

    /// The column of slot `k`, if the batch has one.
    pub fn column(&self, k: u16) -> Option<OutboxEntry<'b>> {
        self.columns().find(|e| e.k == k)
    }

    /// [column](Self::column), searched from `at` (0, or what an earlier call of this batch left there) round to
    /// it again, and `at` set after the column found: looking slots up in the order they were encoded walks the
    /// columns once instead of once per slot.
    pub fn column_after(&self, k: u16, at: &mut usize) -> Option<OutboxEntry<'b>> {
        let buf = self.columns;
        if buf.is_empty() {
            return None;
        }
        let start = if *at < buf.len() { *at } else { 0 };
        let mut off = start;
        loop {
            let (entry, next) = entry_at(buf, off);
            off = if next >= buf.len() { 0 } else { next };
            if entry.k == k {
                *at = off;
                return Some(entry);
            }
            if off == start {
                return None;
            }
        }
    }
}

/// What [Outbox::push_rows_from] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxPush {
    /// Level batches added (a batch that didn't fit the free room is split).
    pub batches: usize,
    /// Rows added.
    pub rows: usize,
    /// Samples (rows times columns) of older batches dropped to make room: missed.
    pub dropped: u64,
    /// Rows the batcher lost or discarded (it was too slow, or one row is bigger than the outbox): missed too.
    pub lost: u64,
}

fn entry_at(buf: &[u8], off: usize) -> (OutboxEntry<'_>, usize) {
    let h = &buf[off..off + OUTBOX_ENTRY_HEADER];
    let k = u16::from_le_bytes([h[0], h[1]]);
    let items = u16::from_le_bytes([h[2], h[3]]);
    let len = u16::from_le_bytes([h[4], h[5]]) as usize;
    let start = off + OUTBOX_ENTRY_HEADER;
    (
        OutboxEntry {
            k,
            items,
            bytes: &buf[start..start + len],
        },
        start + len,
    )
}

impl<const BYTES: usize> Outbox<BYTES> {
    pub const fn new() -> Self {
        const {
            assert!(
                BYTES > 2 * OUTBOX_ENTRY_HEADER && BYTES <= u16::MAX as usize,
                "Outbox size out of 13..=65535"
            )
        };
        Outbox {
            buf: [0; BYTES],
            used: 0,
            entries: 0,
            batches: 0,
            oldest_end: 0,
        }
    }

    /// [new](Self::new) in place: for an `ObserveState` built in a `static` (`ObserveState::init`), the buffer
    /// zeroed where it stays, never on the stack.
    pub fn init(slot: &mut core::mem::MaybeUninit<Self>) -> &mut Self {
        const {
            assert!(
                BYTES > 2 * OUTBOX_ENTRY_HEADER && BYTES <= u16::MAX as usize,
                "Outbox size out of 13..=65535"
            )
        };
        // every field is written below: a new one fails to compile here until it is
        let _ = |o: Self| {
            let Outbox {
                buf: _,
                used: _,
                entries: _,
                batches: _,
                oldest_end: _,
            } = o;
        };
        let at = slot.as_mut_ptr();
        // SAFETY: every field written in place, each through its own pointer
        unsafe {
            core::ptr::addr_of_mut!((*at).buf).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*at).used).write(0);
            core::ptr::addr_of_mut!((*at).entries).write(0);
            core::ptr::addr_of_mut!((*at).batches).write(0);
            core::ptr::addr_of_mut!((*at).oldest_end).write(0);
            slot.assume_init_mut()
        }
    }

    /// Level batches waiting.
    pub fn len(&self) -> usize {
        self.batches
    }

    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// Bytes in use, headers included.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Every entry, time columns too, oldest first.
    fn entries(&self) -> impl Iterator<Item = (usize, OutboxEntry<'_>, usize)> {
        self.entries_in(0, self.used)
    }

    /// The entries in `start..end` (entry boundaries).
    fn entries_in(
        &self,
        start: usize,
        end: usize,
    ) -> impl Iterator<Item = (usize, OutboxEntry<'_>, usize)> {
        let mut off = start;
        core::iter::from_fn(move || {
            if off >= end {
                return None;
            }
            let at = off;
            let (entry, next) = entry_at(&self.buf[..end], off);
            off = next;
            Some((at, entry, next))
        })
    }

    /// Counts the level batches and finds the oldest one's end again, after entries were removed.
    fn reindex(&mut self) {
        let mut batches = 0;
        let mut oldest_end = 0;
        for (_, e, next) in self.entries() {
            if e.k == TIME {
                batches += 1;
            }
            if batches == 1 {
                oldest_end = next;
            }
        }
        self.batches = batches;
        self.oldest_end = oldest_end;
    }

    /// Start and end offset of each level batch, oldest first.
    fn spans(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let mut entries = self.entries().peekable();
        core::iter::from_fn(move || {
            let (start, first, mut end) = entries.next()?;
            debug_assert_eq!(first.k, TIME);
            while let Some((_, e, next)) = entries.peek() {
                if e.k == TIME {
                    break;
                }
                end = *next;
                entries.next();
            }
            Some((start, end))
        })
    }

    /// Panics when the kept batch count or the oldest batch's end don't match a walk over the entries.
    #[cfg(any(test, all(feature = "std", fuzzing)))]
    pub(crate) fn assert_index(&self) {
        assert_eq!(self.batches, self.spans().count(), "outbox batch count");
        assert_eq!(
            self.oldest_end,
            self.spans().next().map_or(0, |(_, end)| end),
            "outbox oldest batch end"
        );
    }

    /// The level batches, oldest first.
    pub fn batches(&self) -> impl Iterator<Item = LevelBatch<'_>> {
        self.spans().map(|(start, end)| {
            let (time, columns) = entry_at(&self.buf, start);
            LevelBatch {
                rows: time.items,
                time: time.bytes,
                columns: &self.buf[columns..end],
            }
        })
    }

    /// The oldest level batch.
    pub fn oldest(&self) -> Option<LevelBatch<'_>> {
        if self.batches == 0 {
            return None;
        }
        let (time, columns) = entry_at(&self.buf, 0);
        Some(LevelBatch {
            rows: time.items,
            time: time.bytes,
            columns: &self.buf[columns..self.oldest_end],
        })
    }

    /// Removes the bytes `start..end`, `entries` entries.
    fn remove_range(&mut self, start: usize, end: usize, entries: usize) {
        self.buf.copy_within(end..self.used, start);
        self.used -= end - start;
        self.entries -= entries;
    }

    /// Removes the oldest level batch (sent, or dropped), returns its samples (rows times columns).
    pub fn remove_oldest(&mut self) -> Option<u64> {
        if self.batches == 0 {
            return None;
        }
        let end = self.oldest_end;
        let mut samples = 0;
        let mut entries = 0;
        for (_, e, _) in self.entries_in(0, end) {
            entries += 1;
            if e.k != TIME {
                samples += e.items as u64;
            }
        }
        self.remove_range(0, end, entries);
        self.batches -= 1;
        // the next batch ends at the time column after its own, or at the end
        self.oldest_end = match self.batches {
            0 => 0,
            _ => self
                .entries()
                .skip(1)
                .find(|(_, e, _)| e.k == TIME)
                .map_or(self.used, |(at, _, _)| at),
        };
        Some(samples)
    }

    /// Keeps the columns of the slots `keep` says, drops the others (no longer observed in `Samples` mode: not
    /// missed) and batches left without a column.
    pub fn retain_columns(&mut self, keep: impl Fn(u16) -> bool) {
        let mut off = 0;
        while off < self.used {
            let (e, next) = entry_at(&self.buf[..self.used], off);
            if e.k != TIME && !keep(e.k) {
                self.remove_range(off, next, 1);
            } else {
                off = next;
            }
        }
        // a time column followed by another one (or nothing) is a batch without columns
        let mut off = 0;
        while off < self.used {
            let (e, next) = entry_at(&self.buf[..self.used], off);
            let alone = e.k == TIME
                && (next >= self.used || entry_at(&self.buf[..self.used], next).0.k == TIME);
            if alone {
                self.remove_range(off, next, 1);
            } else {
                off = next;
            }
        }
        self.reindex();
    }

    /// Drops everything, returns the samples dropped.
    pub fn clear(&mut self) -> u64 {
        let samples = self
            .entries()
            .filter(|(_, e, _)| e.k != TIME)
            .map(|(_, e, _)| e.items as u64)
            .sum();
        self.used = 0;
        self.entries = 0;
        self.batches = 0;
        self.oldest_end = 0;
        samples
    }

    /// Bytes free for data with `entries` more entries.
    fn free(&self, entries: usize) -> usize {
        BYTES.saturating_sub(self.used + entries * OUTBOX_ENTRY_HEADER)
    }

    /// Encodes the sealed batches of `batcher` (a level's rows: a [Consumer](super::Consumer) of a
    /// [Batcher::new_rows](super::Batcher::new_rows)) as level batches: the time column and the value columns in
    /// `columns`, column `c` as the entries of slot `slots[c]`. Room is made by dropping batches that were here
    /// before (oldest first) where a batch doesn't fit; batches added by this call are never dropped by it. A batch
    /// that doesn't fit the room there is is split: the most rows that fit go now, the rest stays sealed in the
    /// batcher. A batch whose worst case fits is encoded right away, else it is written into the room there is and
    /// counted only when it didn't fit.
    pub fn push_rows_from<R, const N: usize, S, const B: usize>(
        &mut self,
        batcher: &mut S,
        columns: Columns,
        slots: &[u16],
        scratch: &mut Scratch<B>,
    ) -> Result<OutboxPush, Error>
    where
        R: LevelRow,
        S: RowsSource<R, N>,
    {
        let columns =
            columns & Self::all_columns(R::COLUMNS.min(slots.len())) & batcher.held_columns();
        let entries = 1 + columns.count_ones() as usize;
        let policy = batcher.rows_policy();
        let mut done = OutboxPush::default();
        let mut droppable = self.len();
        loop {
            let mut never_fits = false;
            let pushed = batcher.encode_rows(|store, rows| {
                let total = rows.len();
                // the bytes of the first `m` rows
                let exact = |m: usize, scratch: &mut Scratch<B>| -> Result<usize, Error> {
                    let mut bytes = R::encode_column::<S::Policy, N, B>(
                        store,
                        rows.start..rows.start + m,
                        None,
                        policy,
                        scratch,
                        None,
                    )?;
                    for c in Self::columns_in(columns) {
                        bytes += R::encode_column::<S::Policy, N, B>(
                            store,
                            rows.start..rows.start + m,
                            Some(c),
                            policy,
                            scratch,
                            None,
                        )?;
                    }
                    Ok(bytes)
                };
                let mut m = total;
                if R::max_bytes(total, columns) > self.free(entries) {
                    // it may not fit as it is: written into the free room first, measured only if it didn't
                    if let Ok(bytes) = self.write_level::<R, S::Policy, N, B>(
                        store,
                        rows.clone(),
                        columns,
                        slots,
                        policy,
                        scratch,
                    ) {
                        return Ok(Some((total, bytes, ())));
                    }
                    let need = exact(total, scratch)?;
                    while self.free(entries) < need && droppable > 0 {
                        done.dropped += self.remove_oldest().unwrap_or(0);
                        droppable -= 1;
                    }
                    if need > self.free(entries) {
                        // the most rows that fit: `lo` do, `hi` don't
                        let (mut lo, mut hi) = (0, total);
                        while hi - lo > 1 {
                            let mid = lo + (hi - lo) / 2;
                            if exact(mid, scratch)? <= self.free(entries) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        if lo == 0 {
                            // nothing older is left to drop: one row is bigger than the outbox, or what this call
                            // added fills it and the rest waits
                            never_fits = self.used == 0;
                            return Ok(None);
                        }
                        m = lo;
                    }
                }
                let bytes = self.write_level::<R, S::Policy, N, B>(
                    store,
                    rows.start..rows.start + m,
                    columns,
                    slots,
                    policy,
                    scratch,
                )?;
                Ok(Some((m, bytes, ())))
            });
            match pushed {
                Ok(Some(rows)) => {
                    done.batches += 1;
                    done.rows += rows.encoded.items;
                    done.lost += rows.encoded.lost_before;
                }
                Ok(None) if never_fits => {
                    // one row is bigger than the whole outbox: it can never be sent
                    let (rows, lost_before) = batcher.discard_sealed();
                    done.lost += rows as u64 + lost_before;
                }
                // nothing sealed, or the rest waits for room
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    /// Writes rows `rows` of `store` as one level batch after the others: the time column and the columns in
    /// `columns`, each `Series` straight into the buffer. Returns the bytes of the columns. On an error (one that
    /// didn't fit the room left, or a value that doesn't serialize) nothing is added.
    fn write_level<R: LevelRow, P: SeriesPolicy + Copy, const N: usize, const B: usize>(
        &mut self,
        store: &R::Store<N>,
        rows: Range<usize>,
        columns: Columns,
        slots: &[u16],
        policy: P,
        scratch: &mut Scratch<B>,
    ) -> Result<usize, Error> {
        let (used, entries) = (self.used, self.entries);
        let mut bytes = 0;
        for column in core::iter::once(None).chain(Self::columns_in(columns).map(Some)) {
            let k = column.map_or(TIME, |c| slots[c]);
            let start = self.used + OUTBOX_ENTRY_HEADER;
            let written = match self.buf.get_mut(start..) {
                Some(buf) => R::encode_column::<P, N, B>(
                    store,
                    rows.clone(),
                    column,
                    policy,
                    scratch,
                    Some(buf),
                ),
                None => Err(Error::OutOfBoundsWriteU8),
            };
            let len = match written {
                Ok(len) => len,
                Err(e) => {
                    self.used = used;
                    self.entries = entries;
                    return Err(e);
                }
            };
            let h = &mut self.buf[self.used..start];
            h[0..2].copy_from_slice(&k.to_le_bytes());
            h[2..4].copy_from_slice(&(rows.len() as u16).to_le_bytes());
            h[4..6].copy_from_slice(&(len as u16).to_le_bytes());
            self.used = start + len;
            self.entries += 1;
            bytes += len;
        }
        self.batches += 1;
        if self.batches == 1 {
            self.oldest_end = self.used;
        }
        Ok(bytes)
    }

    const fn all_columns(n: usize) -> Columns {
        if n >= MAX_COLUMNS {
            Columns::MAX
        } else {
            (1 << n) - 1
        }
    }

    fn columns_in(columns: Columns) -> impl Iterator<Item = usize> + Clone {
        (0..MAX_COLUMNS).filter(move |c| columns & (1 << c) != 0)
    }
}

impl<const BYTES: usize> Default for Outbox<BYTES> {
    fn default() -> Self {
        Self::new()
    }
}
