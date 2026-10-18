//! Observe cells (API-39, design D8): see [RowCells].

use super::batcher::{Bound, Polled};
use super::level::{Columns, LevelRow, RowsEncoded};
use super::{BatchStats, Cause, DefaultPolicy, Encoded};
use crate::observe::{GateCursor, SampleGate};
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::Range;
use core::sync::atomic::{AtomicU32, Ordering, fence};
use wire_weaver::shrink_wrap::series::{SeriesCells, SeriesPolicy, bound};
use ww_si::Second;

/// A [LevelRow] that can live in [RowCells]: generated per level with the time column's cells.
pub trait LevelRowCells: LevelRow + SeriesCells {
    /// Puts the time of row `i`.
    fn put_time<const R: usize>(cells: &Self::Cells<R>, i: usize, t: &Second);
}

/// The ring of a cells level: `R` rows of `Row` as atomics, the row count and the level's gate, the writer's and
/// the reader's state. `const fn` [new](Self::new), so it can be a `static`; one writer and one reader at a time.
///
/// Observe cells (API-39, design D8): a trait level's rows written in place by one context (a control loop, an
/// interrupt) into a ring of atomics, [RowCells], and read by the event loop into one staging store, [Staged],
/// that is encoded and sent as the default path's batches are: same columns, same encoder, same outbox, same wire.
///
/// **Writer.** [RowCells::writer] hands out the one [RowWriter] (a claim, given back on drop). The generated
/// `<Level>Writer` puts each value into the current row with relaxed stores where it is computed (on a Cortex-M a
/// plain `str` per word), then [commit](RowWriter::commit)s the row with its time: the level's grid decides whether
/// it is taken (as `<Level>Sampler::sample` does), then the row count goes up between two release fences. No
/// read-modify-write, no lock, nothing on the stack but the values in registers.
///
/// **Reader.** [RowCells::reader] hands out the one [CellsReader]. The rows `[sent, head)` are copied column by
/// column into the staging store with relaxed loads, then the count is read again: rows the writer may have
/// written over meanwhile (it writes row `head` into the slot of row `head - R`) are dropped and counted as
/// lapped, never sent mixed (a seqlock over a ring). The staging store is sealed at `N` rows, at the budget or at
/// the host's age, as a [Batcher](super::Batcher) is, and the [Outbox](super::Outbox) encodes it from there.
///
/// **Ring size.** `R` rows hold what the writer commits between two `poll_properties` calls (the reader drains the
/// ring into the staging store every call): `R - 1` of them can wait, more are lapped. `R` may be smaller than `N`.
pub struct RowCells<Row: LevelRowCells, const R: usize> {
    rows: Row::Cells<R>,
    /// Rows committed (wrapping): the writer writes row `head` into slot `w_slot`
    head: AtomicU32,
    gate: SampleGate,
    /// The writer's own: the slot of row `head`, and its grid
    w_slot: AtomicU32,
    cursor: UnsafeCell<GateCursor>,
    /// The reader's own: rows read (wrapping), and the slot of row `sent`
    sent: AtomicU32,
    r_slot: AtomicU32,
    writer_claimed: portable_atomic::AtomicBool,
    reader_claimed: portable_atomic::AtomicBool,
}

// SAFETY: `cursor` is touched only by the one claimed `RowWriter` (`writer_claimed`); everything else is atomics.
unsafe impl<Row: LevelRowCells, const R: usize> Sync for RowCells<Row, R> {}

impl<Row: LevelRowCells, const R: usize> RowCells<Row, R> {
    /// An empty ring, sampling nothing until the server opens the gate.
    pub const fn new() -> Self {
        const {
            assert!(
                R >= 2 && R <= (1 << 30),
                "a cells ring holds 2 to 2^30 rows"
            );
        }
        RowCells {
            // SAFETY: `SeriesCells` requires all-zero bytes to be valid cells
            rows: unsafe { core::mem::zeroed() },
            head: AtomicU32::new(0),
            gate: SampleGate::new(),
            w_slot: AtomicU32::new(0),
            cursor: UnsafeCell::new(GateCursor::new()),
            sent: AtomicU32::new(0),
            r_slot: AtomicU32::new(0),
            writer_claimed: portable_atomic::AtomicBool::new(false),
            reader_claimed: portable_atomic::AtomicBool::new(false),
        }
    }

    /// The writer, if no one holds it: the context that has the values (a control loop, an interrupt) keeps it.
    pub fn writer(&self) -> Option<RowWriter<'_, Row, R>> {
        if self.writer_claimed.swap(true, Ordering::Acquire) {
            return None;
        }
        Some(RowWriter {
            cells: self,
            head: self.head.load(Ordering::Relaxed),
            slot: self.w_slot.load(Ordering::Relaxed) as usize,
            #[cfg(debug_assertions)]
            set: 0,
            _not_sync: PhantomData,
        })
    }

    /// The reader, if no one holds it: for `ObserveBatches` (the server's `poll_properties`).
    pub fn reader<const N: usize>(&self) -> Option<CellsReader<'_, Row, N>> {
        if self.reader_claimed.swap(true, Ordering::Acquire) {
            return None;
        }
        Some(CellsReader::Ref(self))
    }

    /// The rows' cells, for the generated setters.
    pub fn rows(&self) -> &Row::Cells<R> {
        &self.rows
    }

    /// The level's gate: open while a host observes one of its properties in `Samples` mode.
    pub fn gate(&self) -> &SampleGate {
        &self.gate
    }

    /// Starts the row count at `rows` (tests of its wrap).
    #[cfg(test)]
    pub(super) fn start_at(&self, rows: u32) {
        self.head.store(rows, Ordering::Relaxed);
        self.sent.store(rows, Ordering::Relaxed);
    }

    /// Rows committed so far (wrapping).
    pub fn committed(&self) -> u32 {
        self.head.load(Ordering::Relaxed)
    }
}

impl<Row: LevelRowCells, const R: usize> Default for RowCells<Row, R> {
    fn default() -> Self {
        Self::new()
    }
}

/// The one writer of a [RowCells]: the generated `<Level>Writer` puts values into [slot](Self::slot) and
/// [commits](Self::commit). `Send`, not `Sync` or `Clone`; dropping it gives it back.
pub struct RowWriter<'a, Row: LevelRowCells, const R: usize> {
    cells: &'a RowCells<Row, R>,
    head: u32,
    slot: usize,
    /// The columns set in this row (debug builds check every one is)
    #[cfg(debug_assertions)]
    set: u64,
    _not_sync: PhantomData<core::cell::Cell<()>>,
}

impl<'a, Row: LevelRowCells, const R: usize> RowWriter<'a, Row, R> {
    /// The rows' cells.
    #[inline(always)]
    pub fn rows(&self) -> &'a Row::Cells<R> {
        &self.cells.rows
    }

    /// The slot of the row being written.
    #[inline(always)]
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Notes that value column `column` was set in this row (debug builds; nothing in release builds).
    #[inline(always)]
    pub fn mark(&mut self, column: usize) {
        #[cfg(debug_assertions)]
        {
            self.set |= 1 << column;
        }
        let _ = column;
    }

    /// Whether a host observes the level in `Samples` mode: skip working out the values otherwise.
    #[inline]
    pub fn is_sampling(&self) -> bool {
        self.cells.gate.is_open()
    }

    /// Publishes the row with its device time `t`, when the level's gate takes it (a host observes the level in
    /// `Samples` mode and `t` is on its grid); else the row's slot is written again next time. Returns whether it
    /// was taken. Debug builds panic when a value of the row wasn't set.
    #[inline]
    pub fn commit(&mut self, t: Second) -> bool {
        // SAFETY: only the one claimed writer touches the cursor
        let cursor = unsafe { &mut *self.cells.cursor.get() };
        if !cursor.take(&self.cells.gate, &t) {
            return false;
        }
        #[cfg(debug_assertions)]
        {
            let all = if Row::COLUMNS >= 64 {
                u64::MAX
            } else {
                (1u64 << Row::COLUMNS) - 1
            };
            assert_eq!(
                self.set & all,
                all,
                "observe cells: a value of the row wasn't set before commit (columns set: {:#b})",
                self.set
            );
            self.set = 0;
        }
        Row::put_time::<R>(&self.cells.rows, self.slot, &t);
        // the row's values before the count that publishes it
        fence(Ordering::Release);
        self.head = self.head.wrapping_add(1);
        self.cells.head.store(self.head, Ordering::Relaxed);
        // the count before the next row's values: a reader that sees one of those sees the count too
        fence(Ordering::Release);
        self.slot = if self.slot + 1 == R { 0 } else { self.slot + 1 };
        self.cells.w_slot.store(self.slot as u32, Ordering::Relaxed);
        true
    }
}

impl<Row: LevelRowCells, const R: usize> Drop for RowWriter<'_, Row, R> {
    fn drop(&mut self) {
        self.cells.writer_claimed.store(false, Ordering::Release);
    }
}

/// What [CellsSource::read_into] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Read {
    /// Rows copied, from the store index given.
    pub rows: usize,
    /// Rows lost: written over by the writer before or while they were copied.
    pub lapped: u64,
}

/// The reader's side of a [RowCells] of any ring size, for a server that doesn't name `R`.
pub trait CellsSource<Row: LevelRowCells, const N: usize> {
    /// The level's gate.
    fn gate(&self) -> &SampleGate;

    /// Rows committed and not read yet (some may be lapped).
    fn waiting(&self) -> u32;

    /// Copies up to `max` committed rows not read yet into `store` from index `at`, oldest first.
    fn read_into(&self, store: &mut Row::Store<N>, at: usize, max: usize) -> Read;

    /// Skips every committed row not read yet, returns how many.
    fn skip_all(&self) -> u32;

    /// Gives the reader back.
    fn release(&self);
}

impl<Row: LevelRowCells, const R: usize, const N: usize> CellsSource<Row, N> for RowCells<Row, R> {
    fn gate(&self) -> &SampleGate {
        &self.gate
    }

    fn waiting(&self) -> u32 {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.sent.load(Ordering::Relaxed))
    }

    fn read_into(&self, store: &mut Row::Store<N>, at: usize, max: usize) -> Read {
        // rows `[head + 1 - R, head)` can be read: the writer writes row `head` over row `head - R`
        let safe = (R - 1) as u32;
        let head = self.head.load(Ordering::Acquire);
        let mut start = self.sent.load(Ordering::Relaxed);
        let mut slot = self.r_slot.load(Ordering::Relaxed) as usize;
        let mut waiting = head.wrapping_sub(start);
        let mut lapped = 0u32;
        if waiting > safe {
            lapped = waiting - safe;
            start = start.wrapping_add(lapped);
            slot = (slot + (lapped as usize % R)) % R;
            waiting = safe;
        }
        let n = (waiting as usize).min(max);
        // two runs where the ring wraps
        let first = n.min(R - slot);
        Row::copy_rows::<R, N>(&self.rows, slot, store, at, first);
        Row::copy_rows::<R, N>(&self.rows, 0, store, at + first, n - first);
        // the copies before the count read again: a value the writer wrote after a later count shows here
        fence(Ordering::Acquire);
        let head2 = self.head.load(Ordering::Relaxed);
        let behind = head2.wrapping_sub(start);
        let bad = (behind.saturating_sub(safe) as usize).min(n);
        if bad > 0 {
            // the oldest `bad` rows may be mixed: move the good ones down over them
            for k in bad..n {
                let row = Row::get::<N>(store, at + k);
                Row::set::<N>(store, at + k - bad, &row);
            }
            lapped += bad as u32;
        }
        self.sent
            .store(start.wrapping_add(n as u32), Ordering::Relaxed);
        self.r_slot
            .store(((slot + n) % R) as u32, Ordering::Relaxed);
        Read {
            rows: n - bad,
            lapped: lapped as u64,
        }
    }

    fn skip_all(&self) -> u32 {
        let head = self.head.load(Ordering::Acquire);
        let sent = self.sent.load(Ordering::Relaxed);
        let skipped = head.wrapping_sub(sent);
        let slot = self.r_slot.load(Ordering::Relaxed) as usize;
        self.sent.store(head, Ordering::Relaxed);
        self.r_slot.store(
            ((slot + skipped as usize % R) % R) as u32,
            Ordering::Relaxed,
        );
        skipped
    }

    fn release(&self) {
        self.reader_claimed.store(false, Ordering::Release);
    }
}

/// The one reader of a cells level, `R` erased: what `ObserveBatches` keeps. Dropping it gives it back.
pub enum CellsReader<'a, Row: LevelRowCells, const N: usize> {
    /// Cells in a `static` or owned by the caller.
    Ref(&'a (dyn CellsSource<Row, N> + Sync)),
    /// Cells shared through an `Arc`.
    #[cfg(feature = "std")]
    Shared(std::sync::Arc<dyn CellsSource<Row, N> + Send + Sync>),
}

impl<Row: LevelRowCells, const N: usize> CellsReader<'_, Row, N> {
    /// The cells to read, for [Staged]'s calls.
    pub fn source(&self) -> &(dyn CellsSource<Row, N> + Sync) {
        match self {
            CellsReader::Ref(s) => *s,
            #[cfg(feature = "std")]
            CellsReader::Shared(s) => &**s,
        }
    }

    /// The level's gate.
    pub fn gate(&self) -> &SampleGate {
        self.source().gate()
    }
}

#[cfg(feature = "std")]
impl<Row: LevelRowCells + 'static, const N: usize> CellsReader<'static, Row, N> {
    /// The reader of cells shared through an `Arc` (a simulator that owns them, no `static`): the generated
    /// `<Level>Cells` or a [RowCells]. `None` when the reader is taken.
    pub fn shared<T, const R: usize>(cells: std::sync::Arc<T>) -> Option<Self>
    where
        T: AsRef<RowCells<Row, R>> + Send + Sync + 'static,
    {
        if (*cells)
            .as_ref()
            .reader_claimed
            .swap(true, Ordering::Acquire)
        {
            return None;
        }
        Some(CellsReader::Shared(std::sync::Arc::new(
            SharedCells::<T, R>(cells),
        )))
    }
}

/// [CellsReader::shared]'s adapter.
#[cfg(feature = "std")]
struct SharedCells<T, const R: usize>(std::sync::Arc<T>);

#[cfg(feature = "std")]
impl<Row, T, const R: usize, const N: usize> CellsSource<Row, N> for SharedCells<T, R>
where
    Row: LevelRowCells + 'static,
    T: AsRef<RowCells<Row, R>>,
{
    fn gate(&self) -> &SampleGate {
        &(*self.0).as_ref().gate
    }

    fn waiting(&self) -> u32 {
        CellsSource::<Row, N>::waiting((*self.0).as_ref())
    }

    fn read_into(&self, store: &mut Row::Store<N>, at: usize, max: usize) -> Read {
        (*self.0).as_ref().read_into(store, at, max)
    }

    fn skip_all(&self) -> u32 {
        CellsSource::<Row, N>::skip_all((*self.0).as_ref())
    }

    fn release(&self) {
        CellsSource::<Row, N>::release((*self.0).as_ref())
    }
}

impl<Row: LevelRowCells, const R: usize> AsRef<RowCells<Row, R>> for RowCells<Row, R> {
    fn as_ref(&self) -> &RowCells<Row, R> {
        self
    }
}

impl<Row: LevelRowCells, const N: usize> Drop for CellsReader<'_, Row, N> {
    fn drop(&mut self) {
        self.source().release();
    }
}

/// The batch of a cells level being filled from its ring or encoded: one store of `N` rows, see the
/// docs of [RowCells]. Sealed at `N` rows, at the budget (as a [Batcher](super::Batcher), from the worst case or
/// the last batch's bytes per row) or at the host's age.
pub struct Staged<Row: LevelRowCells, const N: usize, P = DefaultPolicy> {
    store: Row::Store<N>,
    /// Rows in the store
    n: usize,
    /// Rows `[start, n)` are sealed and still to encode
    sealed: Option<(usize, Cause)>,
    /// When the first row of the filling batch was first seen
    first_us: Option<u64>,
    policy: P,
    budget: usize,
    bound: Bound,
    limit: usize,
    /// (rows, bytes) of the last batch encoded whole
    last: Option<(usize, usize)>,
    /// Bytes of the sealed batch's parts encoded so far
    bytes: usize,
    stats: BatchStats,
}

impl<Row: LevelRowCells, const N: usize, P: SeriesPolicy + Copy> Staged<Row, N, P> {
    /// Empty, encoding with `policy` what its [CellsReader]'s [source](CellsReader::source) gives the calls.
    pub fn new(policy: P) -> Self {
        const {
            assert!(
                N >= 1 && N <= super::MAX_BATCH,
                "batch size out of 1..=8191"
            );
        }
        let bound = Row::bound(Columns::MAX);
        Staged {
            store: Row::empty_store::<N>(),
            n: 0,
            sealed: None,
            first_us: None,
            policy,
            budget: usize::MAX,
            bound,
            limit: N,
            last: None,
            bytes: 0,
            stats: BatchStats::default(),
        }
    }

    pub fn stats(&self) -> BatchStats {
        self.stats
    }

    /// The codec policy the columns are encoded with.
    pub fn policy(&self) -> P {
        self.policy
    }

    /// Rows in the store (filling or sealed).
    pub fn rows(&self) -> usize {
        self.n
    }

    fn set_budget_bound(&mut self, budget: usize, bound: Bound) {
        if budget == self.budget && bound == self.bound {
            return;
        }
        if bound != self.bound {
            self.last = None;
        }
        self.budget = budget;
        self.bound = bound;
        self.publish_limit();
    }

    /// As a [Batcher](super::Batcher) does: the worst case's limit, or the last batch's bytes per row aimed at
    /// 80 % of the budget, whichever allows more.
    fn publish_limit(&mut self) {
        let used = self.bound.fixed + self.bound.series * bound::nib_bits(N as u32);
        let room = self.budget.saturating_mul(8).saturating_sub(used);
        let fresh = room.checked_div(self.bound.item).unwrap_or(N).clamp(1, N);
        let limit = match self.last {
            Some((rows, bytes)) if bytes > 0 => fresh.max(
                (self.budget.saturating_mul(4).saturating_mul(rows) / bytes.saturating_mul(5))
                    .min(N),
            ),
            _ => fresh,
        };
        self.limit = limit.clamp(1, N);
    }

    /// The event loop's turn for a level in `Samples` mode: sets the `budget` against the worst case of the time
    /// and the `columns` observed, copies the rows waiting in the ring into the store (unless a sealed batch still
    /// waits for room), and seals it at `N` rows, at the budget or when its first row is `max_age_us` old. Rows the
    /// writer lapped come back as `lost`.
    pub fn poll_cells(
        &mut self,
        src: &dyn CellsSource<Row, N>,
        now_us: u64,
        max_age_us: Option<u64>,
        budget: usize,
        columns: Columns,
    ) -> Polled {
        self.set_budget_bound(budget, Row::bound(columns));
        let mut polled = Polled::default();
        if self.sealed.is_some() {
            return polled;
        }
        if self.n < self.limit {
            let read = src.read_into(&mut self.store, self.n, self.limit - self.n);
            self.n += read.rows;
            polled.lost = read.lapped;
            self.stats.lost += read.lapped;
        }
        if self.n == 0 {
            self.first_us = None;
            return polled;
        }
        let first = *self.first_us.get_or_insert(now_us);
        let cause = if self.n >= N {
            Cause::Full
        } else if self.n >= self.limit {
            Cause::Budget
        } else if max_age_us.is_some_and(|age| now_us.saturating_sub(first) >= age) {
            Cause::Age
        } else {
            return polled;
        };
        self.seal(cause);
        polled.sealed = Some(cause);
        polled
    }

    fn seal(&mut self, cause: Cause) {
        self.sealed = Some((0, cause));
        self.first_us = None;
        self.stats.sealed[cause as usize] += 1;
    }

    /// Takes the rows waiting in the ring (as many as fit) and seals the store now, if it has rows and none are
    /// sealed. Rows the writer lapped come back as `lost`.
    pub fn flush(&mut self, src: &dyn CellsSource<Row, N>) -> Polled {
        let mut polled = Polled::default();
        if self.sealed.is_some() {
            return polled;
        }
        let read = src.read_into(&mut self.store, self.n, N - self.n);
        self.n += read.rows;
        polled.lost = read.lapped;
        self.stats.lost += read.lapped;
        if self.n > 0 {
            self.seal(Cause::Flush);
            polled.sealed = Some(Cause::Flush);
        }
        polled
    }

    /// Nobody observes the level: drops the rows in the store and skips those in the ring. Returns the rows.
    pub fn discard_all(&mut self, src: &dyn CellsSource<Row, N>) -> u64 {
        let rows = self.n as u64 - self.sealed.map_or(0, |(start, _)| start as u64);
        self.stats.discarded += rows;
        self.n = 0;
        self.sealed = None;
        self.first_us = None;
        self.bytes = 0;
        rows + src.skip_all() as u64
    }

    /// Drops the sealed rows unsent (no room ever): returns them, and 0 (lapped rows are reported by
    /// [poll_cells](Self::poll_cells)).
    pub fn discard_sealed(&mut self) -> (usize, u64) {
        let Some((start, _)) = self.sealed else {
            return (0, 0);
        };
        let rows = self.n - start;
        self.stats.lost += rows as u64;
        self.stats.discarded += rows as u64;
        self.n = 0;
        self.sealed = None;
        self.bytes = 0;
        (rows, 0)
    }

    /// Encodes rows of the sealed batch with `encode`, as
    /// [Consumer::encode_rows](super::Consumer::encode_rows) does.
    pub fn encode_rows<O, E>(
        &mut self,
        encode: impl FnOnce(&Row::Store<N>, Range<usize>) -> Result<Option<(usize, usize, O)>, E>,
    ) -> Result<Option<RowsEncoded<O>>, E> {
        let Some((start, cause)) = self.sealed else {
            return Ok(None);
        };
        let rows = start..self.n;
        let Some((items, len, out)) = encode(&self.store, rows.clone())? else {
            return Ok(None);
        };
        let items = items.min(rows.len());
        let split = items < rows.len();
        self.stats.encoded += 1;
        self.stats.encoded_items += items as u64;
        self.stats.encoded_bytes += len as u64;
        self.stats.split += split as u64;
        if split {
            self.sealed = Some((start + items, cause));
            self.bytes += len;
        } else {
            self.last = Some((self.n, self.bytes + len));
            self.n = 0;
            self.sealed = None;
            self.bytes = 0;
            self.publish_limit();
        }
        Ok(Some(RowsEncoded {
            encoded: Encoded {
                items,
                len,
                lost_before: 0,
                cause,
            },
            out,
        }))
    }
}

impl<Row, const N: usize, P> super::RowsSource<Row, N> for Staged<Row, N, P>
where
    Row: LevelRowCells,
    P: SeriesPolicy + Copy,
{
    type Policy = P;

    fn rows_policy(&mut self) -> P {
        self.policy
    }

    fn encode_rows<O, E>(
        &mut self,
        encode: impl FnOnce(&Row::Store<N>, Range<usize>) -> Result<Option<(usize, usize, O)>, E>,
    ) -> Result<Option<RowsEncoded<O>>, E> {
        Staged::encode_rows(self, encode)
    }

    fn discard_sealed(&mut self) -> (usize, u64) {
        Staged::discard_sealed(self)
    }
}
