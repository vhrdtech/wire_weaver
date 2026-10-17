//! [Batcher]: the two stores, and the hand-off between its [Producer] and its [Consumer].

use super::{BatchStats, Cause, EncodeError, Encoded, Scratch, Seal};
use core::cell::UnsafeCell;
use portable_atomic::{AtomicU32, Ordering};
use wire_weaver::shrink_wrap::series::{
    CodecPolicy, SERIES_MAX_ITEMS, Series, SeriesColumns, SeriesStore, bound,
};
use wire_weaver::shrink_wrap::{Error, SerializeShrinkWrap};

/// Most items a batch can hold: the item counts share one atomic word.
pub const MAX_BATCH: usize = (1 << COUNT_BITS) - 1;

const COUNT_BITS: u32 = 13;
const COUNT_MASK: u32 = (1 << COUNT_BITS) - 1;
/// Index of the store being filled
const FILL: u32 = 1;
/// Items in the filling store
const N_SHIFT: u32 = 1;
/// Items in the spare store, sealed and the consumer's; 0: the spare is free
const SEALED_SHIFT: u32 = N_SHIFT + COUNT_BITS;
/// [Cause] of the sealed batch
const CAUSE_SHIFT: u32 = SEALED_SHIFT + COUNT_BITS;
const CAUSE_MASK: u32 = 0b11 << CAUSE_SHIFT;
/// The producer is writing an item into the filling store: the consumer doesn't seal it meanwhile
const BUSY: u32 = 1 << (CAUSE_SHIFT + 2);
const SPARE_MASK: u32 = (COUNT_MASK << SEALED_SHIFT) | CAUSE_MASK;

/// The atomic hand-off word, see the [module docs](super).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State(u32);

impl State {
    const fn fill(self) -> usize {
        (self.0 & FILL) as usize
    }

    const fn n(self) -> usize {
        ((self.0 >> N_SHIFT) & COUNT_MASK) as usize
    }

    const fn sealed(self) -> usize {
        ((self.0 >> SEALED_SHIFT) & COUNT_MASK) as usize
    }

    const fn cause(self) -> Cause {
        match (self.0 & CAUSE_MASK) >> CAUSE_SHIFT {
            0 => Cause::Full,
            1 => Cause::Budget,
            2 => Cause::Age,
            _ => Cause::Flush,
        }
    }

    const fn busy(self) -> bool {
        self.0 & BUSY != 0
    }

    /// `n` items in the filling store.
    const fn with_n(self, n: usize) -> State {
        State((self.0 & !(COUNT_MASK << N_SHIFT)) | ((n as u32) << N_SHIFT))
    }

    /// The filling store's `n` items sealed for `cause`, the spare (free) filling from 0.
    const fn swapped(self, n: usize, cause: Cause) -> State {
        State(
            ((self.0 & FILL) ^ FILL)
                | ((n as u32) << SEALED_SHIFT)
                | ((cause as u32) << CAUSE_SHIFT)
                | (self.0 & BUSY),
        )
    }
}

/// Collects items of `T` into batches of up to `N`, see the [module docs](super). [split](Self::split) it into a
/// [Producer] and a [Consumer] for a producer in another context (an interrupt, another core); used from one task,
/// its own methods are those of both.
pub struct Batcher<T: SeriesStore, const N: usize> {
    stores: [UnsafeCell<T::Store<N>>; 2],
    state: AtomicU32,
    /// Items lost right before the next batch of each store: added by the producer while it fills the store, taken
    /// by the consumer with that store's batch (or on its own when nothing is sealed)
    lost_before: [AtomicU32; 2],
    /// `n` at which the producer seals for the budget, worked out by the consumer: at most `N`, at least 1
    limit: AtomicU32,
    consumer: UnsafeCell<ConsumerState>,
}

// SAFETY: the stores are handed between the producer and the consumer through `state` (see `Producer::push` and
// `Consumer`): each store is written by one side at a time, and the consumer state only by the one `Consumer`.
unsafe impl<T: SeriesStore, const N: usize> Sync for Batcher<T, N> where T::Store<N>: Send {}

/// What only the consumer side keeps.
struct ConsumerState {
    policy: CodecPolicy,
    budget: usize,
    /// Limit of the worst case at the budget
    fresh_limit: usize,
    /// (items, bytes) of the last batch encoded whole
    last: Option<(usize, usize)>,
    /// The filling batch as last seen: store, items, when its first item was seen
    seen: Option<(usize, usize, u64)>,
    taken: Option<Taken>,
    stats: BatchStats,
}

/// The sealed batch the consumer took: items `start..end` of `store` still to encode.
#[derive(Debug, Clone, Copy)]
struct Taken {
    store: usize,
    start: usize,
    end: usize,
    cause: Cause,
    lost_before: u64,
    /// Bytes of the parts encoded so far
    bytes: usize,
}

/// The pushing half of a [Batcher], see [Batcher::split]. One per batcher; `push` takes `&mut self`.
pub struct Producer<'a, T: SeriesStore, const N: usize>(&'a Batcher<T, N>);

/// The encoding half of a [Batcher], see [Batcher::split]: the event loop's.
pub struct Consumer<'a, T: SeriesStore, const N: usize>(&'a Batcher<T, N>);

/// What [Consumer::poll] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Polled {
    /// It sealed the filling batch (age, a smaller budget).
    pub sealed: Option<Cause>,
    /// Items the producer lost with nothing sealed after them yet: report them now (no batch comes before them).
    pub lost: u64,
}

impl<T, const N: usize> Batcher<T, N>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
{
    const ITEM_BITS: usize = match T::MAX_ITEM_BITS {
        Some(bits) => bits,
        None => panic!("a batched type needs a series size bound (no strings or opaque fields)"),
    };
    const FIXED_BITS: usize = match T::MAX_FIXED_BITS {
        Some(bits) => bits,
        None => panic!("a batched type needs a series size bound (no strings or opaque fields)"),
    };

    /// Empty, encoding with `policy` ([DEFAULT_POLICY](super::DEFAULT_POLICY) usually), no budget until the first
    /// [poll](Consumer::poll).
    pub fn new(policy: CodecPolicy) -> Self {
        const {
            assert!(
                N >= 1 && N <= MAX_BATCH && N <= SERIES_MAX_ITEMS,
                "batch size out of 1..=8191"
            );
            // evaluated here, so that an unbounded `T` is a compile error at the first use
            let _ = Self::ITEM_BITS + Self::FIXED_BITS;
        }
        Batcher {
            stores: [
                UnsafeCell::new(T::empty_store::<N>()),
                UnsafeCell::new(T::empty_store::<N>()),
            ],
            state: AtomicU32::new(0),
            lost_before: [AtomicU32::new(0), AtomicU32::new(0)],
            limit: AtomicU32::new(N as u32),
            consumer: UnsafeCell::new(ConsumerState {
                policy,
                budget: usize::MAX,
                fresh_limit: N,
                last: None,
                seen: None,
                taken: None,
                stats: BatchStats::default(),
            }),
        }
    }

    /// [new](Self::new) with an event budget set before the first poll.
    pub fn with_budget(policy: CodecPolicy, budget: usize) -> Self {
        let mut batcher = Self::new(policy);
        batcher.consumer().set_budget(budget);
        batcher
    }

    /// The two halves, to push from one context and encode in another.
    pub fn split(&mut self) -> (Producer<'_, T, N>, Consumer<'_, T, N>) {
        (Producer(self), Consumer(self))
    }

    /// The pushing half alone.
    pub fn producer(&mut self) -> Producer<'_, T, N> {
        Producer(self)
    }

    /// The encoding half alone.
    pub fn consumer(&mut self) -> Consumer<'_, T, N> {
        Consumer(self)
    }

    /// [Producer::push].
    #[inline]
    pub fn push(&mut self, item: &T) -> Seal {
        self.produce(Some(item), None)
    }

    /// [Producer::push_many].
    pub fn push_many(&mut self, items: impl IntoIterator<Item = T>) -> (Seal, usize) {
        self.producer().push_many(items)
    }

    /// [Producer::flush].
    pub fn flush(&mut self) -> Seal {
        self.produce(None, Some(Cause::Flush))
    }

    /// Items in the filling batch.
    pub fn filling(&self) -> usize {
        State(self.state.load(Ordering::Relaxed)).n()
    }

    pub fn policy(&mut self) -> CodecPolicy {
        self.consumer().state().policy
    }

    pub fn stats(&mut self) -> BatchStats {
        self.consumer().stats()
    }

    /// [Consumer::poll].
    pub fn poll(&mut self, now_us: u64, max_age_us: Option<u64>, budget: usize) -> Polled {
        self.consumer().poll(now_us, max_age_us, budget)
    }

    /// [Consumer::encode_into].
    pub fn encode_into<const B: usize>(
        &mut self,
        scratch: &mut Scratch<B>,
        buf: &mut [u8],
    ) -> Result<Option<Encoded>, EncodeError> {
        self.consumer().encode_into(scratch, buf)
    }

    /// [Consumer::has_sealed].
    pub fn has_sealed(&mut self) -> bool {
        self.consumer().has_sealed()
    }

    /// [Consumer::sealed_items].
    pub fn sealed_items(&mut self) -> usize {
        self.consumer().sealed_items()
    }

    /// [Consumer::sealed_bound].
    pub fn sealed_bound(&mut self) -> Option<usize> {
        self.consumer().sealed_bound()
    }

    /// [Consumer::sealed_len].
    pub fn sealed_len<const B: usize>(
        &mut self,
        scratch: &mut Scratch<B>,
    ) -> Result<Option<usize>, Error> {
        self.consumer().sealed_len(scratch)
    }

    /// [Consumer::discard_sealed].
    pub fn discard_sealed(&mut self) -> (usize, u64) {
        self.consumer().discard_sealed()
    }

    /// The producer's side of a push (`item`) or a flush (`force`). Only one caller at a time: [Producer] is one per
    /// batcher and takes `&mut self`, so does [Batcher::push].
    #[inline]
    fn produce(&self, item: Option<&T>, force: Option<Cause>) -> Seal {
        // Acquire: a store the consumer gave back is complete before it is filled again
        let mut s = State(self.state.fetch_or(BUSY, Ordering::Acquire) | BUSY);
        let f = s.fill();
        let mut n = s.n();
        if let Some(item) = item {
            // SAFETY: the filling store is the producer's alone: the consumer only reads a store after it was sealed
            // (here, or by the consumer itself while the producer isn't BUSY), and only one producer runs
            T::set::<N>(unsafe { &mut *self.stores[f].get() }, n, item);
            n += 1;
        }
        let limit = self.limit.load(Ordering::Relaxed) as usize;
        loop {
            // the consumer may only have given the spare back meanwhile: fill and n are as read
            let (next, seal) = if n == 0 || (n < limit && force.is_none()) {
                (s.with_n(n), Seal::No)
            } else {
                let cause = match force {
                    Some(cause) => cause,
                    None if n >= N => Cause::Full,
                    None => Cause::Budget,
                };
                if s.sealed() == 0 {
                    (s.swapped(n, cause), Seal::Sealed(cause))
                } else {
                    (s.with_n(0), Seal::Lost { cause, items: n })
                }
            };
            // Release: the items are written before the consumer sees the sealed store
            match self.state.compare_exchange_weak(
                s.0,
                next.0 & !BUSY,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if let Seal::Lost { items, .. } = seal {
                        // before the next push publishes anything of this store
                        self.lost_before[f].fetch_add(items as u32, Ordering::Relaxed);
                    }
                    return seal;
                }
                Err(current) => s = State(current),
            }
        }
    }

    /// Most items the worst case allows in `budget` bytes, clamped to `1..=N`.
    fn worst_case_limit(budget: usize) -> usize {
        let used = Self::FIXED_BITS + bound::nib_bits(N as u32);
        let room = budget.saturating_mul(8).saturating_sub(used);
        room.checked_div(Self::ITEM_BITS).unwrap_or(N).clamp(1, N)
    }
}

impl<'a, T, const N: usize> Producer<'a, T, N>
where
    T: SeriesStore + for<'x> SeriesColumns<'x>,
{
    /// Adds `item` to the filling batch: a copy of its fields and two atomic operations, never an encode or a
    /// wait. Seals the batch when it is full or at the budget, see [Seal].
    #[inline]
    pub fn push(&mut self, item: &T) -> Seal {
        self.0.produce(Some(item), None)
    }

    /// [push](Self::push) of every item, for a producer that gets blocks (an ADC DMA half-transfer). Returns the
    /// last seal and the items lost on the way.
    pub fn push_many(&mut self, items: impl IntoIterator<Item = T>) -> (Seal, usize) {
        let mut last = Seal::No;
        let mut lost = 0;
        for item in items {
            match self.push(&item) {
                Seal::No => {}
                seal => {
                    if let Seal::Lost { items, .. } = seal {
                        lost += items;
                    }
                    last = seal;
                }
            }
        }
        (last, lost)
    }

    /// Seals the filling batch now, if it has items (the producer stops for a while).
    pub fn flush(&mut self) -> Seal {
        self.0.produce(None, Some(Cause::Flush))
    }

    /// Items in the filling batch.
    pub fn filling(&self) -> usize {
        State(self.0.state.load(Ordering::Relaxed)).n()
    }
}

/// Where a [Batched](super::Batched) stream takes its consumer half from: a [Batcher] it owns, or a [Consumer]
/// split off a batcher whose [Producer] runs elsewhere.
pub trait BatchSource<T: SeriesStore, const N: usize> {
    fn consumer(&mut self) -> Consumer<'_, T, N>;
}

impl<T, const N: usize> BatchSource<T, N> for Batcher<T, N>
where
    T: SeriesStore + for<'x> SeriesColumns<'x>,
{
    fn consumer(&mut self) -> Consumer<'_, T, N> {
        Consumer(self)
    }
}

impl<T: SeriesStore, const N: usize> BatchSource<T, N> for Consumer<'_, T, N> {
    fn consumer(&mut self) -> Consumer<'_, T, N> {
        // the one consumer, borrowed: `&mut self` keeps it the only one meanwhile
        Consumer(self.0)
    }
}

/// Something items can be pushed into in O(1): a [Producer], a [Batcher] or a [Batched](super::Batched) stream
/// that owns its batcher. Generated `<stream>_push` takes any of them.
pub trait Push<T> {
    /// See [Producer::push].
    fn push(&mut self, item: &T) -> Seal;
}

impl<T, const N: usize> Push<T> for Producer<'_, T, N>
where
    T: SeriesStore + for<'x> SeriesColumns<'x>,
{
    #[inline]
    fn push(&mut self, item: &T) -> Seal {
        self.0.produce(Some(item), None)
    }
}

impl<T, const N: usize> Push<T> for Batcher<T, N>
where
    T: SeriesStore + for<'x> SeriesColumns<'x>,
{
    #[inline]
    fn push(&mut self, item: &T) -> Seal {
        self.produce(Some(item), None)
    }
}

impl<'a, T, const N: usize> Consumer<'a, T, N>
where
    T: SeriesStore + for<'x> SeriesColumns<'x>,
{
    fn state(&mut self) -> &mut ConsumerState {
        // SAFETY: only the one `Consumer` (or `&mut Batcher`, which can't coexist with it) touches it
        unsafe { &mut *self.0.consumer.get() }
    }

    pub fn stats(&mut self) -> BatchStats {
        self.state().stats
    }

    /// Items in the filling batch.
    pub fn filling(&self) -> usize {
        State(self.0.state.load(Ordering::Relaxed)).n()
    }

    /// The bytes a batch may take: the producer seals at the most items the estimate allows.
    pub fn set_budget(&mut self, budget: usize) {
        let st = self.state();
        if budget == st.budget {
            return;
        }
        st.budget = budget;
        st.fresh_limit = Batcher::<T, N>::worst_case_limit(budget);
        self.publish_limit();
    }

    /// The limit from the worst case, or from the last batch's bytes per item with a margin (a batch aimed at 80 %
    /// of the budget), whichever allows more.
    fn publish_limit(&mut self) {
        let st = self.state();
        let measured = match st.last {
            Some((items, bytes)) if bytes > 0 => {
                (st.budget.saturating_mul(4).saturating_mul(items) / bytes.saturating_mul(5)).min(N)
            }
            _ => N,
        };
        let limit = if st.last.is_some() {
            measured.max(st.fresh_limit)
        } else {
            st.fresh_limit
        }
        .clamp(1, N);
        self.0.limit.store(limit as u32, Ordering::Relaxed);
    }

    /// The event loop's turn, before [encode_into](Self::encode_into): sets the `budget`, seals the filling batch
    /// when it is `max_age_us` old (counted from the first poll that saw it) or past a smaller budget (unless the
    /// spare isn't back yet, or the producer is writing just now: then at the next poll), and reports losses that
    /// no batch carries.
    pub fn poll(&mut self, now_us: u64, max_age_us: Option<u64>, budget: usize) -> Polled {
        self.set_budget(budget);
        let mut polled = Polled::default();
        self.take();
        let s = State(self.0.state.load(Ordering::Acquire));
        let spare_free = s.sealed() == 0 && self.state().taken.is_none();
        if spare_free {
            // everything sealed before is done: what the producer lost since comes before its next batch
            let lost = self.0.lost_before[s.fill()].swap(0, Ordering::Relaxed) as u64;
            self.state().stats.lost += lost;
            polled.lost = lost;
        }
        if s.n() == 0 {
            self.state().seen = None;
            return polled;
        }
        let first_us = match self.state().seen {
            // the same batch as before (one dropped and refilled past it in between counts from the dropped one:
            // sealed early, never late)
            Some((store, n, us)) if store == s.fill() && s.n() >= n => us,
            _ => now_us,
        };
        self.state().seen = Some((s.fill(), s.n(), first_us));
        let limit = self.0.limit.load(Ordering::Relaxed) as usize;
        let cause = if max_age_us.is_some_and(|age| now_us.saturating_sub(first_us) >= age) {
            Cause::Age
        } else if s.n() >= limit {
            Cause::Budget
        } else {
            return polled;
        };
        if spare_free && !s.busy() && self.seal_filling(s, cause) {
            polled.sealed = Some(cause);
        }
        polled
    }

    /// Seals the filling batch now, if it has items, the spare is back and the producer isn't writing (a stream
    /// closing). Returns whether it did.
    pub fn flush(&mut self) -> bool {
        self.take();
        let s = State(self.0.state.load(Ordering::Acquire));
        s.n() > 0
            && s.sealed() == 0
            && self.state().taken.is_none()
            && !s.busy()
            && self.seal_filling(s, Cause::Flush)
    }

    /// Seals the filling store as read in `s` (not busy, spare free) and takes it.
    fn seal_filling(&mut self, s: State, cause: Cause) -> bool {
        // Acquire: the producer's items are written before the store is read
        if self
            .0
            .state
            .compare_exchange(
                s.0,
                s.swapped(s.n(), cause).0,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }
        self.state().seen = None;
        self.take();
        true
    }

    /// Takes the sealed store, if there is one and none is taken.
    fn take(&mut self) {
        if self.state().taken.is_some() {
            return;
        }
        let s = State(self.0.state.load(Ordering::Acquire));
        if s.sealed() == 0 {
            return;
        }
        let store = s.fill() ^ 1;
        let lost_before = self.0.lost_before[store].swap(0, Ordering::Relaxed) as u64;
        let st = self.state();
        st.stats.sealed[s.cause() as usize] += 1;
        st.stats.lost += lost_before;
        st.taken = Some(Taken {
            store,
            start: 0,
            end: s.sealed(),
            cause: s.cause(),
            lost_before,
            bytes: 0,
        });
    }

    /// Gives the taken store back to the producer.
    fn release(&mut self) {
        self.state().taken = None;
        // Release: done reading the store before the producer fills it again
        self.0.state.fetch_and(!SPARE_MASK, Ordering::Release);
    }

    /// A sealed batch waits for [encode_into](Self::encode_into).
    pub fn has_sealed(&mut self) -> bool {
        self.take();
        self.state().taken.is_some()
    }

    /// Items of the sealed batch still to encode, 0 without one.
    pub fn sealed_items(&mut self) -> usize {
        self.take();
        self.state().taken.map_or(0, |t| t.end - t.start)
    }

    /// The most bytes the sealed batch takes ([Series::max_bytes]), `None` without one.
    pub fn sealed_bound(&mut self) -> Option<usize> {
        self.take();
        self.state()
            .taken
            .and_then(|t| Series::<T>::max_bytes(t.end - t.start))
    }

    /// The taken store, to read.
    fn taken_store(&self, taken: &Taken) -> &T::Store<N> {
        // SAFETY: a sealed store is the consumer's until `release`; the producer writes only the filling one
        unsafe { &*self.0.stores[taken.store].get() }
    }

    /// The exact bytes of the sealed batch (a counting pass), `None` without one. For an
    /// [Outbox](super::Outbox) that has to make room first.
    pub fn sealed_len<const B: usize>(
        &mut self,
        scratch: &mut Scratch<B>,
    ) -> Result<Option<usize>, Error> {
        self.take();
        let Some(taken) = self.state().taken else {
            return Ok(None);
        };
        let policy = self.state().policy;
        let filled = scratch.fill::<T, N>(self.taken_store(&taken), taken.start..taken.end);
        Series::with_policy(filled.items(), policy)
            .encoded_len()
            .map(Some)
    }

    /// Encodes the sealed batch as a `Series<T>` at the start of `buf`; `None` without one. When it doesn't fit,
    /// the largest prefix that does is encoded and the rest stays (call again). The store goes back to the producer
    /// once the whole batch is encoded.
    pub fn encode_into<const B: usize>(
        &mut self,
        scratch: &mut Scratch<B>,
        buf: &mut [u8],
    ) -> Result<Option<Encoded>, EncodeError> {
        self.take();
        let Some(taken) = self.state().taken else {
            return Ok(None);
        };
        let policy = self.state().policy;
        let (items, len, split) = {
            let filled = scratch.fill::<T, N>(self.taken_store(&taken), taken.start..taken.end);
            encode_fitting(filled.items(), policy, buf)?
        };
        let st = self.state();
        st.stats.encoded += 1;
        st.stats.encoded_items += items as u64;
        st.stats.encoded_bytes += len as u64;
        st.stats.split += split as u64;
        let start = taken.start + items;
        let encoded = Encoded {
            items,
            len,
            lost_before: taken.lost_before,
            cause: taken.cause,
        };
        if start == taken.end {
            st.last = Some((taken.end, taken.bytes + len));
            self.release();
            self.publish_limit();
        } else {
            st.taken = Some(Taken {
                start,
                lost_before: 0,
                bytes: taken.bytes + len,
                ..taken
            });
        }
        Ok(Some(encoded))
    }

    /// Drops the sealed batch unsent (no host, no room ever): returns its items and those lost right before it,
    /// `(0, 0)` without one. The store goes back to the producer.
    pub fn discard_sealed(&mut self) -> (usize, u64) {
        self.take();
        let Some(taken) = self.state().taken else {
            return (0, 0);
        };
        let items = taken.end - taken.start;
        let st = self.state();
        st.stats.lost += items as u64;
        st.stats.discarded += items as u64;
        self.release();
        (items, taken.lost_before)
    }
}

/// `items` encoded at the start of `buf`, or the largest prefix that fits: (items, bytes, split).
fn encode_fitting<T>(
    items: &[T],
    policy: CodecPolicy,
    buf: &mut [u8],
) -> Result<(usize, usize, bool), EncodeError>
where
    T: for<'x> SeriesColumns<'x>,
{
    let m = items.len();
    match Series::with_policy(items, policy).to_ww_bytes(buf) {
        Ok(bytes) => Ok((m, bytes.len(), false)),
        Err(e) => {
            if Series::<T>::max_bytes(m).is_some_and(|max| max <= buf.len())
                || Series::with_policy(items, policy).encoded_len()? <= buf.len()
            {
                // it fits: the writer failed for another reason
                return Err(EncodeError::Write(e));
            }
            // the largest prefix that fits: `lo` items fit, `hi` don't
            let (mut lo, mut hi) = (0, m);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if Series::with_policy(&items[..mid], policy).encoded_len()? <= buf.len() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if lo == 0 {
                return Err(EncodeError::NoRoom);
            }
            let bytes = Series::with_policy(&items[..lo], policy).to_ww_bytes(buf)?;
            Ok((lo, bytes.len(), true))
        }
    }
}
