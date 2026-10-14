//! Sender state machines of counted and reliable streams.
//!
//! [Counted] only counts: it gives every event its position and the units dropped before it, and keeps counting
//! while no host is there, so the first event after a reconnect says exactly what the host missed.
//!
//! [Reliable] keeps every event in a [StreamStore] until the host acknowledges it, sends while the bytes in flight
//! stay within the host's grant, goes back to where the host asks on `ResendFrom` (go-back-N), and after a resume
//! sends two runs: the live run from the newest event on, and the backlog (backfill run) with what the live run
//! leaves of the link.

use crate::counters::{Counting, StreamCnt, TxStats};
use crate::sched::{Priority, StreamReady};
use crate::store::{PushError, StreamStore};
use crate::{HeadExt, Opened, ResumeError, Run, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;
use ww_client_server::stream::{StoreStatus, StreamStatus};
use ww_si::Second;

/// Sender of a counted stream: positions, `lost`, sessions. No store, nothing resent.
///
/// The producer asks for the head of each event with [Counted::head] and sends it right away (generated `_send`);
/// [Counted::unsent] when sending failed, [Counted::lost] for units it dropped itself.
pub struct Counted {
    session: Option<u32>,
    attached: bool,
    /// Position of the next unit, the lost ones included
    next: u64,
    /// Units lost since the last event, go into the next event's `lost`
    pending_lost: u64,
    pending_ext: Option<HeadExt>,
    counting: Counting,
}

impl Default for Counted {
    fn default() -> Self {
        Self::new()
    }
}

impl Counted {
    /// No session yet, no host.
    pub const fn new() -> Self {
        Counted {
            session: None,
            attached: false,
            next: 0,
            pending_lost: 0,
            pending_ext: None,
            counting: Counting::new(),
        }
    }

    /// Count into these `cnt` instance counters as well.
    #[cfg(feature = "cnt")]
    pub fn counters(mut self, counters: &'static cnt::Counters<StreamCnt>) -> Self {
        self.counting.cnt = Some(counters);
        self
    }

    /// What happened to the data so far.
    pub fn stats(&self) -> &TxStats {
        &self.counting.stats
    }

    /// Current session, `None` before the first one.
    pub fn session(&self) -> Option<u32> {
        self.session
    }

    /// A host has the stream open.
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Position of the next unit.
    pub fn next_pos(&self) -> u64 {
        self.next
    }

    /// A new session without a host (the device starts the stream by itself): positions from 0, units made until a
    /// host resumes are counted as lost.
    pub fn start_session(&mut self, session: u32) {
        self.session = Some(session);
        self.attached = false;
        self.next = 0;
        self.pending_lost = 0;
    }

    /// The host opened the stream (its function returned `Ok`): a new session, positions from 0.
    pub fn open(&mut self, session: u32) -> Opened {
        self.start_session(session);
        self.attached = true;
        Opened {
            session,
            oldest: 0,
            live_from: 0,
            resume_from: None,
        }
    }

    /// The host resumes from `cursor`: the session goes on, the next event's position tells the host what it missed
    /// (`live_from` is where it continues). A cursor of another session gets the current one, the host reports its
    /// old session as ended.
    pub fn resume(&mut self, cursor: StreamCursor) -> Result<Opened, ResumeError<()>> {
        let session = self.session.ok_or(ResumeError::NoSession)?;
        let _ = cursor;
        self.attached = true;
        Ok(Opened {
            session,
            oldest: self.next,
            live_from: self.next,
            resume_from: None,
        })
    }

    /// The host closed the stream or the link went down: the session goes on, units are counted as lost until a
    /// host resumes.
    pub fn detach(&mut self) {
        self.attached = false;
    }

    /// The producer dropped `units` before they reached the stream (its own ring overflowed): they go into the
    /// next event's `lost`.
    pub fn lost(&mut self, units: u64) {
        self.next += units;
        self.pending_lost += units;
        self.counting.add(StreamCnt::LostProducer, units);
    }

    /// Mark the next event: after a reboot, the clock epoch the timestamps from here on belong to.
    pub fn set_ext(&mut self, ext: HeadExt) {
        self.pending_ext = Some(ext);
    }

    /// Head of the next event of `units` units made at `t`, to send right away. `None` while no host has the
    /// stream open: the units are counted as lost (and go into the next event's `lost` when there is a session).
    pub fn head(&mut self, units: u64, t: Option<Second>) -> Option<StreamHead> {
        if !self.attached {
            if self.session.is_some() {
                self.next += units;
                self.pending_lost += units;
            }
            self.counting.add(StreamCnt::LostNoHost, units);
            return None;
        }
        let head = StreamHead {
            pos: self.next,
            units,
            lost: (self.pending_lost > 0).then_some(self.pending_lost),
            t,
            ext: self.pending_ext.take(),
        };
        self.next += units;
        self.pending_lost = 0;
        self.counting.add(StreamCnt::Sent, units);
        Some(head)
    }

    /// State for a read on the stream.
    pub fn status(&self) -> StreamStatus {
        StreamStatus {
            open: self.attached,
            session: self.session.map(UVlq32),
            next: UVlq64(self.next),
            oldest: None,
            newest: None,
            in_flight: UVlq64(0),
            store: None,
        }
    }

    /// The event with `head` couldn't be sent: its units, and the `lost` it carried, go into the next event's
    /// `lost`, so the host sees a device loss instead of a link loss.
    pub fn unsent(&mut self, head: &StreamHead) {
        self.pending_lost += head.units + head.lost_units();
        if let Some(ext) = head.ext {
            self.pending_ext.get_or_insert(ext);
        }
        self.counting.stats.sent = self.counting.stats.sent.saturating_sub(head.units);
        self.counting.add(StreamCnt::LostNoHost, head.units);
    }
}

/// Grant a [Reliable] stream starts with, before the host's first `Ack`.
pub const DEFAULT_INITIAL_GRANT: u64 = 4096;
/// How long a [Reliable] stream with data in flight waits for an `Ack` before it asks with a `Query`.
pub const DEFAULT_QUERY_INTERVAL_MS: u64 = 200;

/// Events sent and not acknowledged yet, `(pos, end, bytes)` each, in position order. When more than `K` are in
/// flight, the two neighbours with the fewest bytes are merged: acknowledgements and resends then act on the merged
/// range, a little later or a little more, never wrong.
#[derive(Debug, Clone)]
struct Flight<const K: usize> {
    entries: [(u64, u64, u64); K],
    len: usize,
}

impl<const K: usize> Flight<K> {
    const fn new() -> Self {
        Flight {
            entries: [(0, 0, 0); K],
            len: 0,
        }
    }

    fn bytes(&self) -> u64 {
        self.entries[..self.len].iter().map(|e| e.2).sum()
    }

    fn push(&mut self, pos: u64, end: u64, bytes: u64) {
        if K == 0 {
            return;
        }
        if self.len == K {
            if K == 1 {
                let e = &mut self.entries[0];
                *e = (e.0, end, e.2 + bytes);
                return;
            }
            let i = (0..K - 1)
                .min_by_key(|&i| self.entries[i].2 + self.entries[i + 1].2)
                .unwrap_or(0);
            let next = self.entries[i + 1];
            self.entries[i].1 = next.1;
            self.entries[i].2 += next.2;
            self.entries.copy_within(i + 2..K, i + 1);
            self.len -= 1;
        }
        self.entries[self.len] = (pos, end, bytes);
        self.len += 1;
    }

    /// Everything that ends at or before `have` arrived: drop it, return its bytes.
    fn ack(&mut self, have: u64) -> u64 {
        let n = self.entries[..self.len]
            .iter()
            .take_while(|e| e.1 <= have)
            .count();
        let bytes = self.entries[..n].iter().map(|e| e.2).sum();
        self.entries.copy_within(n..self.len, 0);
        self.len -= n;
        bytes
    }

    /// Send again from `from`: drop what starts at or after it; returns where sending resumes (the start of a merged
    /// entry `from` falls into).
    fn rewind(&mut self, from: u64) -> u64 {
        let mut resume = from;
        while self.len > 0 {
            let (pos, end, _) = self.entries[self.len - 1];
            if pos >= from || end > from {
                resume = resume.min(pos);
                self.len -= 1;
            } else {
                break;
            }
        }
        resume
    }

    fn clear(&mut self) {
        self.len = 0;
    }
}

/// Sending state of one run.
#[derive(Debug, Clone)]
struct RunTx<const K: usize> {
    /// Next position to send from (read the store there)
    next: u64,
    /// Every unit before it was acknowledged
    acked: u64,
    /// Highest end sent so far, what is below it is a resend
    high: u64,
    flight: Flight<K>,
}

impl<const K: usize> RunTx<K> {
    const fn at(pos: u64) -> Self {
        RunTx {
            next: pos,
            acked: pos,
            high: pos,
            flight: Flight::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct Backfill<const K: usize> {
    run: RunTx<K>,
    /// The live run's start: the backfill run ends right before it
    end: u64,
    /// The rest of the backlog was dropped from the store (drop oldest): the run ends once what was sent is
    /// acknowledged
    tail_gone: bool,
}

/// What [Reliable::push] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pushed {
    Stored,
    /// Dropped (store full, no session) and counted, its units go into the next event's `lost`.
    Lost,
}

/// Why a generated `<stream>_push` failed: the item didn't serialize (nothing was pushed or counted), or the store
/// failed (the units were counted as lost).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushItemError<E> {
    Serialize(wire_weaver::shrink_wrap::Error),
    Store(E),
}

/// An event to send, see [Reliable::next_event].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outgoing<'b> {
    /// Head as stored, with `lost` covering what the host won't get before it in this run.
    pub head: StreamHead,
    pub item: &'b [u8],
    pub run: Run,
}

/// Sender of a reliable stream over a [StreamStore] `S`, with up to `K` in-flight records per run (see the
/// [module docs](self)).
///
/// The producer [pushes](Self::push) events into the store, whether a host is there or not. The event loop asks
/// [ready](Self::ready) (or the [Scheduler](crate::sched::Scheduler) does), takes the [next event](Self::next_event)
/// of a run, sends it, and feeds sideband commands from the host to [on_sideband](Self::on_sideband).
/// [poll_query](Self::poll_query) asks the host where it is when acknowledgements stop coming.
pub struct Reliable<S, const K: usize = 16> {
    store: S,
    session: Option<u32>,
    attached: bool,
    /// Position of the next unit, the lost ones included
    next: u64,
    pending_lost: u64,
    pending_ext: Option<HeadExt>,
    live: RunTx<K>,
    backfill: Option<Backfill<K>>,
    /// Released from the store up to here
    released: u64,
    /// The store's `dropped_units` when last looked at
    store_dropped: u64,
    /// Position of the newest event stored and where the previous one ended (`pos - lost`)
    newest_stored: Option<(u64, u64)>,
    grant: u64,
    initial_grant: u64,
    priority: Priority,
    query_interval_ms: u64,
    last_heard_ms: Option<u64>,
    last_query_ms: Option<u64>,
    /// Tell the host with a `Query` right away (the backfill run ended early)
    query_now: bool,
    counting: Counting,
}

impl<S: StreamStore, const K: usize> Reliable<S, K> {
    /// No session yet, no host, normal priority.
    pub const fn new(store: S) -> Self {
        Reliable {
            store,
            session: None,
            attached: false,
            next: 0,
            pending_lost: 0,
            pending_ext: None,
            live: RunTx::at(0),
            backfill: None,
            released: 0,
            store_dropped: 0,
            newest_stored: None,
            grant: DEFAULT_INITIAL_GRANT,
            initial_grant: DEFAULT_INITIAL_GRANT,
            priority: Priority::Normal,
            query_interval_ms: DEFAULT_QUERY_INTERVAL_MS,
            last_heard_ms: None,
            last_query_ms: None,
            query_now: false,
            counting: Counting::new(),
        }
    }

    /// Priority against other streams in the [Scheduler](crate::sched::Scheduler).
    pub const fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Bytes in flight allowed before the host's first `Ack` ([DEFAULT_INITIAL_GRANT]).
    pub const fn initial_grant(mut self, bytes: u64) -> Self {
        self.initial_grant = bytes;
        self
    }

    /// How long to wait for an `Ack` with data in flight before sending a `Query` ([DEFAULT_QUERY_INTERVAL_MS]).
    pub const fn query_interval_ms(mut self, ms: u64) -> Self {
        self.query_interval_ms = ms;
        self
    }

    /// Count into these `cnt` instance counters as well.
    #[cfg(feature = "cnt")]
    pub fn counters(mut self, counters: &'static cnt::Counters<StreamCnt>) -> Self {
        self.counting.cnt = Some(counters);
        self
    }

    /// What happened to the data so far.
    pub fn stats(&self) -> &TxStats {
        &self.counting.stats
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Current session, `None` before the first one.
    pub fn session(&self) -> Option<u32> {
        self.session
    }

    /// A host has the stream open.
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Position of the next unit.
    pub fn next_pos(&self) -> u64 {
        self.next
    }

    /// A backfill run is going on.
    pub fn is_backfilling(&self) -> bool {
        self.backfill.is_some()
    }

    /// Bytes sent and not acknowledged, both runs.
    pub fn in_flight(&self) -> u64 {
        self.live.flight.bytes() + self.backfill.as_ref().map_or(0, |b| b.run.flight.bytes())
    }

    /// Continue the session a persistent store holds (after a reboot): session and position from the store. Does
    /// nothing when the store holds no session (`next` is 0 and it is empty).
    pub fn continue_stored_session(&mut self) {
        let state = self.store.state();
        if state.next > 0 || state.oldest.is_some() {
            self.session = Some(state.session);
            self.next = state.next;
            self.pending_lost = 0;
            self.released = state.oldest.unwrap_or(state.next);
            self.store_dropped = state.dropped_units;
        }
    }

    /// A new session without a host (a logger starting by itself): the store is emptied, positions from 0; events
    /// are kept until a host resumes and acknowledges them.
    pub async fn start_session(&mut self, session: u32) -> Result<(), S::Error> {
        self.store.reset(session).await?;
        self.session = Some(session);
        self.attached = false;
        self.next = 0;
        self.pending_lost = 0;
        self.released = 0;
        self.store_dropped = 0;
        self.newest_stored = None;
        self.live = RunTx::at(0);
        self.backfill = None;
        Ok(())
    }

    /// The host opened the stream (its function returned `Ok`): a new session, positions from 0.
    pub async fn open(&mut self, session: u32) -> Result<Opened, S::Error> {
        self.start_session(session).await?;
        self.attach(0, None);
        Ok(Opened {
            session,
            oldest: 0,
            live_from: 0,
            resume_from: None,
        })
    }

    /// The host resumes from `cursor`. Live events start at the newest event kept (so the host sees the latest value
    /// at once; `live_from` is where the units it says were lost before it start), the backlog from the cursor (or
    /// the oldest event kept) up to there is sent as the backfill run.
    /// A cursor of another session resumes the current one from its start, the host reports its old session as
    /// ended.
    pub async fn resume(&mut self, cursor: StreamCursor) -> Result<Opened, ResumeError<S::Error>> {
        let session = self.session.ok_or(ResumeError::NoSession)?;
        let same_session = cursor.session.0 == session;
        let from = if same_session {
            cursor.pos.0.min(self.next)
        } else {
            0
        };
        if same_session && from > self.released {
            self.store.release(from).await.map_err(ResumeError::Store)?;
            self.released = from;
        }
        let state = self.store.state();
        let live_from = match state.newest {
            // the newest event's own `lost` stays in the live run, before it
            Some(newest) if newest >= from => match self.newest_stored {
                Some((pos, after_previous)) if pos == newest => after_previous.max(from),
                _ => newest,
            },
            _ => self.next,
        };
        let backfill_from = state.oldest.map(|oldest| oldest.max(from));
        let resume_from = backfill_from.filter(|&b| b < live_from);
        self.attach(live_from, resume_from);
        Ok(Opened {
            session,
            oldest: state.oldest.unwrap_or(live_from),
            live_from,
            resume_from,
        })
    }

    fn attach(&mut self, live_from: u64, backfill_from: Option<u64>) {
        self.attached = true;
        self.live = RunTx::at(live_from);
        self.backfill = backfill_from.map(|from| Backfill {
            run: RunTx::at(from),
            end: live_from,
            tail_gone: false,
        });
        self.query_now = false;
        self.grant = self.initial_grant;
        self.last_heard_ms = None;
        self.last_query_ms = None;
    }

    /// The host closed the stream or the link went down: nothing is sent, events stay in the store (unless
    /// acknowledged) for a resume.
    pub fn detach(&mut self) {
        self.attached = false;
        self.live.flight.clear();
        self.backfill = None;
    }

    /// The producer dropped `units` before they reached the stream (its own ring overflowed): they go into the
    /// next event's `lost`.
    pub fn lost(&mut self, units: u64) {
        self.next += units;
        self.pending_lost += units;
        self.counting.add(StreamCnt::LostProducer, units);
    }

    /// Mark the next event: after a reboot, the clock epoch the timestamps from here on belong to.
    pub fn set_ext(&mut self, ext: HeadExt) {
        self.pending_ext = Some(ext);
    }

    /// Put an event of `units` units made at `t` into the store, host or not. Never waits for the link.
    ///
    /// No room (or no session yet): the units are counted as lost and go into the next event's `lost`. A store
    /// error is returned after the units were counted as lost the same way.
    pub async fn push(
        &mut self,
        item: &[u8],
        units: u64,
        t: Option<Second>,
    ) -> Result<Pushed, S::Error> {
        if self.session.is_none() {
            self.counting.add(StreamCnt::LostNoHost, units);
            return Ok(Pushed::Lost);
        }
        let head = StreamHead {
            pos: self.next,
            units,
            lost: (self.pending_lost > 0).then_some(self.pending_lost),
            t,
            ext: self.pending_ext.take(),
        };
        self.next += units;
        let result = self.store.push(&head, item).await;
        let dropped = self.store.state().dropped_units;
        self.counting.add(
            StreamCnt::LostStoreFull,
            dropped.saturating_sub(self.store_dropped),
        );
        self.store_dropped = dropped;
        match result {
            Ok(()) => {
                self.pending_lost = 0;
                self.newest_stored = Some((head.pos, head.after_previous()));
                Ok(Pushed::Stored)
            }
            Err(e) => {
                self.pending_lost += units;
                self.pending_ext = head.ext;
                self.counting.add(StreamCnt::LostStoreFull, units);
                match e {
                    PushError::Full | PushError::TooBig => Ok(Pushed::Lost),
                    PushError::Store(e) => Err(e),
                }
            }
        }
    }

    fn credit_left(&self) -> bool {
        self.in_flight() < self.grant
    }

    /// What this stream could send now, for the [Scheduler](crate::sched::Scheduler).
    pub fn ready(&self) -> StreamReady {
        let credit = self.attached && self.credit_left();
        let newest = self.store.state().newest;
        StreamReady {
            priority: self.priority,
            live: credit && newest.is_some_and(|n| n >= self.live.next),
            backfill: credit
                // also when the store has nothing there: next_event finds out the backlog is gone
                && self
                    .backfill
                    .as_ref()
                    .is_some_and(|b| b.run.next < b.end && !b.tail_gone),
        }
    }

    /// Read the next event of `run` into `buf` and mark it sent; `None` when the run has nothing to send or no
    /// credit is left. Send the returned event right away; the stream is not borrowed by it.
    ///
    /// `buf` must hold the biggest item. An event may go out while fewer bytes than its own are left of the grant
    /// (the host grants room for at least its biggest item).
    pub async fn next_event<'b>(
        &mut self,
        run: Run,
        buf: &'b mut [u8],
    ) -> Result<Option<Outgoing<'b>>, S::Error> {
        if !self.attached || !self.credit_left() {
            return Ok(None);
        }
        let (from, end) = match run {
            Run::Live => (self.live.next, u64::MAX),
            Run::Backfill => match &self.backfill {
                Some(b) if b.run.next < b.end && !b.tail_gone => (b.run.next, b.end),
                _ => return Ok(None),
            },
        };
        let Some(event) = self.store.read(from, buf).await? else {
            if let (Run::Backfill, Some(b)) = (run, &mut self.backfill) {
                b.tail_gone = true;
                let acked = b.run.acked;
                self.ack_backfill(acked);
            }
            return Ok(None);
        };
        let state = match run {
            Run::Live => &mut self.live,
            Run::Backfill => {
                let backfill = self.backfill.as_mut().expect("checked above");
                if event.head.pos >= end {
                    // the rest of the backlog is gone (dropped oldest): the run ends once what was sent arrived
                    backfill.tail_gone = true;
                    let acked = backfill.run.acked;
                    self.ack_backfill(acked);
                    return Ok(None);
                }
                &mut backfill.run
            }
        };
        let mut head = event.head;
        let skipped = head.pos - state.next;
        head.lost = (skipped > 0).then_some(skipped);
        let resend = head.end() <= state.high;
        state
            .flight
            .push(head.pos, head.end(), event.item.len() as u64);
        state.next = head.end();
        state.high = state.high.max(state.next);
        self.counting.add(
            if resend {
                StreamCnt::Resent
            } else {
                StreamCnt::Sent
            },
            head.units,
        );
        Ok(Some(Outgoing {
            head,
            item: event.item,
            run,
        }))
    }

    /// The host's `Ack` or `ResendFrom` (other sideband commands are ignored). Acknowledged events are released
    /// from the store.
    pub async fn on_sideband(
        &mut self,
        sideband: &StreamSideband,
        now_ms: u64,
    ) -> Result<(), S::Error> {
        self.on_sideband_at(sideband, Some(now_ms)).await
    }

    /// [on_sideband](Self::on_sideband) where the time isn't at hand (generated request dispatch): the host counts
    /// as heard at the next [poll_query](Self::poll_query).
    pub async fn on_host_sideband(&mut self, sideband: &StreamSideband) -> Result<(), S::Error> {
        self.on_sideband_at(sideband, None).await
    }

    async fn on_sideband_at(
        &mut self,
        sideband: &StreamSideband,
        now_ms: Option<u64>,
    ) -> Result<(), S::Error> {
        if !self.attached {
            return Ok(());
        }
        match *sideband {
            StreamSideband::Ack {
                have,
                backfill,
                grant,
            } => {
                self.last_heard_ms = now_ms;
                self.last_query_ms = None;
                if !self.ack_live(have.0) {
                    return Ok(());
                }
                match (backfill, &self.backfill) {
                    (Some(b), Some(_)) => self.ack_backfill(b.0),
                    // the host still waits for a backfill run that ended here: tell it
                    (Some(_), None) => self.query_now = true,
                    // the host has no backfill run (any more): it got all of it
                    (None, Some(b)) if b.run.next >= b.end => {
                        let end = b.end;
                        self.ack_backfill(end);
                    }
                    _ => {}
                }
                self.grant = grant.0 as u64;
            }
            StreamSideband::ResendFrom { from, grant } => {
                self.last_heard_ms = now_ms;
                self.last_query_ms = None;
                self.counting.add(StreamCnt::ResendRequests, 1);
                let from = from.0;
                match &mut self.backfill {
                    Some(b) if from < b.end => {
                        if from < b.run.acked || from > b.run.next {
                            self.counting.add(StreamCnt::Overruns, 1);
                            self.query_now = true;
                            return Ok(());
                        }
                        self.ack_backfill(from);
                        if let Some(b) = &mut self.backfill {
                            // the host acknowledged up to an event boundary: never back before it
                            b.run.next = b.run.flight.rewind(from).max(b.run.acked);
                            b.tail_gone = false;
                        }
                    }
                    _ => {
                        if from < self.live.acked || from > self.live.next {
                            // the host is out of step (the backfill run it asks for ended here): tell it
                            self.counting.add(StreamCnt::Overruns, 1);
                            self.query_now = true;
                            return Ok(());
                        }
                        self.ack_live(from);
                        self.live.next = self.live.flight.rewind(from).max(self.live.acked);
                    }
                }
                self.grant = grant.0 as u64;
            }
            _ => return Ok(()),
        }
        let release = match &self.backfill {
            Some(b) => b.run.acked,
            None => self.live.acked,
        };
        if release > self.released {
            self.store.release(release).await?;
            self.released = release;
        }
        Ok(())
    }

    /// Returns false for an acknowledgement beyond what was sent (ignored, counted as an overrun).
    fn ack_live(&mut self, have: u64) -> bool {
        if have > self.live.next {
            self.counting.add(StreamCnt::Overruns, 1);
            return false;
        }
        if have > self.live.acked {
            let bytes = self.live.flight.ack(have);
            self.counting.add(StreamCnt::Acked, bytes);
            self.live.acked = have;
        }
        true
    }

    fn ack_backfill(&mut self, have: u64) {
        let Some(b) = &mut self.backfill else {
            return;
        };
        if have > b.run.next {
            self.counting.add(StreamCnt::Overruns, 1);
            return;
        }
        if have > b.run.acked {
            let bytes = b.run.flight.ack(have);
            self.counting.add(StreamCnt::Acked, bytes);
            b.run.acked = have;
        }
        if b.run.acked >= b.end {
            self.backfill = None;
        } else if b.tail_gone && b.run.acked >= b.run.next {
            self.backfill = None;
            self.query_now = true;
        }
    }

    /// A `Query` to send when events are in flight and the host was silent for the query interval (once per
    /// interval), or right away when the backfill run ended early because the store dropped the rest of the
    /// backlog (its `backfill` is `None` then, the host reports the rest as lost).
    pub fn poll_query(&mut self, now_ms: u64) -> Option<StreamSideband> {
        if !self.attached {
            return None;
        }
        if core::mem::take(&mut self.query_now) {
            self.last_query_ms = Some(now_ms);
            return Some(self.query());
        }
        if self.in_flight() == 0 {
            return None;
        }
        let since = *self.last_heard_ms.get_or_insert(now_ms);
        let last = self.last_query_ms.unwrap_or(since);
        if now_ms.saturating_sub(last) < self.query_interval_ms
            || now_ms.saturating_sub(since) < self.query_interval_ms
        {
            return None;
        }
        self.last_query_ms = Some(now_ms);
        Some(self.query())
    }

    fn query(&self) -> StreamSideband {
        StreamSideband::Query {
            sent: UVlq64(self.live.next),
            backfill: self.backfill.as_ref().map(|b| UVlq64(b.run.next)),
        }
    }

    /// State for a read on the stream.
    pub fn status(&self) -> StreamStatus {
        let state = self.store.state();
        StreamStatus {
            open: self.attached,
            session: self.session.map(UVlq32),
            next: UVlq64(self.next),
            oldest: state.oldest.map(UVlq64),
            newest: state.newest.map(UVlq64),
            in_flight: UVlq64(self.in_flight()),
            store: Some(StoreStatus {
                used_bytes: UVlq64(state.used_bytes),
                capacity_bytes: UVlq64(state.capacity_bytes),
                persistent: state.persistent,
                dropped_units: UVlq64(state.dropped_units),
            }),
        }
    }

    /// The cursor the host would need to have everything acknowledged so far.
    pub fn acked_cursor(&self) -> Option<StreamCursor> {
        Some(StreamCursor {
            session: UVlq32(self.session?),
            pos: UVlq64(
                self.backfill
                    .as_ref()
                    .map_or(self.live.acked, |b| b.run.acked),
            ),
        })
    }
}

#[cfg(test)]
impl<S: StreamStore, const K: usize> Reliable<S, K> {
    pub(crate) fn debug_state(&self) -> std::string::String {
        std::format!(
            "live {:?} backfill {:?} next {} released {} grant {} store {:?}",
            self.live,
            self.backfill,
            self.next,
            self.released,
            self.grant,
            self.store.state()
        )
    }
}
