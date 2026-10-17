//! Batched streams: the consumer side for a [Reliable] and a [Counted] stream, and [Batched] to keep a stream and
//! its batcher together.

use super::{Batcher, Consumer, EncodeError, Producer, Scratch, Seal};
use crate::StreamHead;
use crate::store::StreamStore;
use crate::tx::{Counted, Pushed, Reliable};
use wire_weaver::shrink_wrap::Error;
use wire_weaver::shrink_wrap::series::{CodecPolicy, SeriesColumns, SeriesStore};

/// A stream whose items are batched: the delivery state `S` ([Reliable], [Counted]) and the [Batcher] of its items
/// (`Timed<T>` for a timed stream). Events carry one batch each, `units` = its items, no head `t`.
/// [split](Self::split) it for a producer in another context.
pub struct Batched<S, T: SeriesStore, const N: usize> {
    pub stream: S,
    pub batcher: Batcher<T, N>,
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

impl<S, T, const N: usize> Batched<S, T, N>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
{
    pub fn new(stream: S, policy: CodecPolicy) -> Self {
        Batched {
            stream,
            batcher: Batcher::new(policy),
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
    pub fn split(&mut self) -> (Producer<'_, T, N>, Consumer<'_, T, N>, &mut S) {
        let (producer, consumer) = self.batcher.split();
        (producer, consumer, &mut self.stream)
    }
}

impl<St, const K: usize, T, const N: usize> Batched<Reliable<St, K>, T, N>
where
    St: StreamStore,
    T: SeriesStore + for<'a> SeriesColumns<'a>,
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

impl<T, const N: usize> Batched<Counted, T, N>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
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

impl<T, const N: usize> Consumer<'_, T, N>
where
    T: SeriesStore + for<'a> SeriesColumns<'a>,
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
}
