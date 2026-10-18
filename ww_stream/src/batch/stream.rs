//! Batched streams: the consumer side for a [Reliable], a [Counted] and a [Plain] stream, and [Batched] to keep a
//! stream and its batcher together.

use super::{BatchSource, Batcher, Consumer, EncodeError, Producer, Push, Scratch, Seal};
use crate::StreamHead;
use crate::store::StreamStore;
use crate::tx::{Counted, Pushed, Reliable};
use core::marker::PhantomData;
use wire_weaver::shrink_wrap::Error;
use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesPolicy, SeriesStore};

/// [Batched::max_age_us] when the host asks for none: 100 ms.
pub const DEFAULT_MAX_AGE_US: u32 = 100_000;
/// Least max age a host gets by default ([Batched::with_max_age]): 1 ms; a smaller wish is raised to it.
pub const MIN_MAX_AGE_US: u32 = 1_000;
/// Most max age a host gets by default: 10 s; a bigger wish is lowered to it.
pub const MAX_MAX_AGE_US: u32 = 10_000_000;

/// A stream whose items are batched: the delivery state `S` ([Reliable], [Counted], [Plain]) and the batcher of its
/// items (`Timed<T>` for a timed stream). Events carry one batch each, `units` = its items, no head `t`.
///
/// `Src` is where the consumer half comes from: the [Batcher] itself (the default, a producer in the same task pushes
/// through [Batched::push] or [split](Self::split)), or a [Consumer] whose [Producer] lives elsewhere, an interrupt
/// handler or another core, both split off a `&'static mut Batcher` ([Batched::with_consumer]).
///
/// It also keeps how old a batch may get before it is sent, as the host asked when it opened the stream, within
/// the device's bounds ([grant_max_age](Self::grant_max_age)).
pub struct Batched<S, T: SeriesStore, const N: usize, Src = Batcher<T, N>> {
    pub stream: S,
    pub batcher: Src,
    max_age: MaxAge,
    _item: PhantomData<fn(T)>,
}

/// Bounds and the granted value of a batched stream's max age, µs.
#[derive(Debug, Clone, Copy)]
struct MaxAge {
    min: u32,
    default: u32,
    max: u32,
    granted: u32,
}

/// Delivery state of a plain batched stream: batches are sent while a host has it open, dropped (and counted as
/// lost in the batcher's stats) while none has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plain {
    open: bool,
}

impl Plain {
    pub const fn new() -> Self {
        Plain { open: false }
    }

    /// A host opened the stream.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// The host closed it.
    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

/// What [Consumer::poll_reliable] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchedPoll {
    /// Batches put into the store.
    pub stored: usize,
    /// Items lost: the store had no room or no session, the producer was faster than the event loop, or one item is
    /// bigger than the budget.
    pub lost: u64,
}

/// Why [Consumer::poll_reliable] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchedError<E> {
    /// The series writer failed; the batch stays sealed.
    Write(Error),
    /// The store failed; the batch's items were counted as lost.
    Store(E),
}

const DEFAULT_BOUNDS: MaxAge = MaxAge {
    min: MIN_MAX_AGE_US,
    default: DEFAULT_MAX_AGE_US,
    max: MAX_MAX_AGE_US,
    granted: DEFAULT_MAX_AGE_US,
};

impl<S, T, const N: usize, P> Batched<S, T, N, Batcher<T, N, P>>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    P: SeriesPolicy + Copy,
{
    /// `policy` [DEFAULT_POLICY](super::DEFAULT_POLICY) usually, see [Batcher].
    pub fn new(stream: S, policy: P) -> Self {
        Batched {
            stream,
            batcher: Batcher::new(policy),
            max_age: DEFAULT_BOUNDS,
            _item: PhantomData,
        }
    }

    /// [Producer::push].
    #[inline]
    pub fn push(&mut self, item: &T) -> Seal {
        self.batcher.push(item)
    }

    /// [Producer::push_many].
    pub fn push_many(&mut self, items: impl IntoIterator<Item = T>) -> (Seal, usize) {
        self.batcher.push_many(items)
    }

    /// The batcher's halves and the stream, which goes with the consumer.
    pub fn split(&mut self) -> (Producer<'_, T, N, P>, Consumer<'_, T, N, P>, &mut S) {
        let (producer, consumer) = self.batcher.split();
        (producer, consumer, &mut self.stream)
    }
}

impl<'c, S, T, const N: usize, P> Batched<S, T, N, Consumer<'c, T, N, P>>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    P: SeriesPolicy + Copy,
{
    /// The consumer half of a batcher whose producer pushes from another context: both are
    /// [split](Batcher::split) off a batcher that outlives them (a `static`, a `StaticCell`).
    pub fn with_consumer(stream: S, consumer: Consumer<'c, T, N, P>) -> Self {
        Batched {
            stream,
            batcher: consumer,
            max_age: DEFAULT_BOUNDS,
            _item: PhantomData,
        }
    }
}

impl<S, T, const N: usize, Src> Batched<S, T, N, Src>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    Src: BatchSource<T, N>,
{
    /// Bounds of the max age a host may ask for, and what it gets when it asks for none, µs
    /// ([MIN_MAX_AGE_US], [DEFAULT_MAX_AGE_US], [MAX_MAX_AGE_US] otherwise). `min` is at least 1.
    pub fn with_max_age(mut self, min_us: u32, default_us: u32, max_us: u32) -> Self {
        let min = min_us.max(1);
        let max = max_us.max(min);
        let default = default_us.clamp(min, max);
        self.max_age = MaxAge {
            min,
            default,
            max,
            granted: default,
        };
        self
    }

    /// A host opened (or resumed) the stream asking for batches at most `requested` µs old (`None`: the default):
    /// the age within the bounds, which the poll seals batches at from now on and the host is told.
    pub fn grant_max_age(&mut self, requested: Option<u32>) -> u32 {
        let m = &mut self.max_age;
        m.granted = requested.map_or(m.default, |r| r.clamp(m.min, m.max));
        m.granted
    }

    /// The max age granted last, µs: a batch is sealed and sent once its first item is this old.
    pub fn max_age_us(&self) -> u32 {
        self.max_age.granted
    }

    /// The consumer half.
    pub fn consumer(&mut self) -> Consumer<'_, T, N, Src::Policy> {
        self.batcher.consumer()
    }
}

impl<S, T, const N: usize, P> Push<T> for Batched<S, T, N, Batcher<T, N, P>>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    P: SeriesPolicy + Copy,
{
    #[inline]
    fn push(&mut self, item: &T) -> Seal {
        self.batcher.push(item)
    }
}

impl<St, const K: usize, T, const N: usize, Src> Batched<Reliable<St, K>, T, N, Src>
where
    St: StreamStore,
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    Src: BatchSource<T, N>,
{
    /// [Consumer::poll_reliable].
    pub async fn poll<const B: usize>(
        &mut self,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &mut [u8],
    ) -> Result<BatchedPoll, BatchedError<St::Error>> {
        self.batcher
            .consumer()
            .poll_reliable(&mut self.stream, now_us, max_age_us, scratch, buf)
            .await
    }
}

impl<T, const N: usize, Src> Batched<Counted, T, N, Src>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    Src: BatchSource<T, N>,
{
    /// [Consumer::poll_counted].
    pub fn poll<'b, const B: usize>(
        &mut self,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &'b mut [u8],
    ) -> Result<Option<(StreamHead, &'b [u8])>, Error> {
        self.batcher
            .consumer()
            .poll_counted(&mut self.stream, now_us, max_age_us, scratch, buf)
    }
}

impl<T, const N: usize, Src> Batched<Plain, T, N, Src>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    Src: BatchSource<T, N>,
{
    /// [Consumer::poll_plain].
    pub fn poll<'b, const B: usize>(
        &mut self,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &'b mut [u8],
    ) -> Result<Option<(usize, &'b [u8])>, Error> {
        self.batcher
            .consumer()
            .poll_plain(&self.stream, now_us, max_age_us, scratch, buf)
    }
}

impl<T, const N: usize, P> Consumer<'_, T, N, P>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
    P: SeriesPolicy + Copy,
{
    /// The event loop's turn for a batched reliable stream: seals on age and budget, encodes sealed batches into
    /// `buf` and puts each into the store as one event (once; resends copy it). The budget is `buf`'s length and the
    /// store's [max_item_len](StreamStore::max_item_len), whichever is smaller; `buf` is free again afterwards (the
    /// one `poll_streams` reads events into will do). Without a session batches are dropped unencoded.
    pub async fn poll_reliable<St: StreamStore, const K: usize, const B: usize>(
        &mut self,
        stream: &mut Reliable<St, K>,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &mut [u8],
    ) -> Result<BatchedPoll, BatchedError<St::Error>> {
        let budget = buf.len().min(stream.store().max_item_len());
        let buf = &mut buf[..budget];
        let mut done = BatchedPoll::default();
        // a second round: a batch due while the spare was still out can be sealed once it is back
        for _ in 0..2 {
            let polled = self.poll(now_us, max_age_us, budget);
            if polled.lost > 0 {
                stream.lost(polled.lost);
                done.lost += polled.lost;
            }
            let mut any = polled.sealed.is_some();
            loop {
                if self.has_sealed() && stream.session().is_none() {
                    // nowhere to store it: not encoded for nothing
                    let (items, lost_before) = self.discard_sealed();
                    stream.lost_no_session(items as u64 + lost_before);
                    done.lost += items as u64 + lost_before;
                    any = true;
                    continue;
                }
                let enc = match self.encode_into(scratch, buf) {
                    Ok(Some(enc)) => enc,
                    Ok(None) => break,
                    Err(EncodeError::NoRoom) => {
                        let (items, lost_before) = self.discard_sealed();
                        let lost = items as u64 + lost_before;
                        stream.lost(lost);
                        done.lost += lost;
                        any = true;
                        continue;
                    }
                    Err(EncodeError::Write(e)) => return Err(BatchedError::Write(e)),
                };
                any = true;
                if enc.lost_before > 0 {
                    stream.lost(enc.lost_before);
                    done.lost += enc.lost_before;
                }
                match stream.push(&buf[..enc.len], enc.items as u64, None).await {
                    Ok(Pushed::Stored) => done.stored += 1,
                    Ok(Pushed::Lost) => done.lost += enc.items as u64,
                    Err(e) => return Err(BatchedError::Store(e)),
                }
            }
            if !any {
                break;
            }
        }
        Ok(done)
    }

    /// The event loop's turn for a batched counted stream: seals on age and budget (`buf`'s length), then encodes
    /// the next sealed batch into `buf` and returns its head and bytes, to send right away ([Counted::unsent] when
    /// that fails). Call again while it returns one. Without a host a sealed batch is dropped unencoded and counted
    /// as lost.
    pub fn poll_counted<'b, const B: usize>(
        &mut self,
        stream: &mut Counted,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &'b mut [u8],
    ) -> Result<Option<(StreamHead, &'b [u8])>, Error> {
        let polled = self.poll(now_us, max_age_us, buf.len());
        if polled.lost > 0 {
            stream.lost(polled.lost);
        }
        loop {
            if !self.has_sealed() {
                return Ok(None);
            }
            if !stream.is_attached() {
                let (items, lost_before) = self.discard_sealed();
                // counted as lost without a host, in order
                stream.lost(lost_before);
                let _ = stream.head(items as u64, None);
                continue;
            }
            match self.encode_into(scratch, buf) {
                Ok(Some(enc)) => {
                    if enc.lost_before > 0 {
                        stream.lost(enc.lost_before);
                    }
                    let head = stream.head(enc.items as u64, None);
                    return Ok(head.map(|head| (head, &buf[..enc.len])));
                }
                Ok(None) => return Ok(None),
                Err(EncodeError::NoRoom) => {
                    let (items, lost_before) = self.discard_sealed();
                    stream.lost(lost_before + items as u64);
                }
                Err(EncodeError::Write(e)) => return Err(e),
            }
        }
    }

    /// The event loop's turn for a batched plain stream: seals on age and budget (`buf`'s length), then encodes
    /// the next sealed batch into `buf` and returns its items and bytes, to send right away as stream data. Call
    /// again while it returns one. While no host has the stream open a sealed batch is dropped unencoded (counted
    /// in [stats](Consumer::stats) as lost); a plain stream reports no losses to the host.
    pub fn poll_plain<'b, const B: usize>(
        &mut self,
        stream: &Plain,
        now_us: u64,
        max_age_us: Option<u64>,
        scratch: &mut Scratch<B>,
        buf: &'b mut [u8],
    ) -> Result<Option<(usize, &'b [u8])>, Error> {
        // a plain stream has no positions to report the producer's losses in: they stay in the stats
        self.poll(now_us, max_age_us, buf.len());
        loop {
            if !self.has_sealed() {
                return Ok(None);
            }
            if !stream.is_open() {
                self.discard_sealed();
                continue;
            }
            match self.encode_into(scratch, buf) {
                Ok(Some(enc)) => return Ok(Some((enc.items, &buf[..enc.len]))),
                Ok(None) => return Ok(None),
                Err(EncodeError::NoRoom) => {
                    self.discard_sealed();
                }
                Err(EncodeError::Write(e)) => return Err(e),
            }
        }
    }
}
