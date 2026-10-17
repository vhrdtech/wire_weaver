//! Host side of counted and reliable streams (CLT-23), see `docs/api/stream_reliability.md`.
//!
//! The event loop feeds every event of a stream into its [Shared] state: the `ww_stream` [Receiver] sorts it by
//! position (in order, gap, duplicate, late), accepted items go into a bounded queue, per run. User code takes them
//! out through a [PositionedStream] handle (or the two halves of [PositionedStream::split]); what it took (or
//! confirmed, with [AckMode::Explicit]) goes back into the receiver, whose `Ack` the event loop sends.

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::pin::pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tracing::{debug, warn};
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_client_server::{PathKindOwned, StreamSideband};
use ww_stream::rx::{Gaps, Receiver, Verdict};
use ww_stream::{Opened, StreamCursor, StreamHead};

use crate::event_loop::commander::TransportCommander;
use crate::{Error, StreamError};

pub use ww_si::Second;
pub use ww_stream::Run;
pub use ww_stream::rx::{AckMode, Cause, DEFAULT_WINDOW};

/// Default of [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes): bytes of items queued
/// per stream before plain and counted streams drop new ones.
pub const DEFAULT_STREAM_QUEUE_BYTES: usize = 4 << 20;

/// Event of a counted or reliable stream, see [PositionedStream::recv_event].
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent<T> {
    /// An item, at position `pos` (of its first unit) in its `run`, `units` long.
    ///
    /// When the device sends an item again from inside it (a resend or a resume from a position in the middle of a
    /// multi-unit item), `pos` and `units` cover only the units that are new to the host; `value` and `t` are the
    /// whole item's.
    Item {
        pos: u64,
        units: u64,
        /// Device time of the first unit, when the device sends one.
        t: Option<Second>,
        /// Rare markers: a new clock epoch after a reboot.
        ext: Option<ww_stream::HeadExt>,
        run: Run,
        value: T,
    },
    /// `units` from `at` on are missing in `run`, and whose loss it is.
    Gap {
        run: Run,
        at: u64,
        units: u64,
        cause: Cause,
    },
    /// The stream was opened again from `cursor` (a resume after a reconnect, or [PositionedStream] opened with a
    /// cursor): positions go on from there. `ended`: the cursor's session, when the device is in another one now
    /// (positions start over in `session`).
    Resumed {
        cursor: StreamCursor,
        session: u32,
        ended: Option<u32>,
    },
    /// The backfill run caught up with the live run: one run from here on.
    BackfillDone,
    /// The link went down. With `disconnect_keep_streams` the stream resumes by itself on the next connection
    /// ([StreamEvent::Resumed]), otherwise nothing comes after this.
    Disconnected,
    /// The device closed the stream.
    Closed,
}

/// What happened to a stream's data on its way to user code, see [PositionedStream::stats].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamStats {
    /// Units accepted.
    pub received: u64,
    /// Units the device dropped (producer overflow, store full, no host).
    pub lost_device: u64,
    /// Units lost on the link (counted streams; reliable ones get them resent).
    pub lost_link: u64,
    /// Units the device's store didn't keep for a resume.
    pub lost_store: u64,
    /// Units dropped because this stream's queue on the host was full (plain and counted streams).
    pub lost_host: u64,
    /// Events received twice.
    pub duplicates: u64,
    /// Events that arrived after their units were reported as a link gap.
    pub late: u64,
    /// Reliable: events dropped because something before them was missing (resent after).
    pub out_of_order: u64,
    /// Reliable: `ResendFrom` requests sent.
    pub resend_requests: u64,
    /// Events in the queue now, and their bytes.
    pub queued: usize,
    pub queued_bytes: usize,
}

/// Something queued for user code.
#[derive(Debug)]
enum Entry {
    Item {
        head: StreamHead,
        /// Units at the start of the item the host had already (see [Verdict::Accept])
        skip: u64,
        run: Run,
        bytes: Vec<u8>,
    },
    Gap(ww_stream::rx::Gap),
    Resumed {
        cursor: StreamCursor,
        session: u32,
        ended: Option<u32>,
    },
    BackfillDone,
    Disconnected,
    Closed,
}

impl Entry {
    fn bytes(&self) -> usize {
        match self {
            Entry::Item { bytes, .. } => bytes.len(),
            _ => 0,
        }
    }

    fn run(&self) -> Option<Run> {
        match self {
            Entry::Item { run, .. } => Some(*run),
            Entry::Gap(gap) => Some(gap.run),
            Entry::BackfillDone => Some(Run::Backfill),
            _ => None,
        }
    }
}

fn run_index(run: Run) -> usize {
    match run {
        Run::Live => 0,
        Run::Backfill => 1,
    }
}

/// How a positioned stream is set up on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositionedConfig {
    pub delivery: ww_self::Delivery,
    pub ack_mode: AckMode,
    /// Reliable: bytes the device may have in flight beyond what was acknowledged
    pub window: u32,
    /// Counted: bytes of items queued before new ones are dropped
    pub queue_bytes: usize,
    /// Batched: what the host asked as the most a batch may wait on the device, µs, asked again on a resume
    pub max_age_us: Option<u32>,
}

/// Receiving state of one counted or reliable stream, shared between the event loop and the user's handles.
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
    cond: Condvar,
}

pub(crate) struct Inner {
    pub(crate) path: Vec<wire_weaver::shrink_wrap::UNib32>,
    config: PositionedConfig,
    rx: Receiver,
    /// Live and backfill run, with a sequence number each for the order they arrived in
    queues: [VecDeque<(u64, Entry)>; 2],
    next_seq: u64,
    queued_bytes: usize,
    /// Split into a live and a backfill half: events of no run go into both queues
    split: bool,
    /// Units dropped since the last item that fit, per run (counted streams), reported as one gap
    host_gap: [Option<ww_stream::rx::Gap>; 2],
    lost_host: u64,
    /// An event loop feeds this stream (it is registered with a connected dispatcher)
    pub(crate) attached: bool,
    /// Every handle is gone, or closed: the event loop forgets the stream
    pub(crate) closed: bool,
    /// The device closed it
    device_closed: bool,
    /// User handles (two after a split): the last one dropped closes the stream
    handles: usize,
}

impl Shared {
    pub(crate) fn new(
        path: Vec<wire_weaver::shrink_wrap::UNib32>,
        config: PositionedConfig,
    ) -> Arc<Self> {
        let delivery = match config.delivery {
            ww_self::Delivery::Counted => ww_stream::rx::Delivery::Counted,
            ww_self::Delivery::Reliable => ww_stream::rx::Delivery::Reliable,
            // plain streams are opened as Stream<T>, never as positioned ones
            ww_self::Delivery::Plain => unreachable!("a plain stream has no positions"),
        };
        Arc::new(Shared {
            inner: Mutex::new(Inner {
                path,
                config,
                rx: Receiver::new(delivery)
                    .window(config.window)
                    .ack_mode(config.ack_mode),
                queues: [VecDeque::new(), VecDeque::new()],
                next_seq: 0,
                queued_bytes: 0,
                split: false,
                host_gap: [None, None],
                lost_host: 0,
                attached: false,
                closed: false,
                device_closed: false,
                handles: 1,
            }),
            notify: tokio::sync::Notify::new(),
            cond: Condvar::new(),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wake every waiting handle, after the event loop changed something.
    pub(crate) fn wake(&self) {
        self.notify.notify_waiters();
        self.cond.notify_all();
    }
}

impl Inner {
    pub(crate) fn is_reliable(&self) -> bool {
        self.config.delivery == ww_self::Delivery::Reliable
    }

    pub(crate) fn cursor(&self) -> Option<StreamCursor> {
        self.rx.cursor()
    }

    pub(crate) fn max_age_us(&self) -> Option<u32> {
        self.config.max_age_us
    }

    /// The device answered the open (or a resume from `cursor`).
    pub(crate) fn opened(&mut self, cursor: Option<StreamCursor>, opened: &Opened) {
        // positions may start over (another session): a drop still being counted ends here
        self.flush_host_gaps();
        let (gaps, ended) = self.rx.opened(cursor, opened);
        self.attached = true;
        if let Some(cursor) = cursor {
            self.push(Entry::Resumed {
                cursor,
                session: opened.session,
                ended,
            });
        }
        self.push_gaps(&gaps);
    }

    /// An event of this stream arrived.
    pub(crate) fn on_item(&mut self, head: StreamHead, data: &[u8]) {
        match self.rx.on_item(&head) {
            Verdict::Accept {
                run,
                gaps,
                backfill_done,
                skip,
            } => {
                self.push_gaps(&gaps);
                let reliable = self.is_reliable();
                let i = run_index(run);
                if !reliable && self.queued_bytes + data.len() > self.config.queue_bytes {
                    // plain drop-newest: reported as one gap once an item fits again
                    let gap = self.host_gap[i].get_or_insert(ww_stream::rx::Gap {
                        run,
                        at: head.pos + skip,
                        units: 0,
                        cause: Cause::Host,
                    });
                    gap.units = head.end() - gap.at;
                    self.lost_host += head.units - skip;
                } else {
                    if self.queued_bytes + data.len() > self.config.queue_bytes {
                        // a reliable device never has more in flight than the window
                        debug!("reliable stream {:?}: queue above its window", self.path);
                    }
                    if let Some(gap) = self.host_gap[i].take() {
                        warn!(
                            "stream {:?}: host queue full, dropped {} units at {}",
                            self.path, gap.units, gap.at
                        );
                        self.push(Entry::Gap(gap));
                    }
                    self.push(Entry::Item {
                        head,
                        skip,
                        run,
                        bytes: data.to_vec(),
                    });
                }
                if backfill_done {
                    self.push(Entry::BackfillDone);
                }
            }
            Verdict::Had {
                gaps,
                backfill_done,
            } => {
                self.push_gaps(&gaps);
                if backfill_done {
                    self.push(Entry::BackfillDone);
                }
            }
            Verdict::Duplicate | Verdict::Late | Verdict::OutOfOrder | Verdict::NotOpen => {}
        }
    }

    /// Sideband from the device.
    pub(crate) fn on_sideband(&mut self, sideband: StreamSideband) {
        match sideband {
            StreamSideband::Query { sent, backfill } => {
                let gaps = self.rx.on_query(sent.0, backfill.map(|b| b.0));
                self.push_gaps(&gaps);
            }
            StreamSideband::Close => {
                self.device_closed = true;
                self.rx.close();
                self.push(Entry::Closed);
            }
            _ => {}
        }
    }

    /// Sideband to send now (`Ack`, `ResendFrom`).
    pub(crate) fn poll(&mut self, now_ms: u64) -> Option<StreamSideband> {
        if self.closed || self.device_closed {
            return None;
        }
        self.rx.poll(now_ms)
    }

    /// The link went down (`keep`: the stream stays registered for a resume on the next connection).
    pub(crate) fn disconnected(&mut self, keep: bool) {
        self.flush_host_gaps();
        self.rx.close();
        self.attached = keep;
        self.push(Entry::Disconnected);
    }

    /// A resume failed: nothing more comes.
    pub(crate) fn ended(&mut self) {
        self.attached = false;
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.closed || self.device_closed
    }

    /// Report the units dropped because the host queue was full (counted streams) that no item has closed yet.
    fn flush_host_gaps(&mut self) {
        for i in 0..self.host_gap.len() {
            if let Some(gap) = self.host_gap[i].take() {
                warn!(
                    "stream {:?}: host queue full, dropped {} units at {}",
                    self.path, gap.units, gap.at
                );
                self.push(Entry::Gap(gap));
            }
        }
    }

    fn push_gaps(&mut self, gaps: &Gaps) {
        for gap in gaps.iter() {
            warn!(
                "stream {:?}: {} units lost at {} ({:?}, {:?} run)",
                self.path, gap.units, gap.at, gap.cause, gap.run
            );
            self.push(Entry::Gap(*gap));
        }
    }

    fn push(&mut self, entry: Entry) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queued_bytes += entry.bytes();
        match entry.run() {
            Some(run) => self.queues[run_index(run)].push_back((seq, entry)),
            None if self.split => {
                let copy = match &entry {
                    Entry::Resumed {
                        cursor,
                        session,
                        ended,
                    } => Entry::Resumed {
                        cursor: *cursor,
                        session: *session,
                        ended: *ended,
                    },
                    Entry::Disconnected => Entry::Disconnected,
                    Entry::Closed => Entry::Closed,
                    _ => unreachable!("events of a run"),
                };
                self.queues[1].push_back((seq, copy));
                self.queues[0].push_back((seq, entry));
            }
            None => self.queues[0].push_back((seq, entry)),
        }
    }

    /// Next entry for a handle of `run` (None: both, in arrival order).
    fn pop(&mut self, run: Option<Run>) -> Option<Entry> {
        let i = match run {
            Some(run) => run_index(run),
            None => match (self.queues[0].front(), self.queues[1].front()) {
                (Some((a, _)), Some((b, _))) => usize::from(b < a),
                (None, Some(_)) => 1,
                _ => 0,
            },
        };
        let (_, entry) = self.queues[i].pop_front()?;
        self.queued_bytes -= entry.bytes();
        if let Entry::Item { head, bytes, .. } = &entry {
            self.rx.delivered(head, bytes.len());
        }
        Some(entry)
    }

    /// Nothing more will come for a handle: closed, or no event loop feeds the stream.
    fn finished(&self, run: Option<Run>) -> bool {
        let empty = match run {
            Some(run) => self.queues[run_index(run)].is_empty(),
            None => self.queues.iter().all(|q| q.is_empty()),
        };
        empty && (self.closed || self.device_closed || !self.attached)
    }

    fn stats(&self) -> StreamStats {
        let rx = self.rx.stats();
        StreamStats {
            received: rx.received,
            lost_device: rx.lost_device,
            lost_link: rx.lost_link,
            lost_store: rx.lost_store,
            lost_host: self.lost_host,
            duplicates: rx.duplicates,
            late: rx.late,
            out_of_order: rx.out_of_order,
            resend_requests: rx.resend_requests,
            queued: self.queues.iter().map(|q| q.len()).sum(),
            queued_bytes: self.queued_bytes,
        }
    }
}

/// Handle of a counted or reliable stream, returned by opening it (`device.samples(config).call().await`), see
/// `docs/api/stream_reliability.md`. Dropping it (both halves after [split](Self::split)) closes the stream.
///
/// ```ignore
/// let (actual, mut samples) = adc.samples(config).call().await??;
/// while let Ok(ev) = samples.recv_event().await {
///     match ev {
///         StreamEvent::Item { pos, t, value, run, .. } => { .. }
///         StreamEvent::Gap { at, units, cause, .. } => { .. }
///         _ => {}
///     }
/// }
/// save(samples.cursor()); // with the data, for samples_resume(cursor) after a restart
/// ```
pub struct PositionedStream<T> {
    pub(crate) shared: Arc<Shared>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    /// Some for a half of [Self::split]
    pub(crate) run: Option<Run>,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T: DeserializeShrinkWrapOwned> PositionedStream<T> {
    /// Next event: an item, a gap, a resume, ...; [StreamError::Closed] once nothing more can come (closed, or the
    /// link went down without `disconnect_keep_streams`).
    pub async fn recv_event(&mut self) -> Result<StreamEvent<T>, StreamError> {
        let shared = self.shared.clone();
        loop {
            let mut notified = pin!(shared.notify.notified());
            notified.as_mut().enable();
            if let Some(ev) = self.try_take()? {
                return Ok(ev);
            }
            notified.await;
        }
    }

    /// Blocking variant of [Self::recv_event].
    pub fn recv_event_blocking(&mut self) -> Result<StreamEvent<T>, StreamError> {
        let mut inner = self.shared.lock();
        loop {
            if let Some(entry) = inner.pop(self.run) {
                drop(inner);
                return to_event(entry);
            }
            if inner.finished(self.run) {
                return Err(StreamError::Closed);
            }
            inner = self
                .shared
                .cond
                .wait(inner)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Next event if there is one queued, without waiting.
    pub fn try_recv_event(&mut self) -> Result<Option<StreamEvent<T>>, StreamError> {
        self.try_take()
    }

    /// Same as [Self::recv_event_blocking], but fails with [Error::Timeout] when nothing arrives within `timeout`.
    pub fn recv_event_timeout_blocking(
        &mut self,
        timeout: Duration,
    ) -> Result<StreamEvent<T>, StreamError> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.shared.lock();
        loop {
            if let Some(entry) = inner.pop(self.run) {
                drop(inner);
                return to_event(entry);
            }
            if inner.finished(self.run) {
                return Err(StreamError::Closed);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout.into());
            }
            inner = self
                .shared
                .cond
                .wait_timeout(inner, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Next value; gaps, resumes and disconnects are skipped (they are counted in [Self::stats] and logged).
    pub async fn recv(&mut self) -> Result<T, StreamError> {
        loop {
            if let StreamEvent::Item { value, .. } = self.recv_event().await? {
                return Ok(value);
            }
        }
    }

    /// Blocking variant of [Self::recv].
    pub fn recv_blocking(&mut self) -> Result<T, StreamError> {
        loop {
            if let StreamEvent::Item { value, .. } = self.recv_event_blocking()? {
                return Ok(value);
            }
        }
    }

    /// Next value if there is one queued, without waiting.
    pub fn try_recv(&mut self) -> Result<Option<T>, StreamError> {
        while let Some(ev) = self.try_take()? {
            if let StreamEvent::Item { value, .. } = ev {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn try_take(&mut self) -> Result<Option<StreamEvent<T>>, StreamError> {
        let mut inner = self.shared.lock();
        match inner.pop(self.run) {
            Some(entry) => {
                drop(inner);
                to_event(entry).map(Some)
            }
            None if inner.finished(self.run) => Err(StreamError::Closed),
            None => Ok(None),
        }
    }

    /// Split into the live run and the backfill run, two handles each delivering its run in order (a dashboard
    /// reads the live one, a recorder both). Call it right after opening; resumes, disconnects and closes come
    /// to both halves.
    pub fn split(self) -> (PositionedStream<T>, PositionedStream<T>) {
        let mut inner = self.shared.lock();
        inner.split = true;
        inner.handles += 1;
        drop(inner);
        let backfill = PositionedStream {
            shared: self.shared.clone(),
            transport_cmd_tx: self.transport_cmd_tx.clone(),
            path_kind: self.path_kind.clone(),
            run: Some(Run::Backfill),
            _phantom: PhantomData,
        };
        let mut live = self;
        live.run = Some(Run::Live);
        (live, backfill)
    }
}

impl<T> PositionedStream<T> {
    /// Where to resume from after a restart: every unit before it was received (counted), or acknowledged
    /// (reliable, see [AckMode]). Save it with the data, pass it to `<name>_resume(cursor)` after a restart.
    /// None before the stream was opened.
    pub fn cursor(&self) -> Option<StreamCursor> {
        self.shared.lock().cursor()
    }

    /// Session the stream is in, as numbered by the device.
    pub fn session(&self) -> Option<u32> {
        self.shared.lock().rx.session()
    }

    /// A backfill run is going on.
    pub fn is_backfilling(&self) -> bool {
        self.shared.lock().rx.is_backfilling()
    }

    /// Confirm every unit before `pos` (an event's end, `pos + units` of an item) with [AckMode::Explicit],
    /// e.g. after writing the data to disk: the device frees it from its store.
    pub fn ack(&self, pos: u64) {
        self.shared.lock().rx.ack(pos);
    }

    pub fn stats(&self) -> StreamStats {
        self.shared.lock().stats()
    }

    /// Close the stream on the device; nothing more is received.
    pub async fn close(&self) -> Result<(), StreamError> {
        self.mark_closed();
        self.transport_cmd_tx
            .send_stream_sideband(self.path_kind.clone(), StreamSideband::Close, None)
            .await?;
        Ok(())
    }

    /// Blocking variant of [Self::close].
    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.mark_closed();
        self.transport_cmd_tx.send_stream_sideband_blocking(
            self.path_kind.clone(),
            StreamSideband::Close,
            None,
        )?;
        Ok(())
    }

    fn mark_closed(&self) {
        let mut inner = self.shared.lock();
        inner.closed = true;
        inner.rx.close();
        drop(inner);
        self.shared.wake();
    }
}

impl<T> Drop for PositionedStream<T> {
    fn drop(&mut self) {
        let mut inner = self.shared.lock();
        inner.handles -= 1;
        // the other half of a split keeps it open
        if inner.handles > 0 || inner.closed {
            return;
        }
        drop(inner);
        self.mark_closed();
        // can't wait here: a place in the command queue, or no Close (the device keeps sending to a host that drops)
        if let Ok(Some(permit)) = self.transport_cmd_tx.try_reserve()
            && let Ok(cmd) = self
                .transport_cmd_tx
                .sideband_command(self.path_kind.clone(), StreamSideband::Close)
        {
            permit.send(cmd);
        }
    }
}

fn to_event<T: DeserializeShrinkWrapOwned>(entry: Entry) -> Result<StreamEvent<T>, StreamError> {
    Ok(match entry {
        Entry::Item {
            head,
            skip,
            run,
            bytes,
        } => StreamEvent::Item {
            pos: head.pos + skip,
            units: head.units - skip,
            t: head.t,
            ext: head.ext,
            run,
            value: T::from_ww_bytes_owned(&bytes)?,
        },
        Entry::Gap(gap) => StreamEvent::Gap {
            run: gap.run,
            at: gap.at,
            units: gap.units,
            cause: gap.cause,
        },
        Entry::Resumed {
            cursor,
            session,
            ended,
        } => StreamEvent::Resumed {
            cursor,
            session,
            ended,
        },
        Entry::BackfillDone => StreamEvent::BackfillDone,
        Entry::Disconnected => StreamEvent::Disconnected,
        Entry::Closed => StreamEvent::Closed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(queue_bytes: usize) -> Arc<Shared> {
        Shared::new(
            vec![],
            PositionedConfig {
                delivery: ww_self::Delivery::Counted,
                ack_mode: AckMode::OnReceive,
                window: DEFAULT_WINDOW,
                queue_bytes,
                max_age_us: None,
            },
        )
    }

    fn entries(inner: &mut Inner) -> Vec<String> {
        core::iter::from_fn(|| inner.pop(None))
            .map(|e| format!("{e:?}"))
            .collect()
    }

    /// A counted stream drops items while the host queue is full, then the device reboots (a new session, positions
    /// from 0): the drop is reported in the old session, the new one starts clean (it panicked on an underflow).
    #[test]
    fn host_queue_gap_ends_at_reopen() {
        let shared = counted(16);
        let mut inner = shared.lock();
        let opened = |session| Opened {
            session,
            oldest: 0,
            live_from: 0,
            resume_from: None,
        };
        inner.opened(None, &opened(1));
        for pos in 0..100 {
            inner.on_item(StreamHead::at(pos), &[0; 8]);
        }
        assert!(inner.host_gap[0].is_some());
        inner.disconnected(true);
        let cursor = inner.cursor();
        inner.opened(cursor, &opened(2));
        inner.on_item(StreamHead::at(0), &[0; 8]);
        let got = entries(&mut inner);
        assert!(got[2].contains("at: 2, units: 98, cause: Host"), "{got:?}");
        assert!(got[3].starts_with("Disconnected"), "{got:?}");
        assert!(got[4].starts_with("Resumed"), "{got:?}");
        assert_eq!(got.len(), 5, "{got:?}");
        // the queue was still full: a new drop, in the new session's positions
        let gap = inner.host_gap[0].unwrap();
        assert_eq!((gap.at, gap.units), (0, 1));
        assert_eq!(inner.lost_host, 99);
    }
}
