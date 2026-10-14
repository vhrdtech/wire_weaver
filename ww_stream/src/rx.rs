//! Receiver state machine of counted and reliable streams (host side).
//!
//! [Receiver] sorts each arriving event by its position: in order, after a gap (and whose gap it is: the device's,
//! the link's, the store's), a duplicate, or late. A counted stream takes every new event and reports gaps; a
//! reliable one takes events in order only, per run, asks for a resend once per gap and acknowledges what user code
//! received (or, with [AckMode::Explicit], what it confirmed), so the device can free its store.
//!
//! The host never counts units of an item itself: everything comes from the heads.

use crate::{Opened, Run, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;

/// Kind of stream the receiver is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Positions and gaps, nothing acknowledged or resent.
    Counted,
    /// In order, acknowledged, resent on loss.
    Reliable,
}

/// When a reliable stream's data counts as received, for the `Ack` that lets the device free it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AckMode {
    /// When user code takes an event ([Receiver::delivered]).
    #[default]
    OnReceive,
    /// Only when user code confirms it ([Receiver::ack]), e.g. after writing it to disk, so a crash in between loses
    /// nothing.
    Explicit,
}

/// Whose loss a gap is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cause {
    /// The device dropped the units (producer overflow, store full, no host): `lost` in a head.
    Device,
    /// Lost on the link: a jump of the position that `lost` doesn't explain.
    Link,
    /// The device's store didn't keep them for a resume.
    Store,
    /// The host's own queue was full.
    Host,
}

/// Units missing in a run, reported once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub run: Run,
    /// Position of the first missing unit.
    pub at: u64,
    pub units: u64,
    pub cause: Cause,
}

/// Gaps reported at once (before one event: link and device; split around ranges the host already has), see
/// [Gaps::iter].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gaps([Option<Gap>; 6]);

impl Gaps {
    const NONE: Gaps = Gaps([None; 6]);

    fn push(&mut self, gap: Gap) {
        if gap.units == 0 {
            return;
        }
        if let Some(slot) = self.0.iter_mut().find(|g| g.is_none()) {
            *slot = Some(gap);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Gap> {
        self.0.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.0[0].is_none()
    }
}

/// Ranges of live events received before a resume in the middle of a backfill, sorted, disjoint. They come again
/// in the new backlog (the cursor can't say "backlog up to here, and this live range"): dropped as duplicates, no
/// gap in them reported twice. Up to [SEEN_RANGES], the lowest forgotten (its events are then delivered twice,
/// never lost).
#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    ranges: [(u64, u64); SEEN_RANGES],
    len: usize,
}

/// Live ranges a receiver remembers across resumes in the middle of a backfill.
const SEEN_RANGES: usize = 8;

impl Seen {
    fn add(&mut self, a: u64, b: u64) {
        if a >= b {
            return;
        }
        let (mut a, mut b) = (a, b);
        let mut kept = [(0, 0); SEEN_RANGES];
        let mut n = 0;
        for &(ra, rb) in &self.ranges[..self.len] {
            if rb >= a && ra <= b {
                a = a.min(ra);
                b = b.max(rb);
            } else {
                kept[n] = (ra, rb);
                n += 1;
            }
        }
        if n == kept.len() {
            kept.copy_within(1.., 0);
            n -= 1;
        }
        kept[n] = (a, b);
        n += 1;
        kept[..n].sort_unstable();
        self.ranges = kept;
        self.len = n;
    }

    /// Drop what ends at or before `pos`: everything before it is had anyway.
    fn forget_before(&mut self, pos: u64) {
        let mut n = 0;
        for i in 0..self.len {
            if self.ranges[i].1 > pos {
                self.ranges[n] = self.ranges[i];
                n += 1;
            }
        }
        self.len = n;
    }

    fn contains(&self, a: u64, b: u64) -> bool {
        self.ranges[..self.len]
            .iter()
            .any(|&(ra, rb)| ra <= a && b <= rb)
    }

    /// `gaps` without the parts the host already has.
    fn clip(&self, gaps: Gaps) -> Gaps {
        let mut out = Gaps::NONE;
        for gap in gaps.iter() {
            let end = gap.at + gap.units;
            let mut at = gap.at;
            for &(ra, rb) in &self.ranges[..self.len] {
                if rb <= at {
                    continue;
                }
                if ra >= end {
                    break;
                }
                if ra > at {
                    out.push(Gap {
                        at,
                        units: ra - at,
                        ..*gap
                    });
                }
                at = at.max(rb);
            }
            if at < end {
                out.push(Gap {
                    at,
                    units: end - at,
                    ..*gap
                });
            }
        }
        out
    }
}

/// What to do with an event, see [Receiver::on_item].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Pass it on to user code, after reporting `gaps`. `backfill_done`: it completed the backfill run.
    Accept {
        run: Run,
        gaps: Gaps,
        backfill_done: bool,
    },
    /// Received before: drop it.
    Duplicate,
    /// Received before a resume, comes again in the backlog: drop it, report `gaps` (the part of the units lost
    /// before it that the host didn't have yet). `backfill_done`: it completed the backfill run.
    Had { gaps: Gaps, backfill_done: bool },
    /// Counted streams: arrived after its units were reported as a link gap: drop it.
    Late,
    /// Reliable streams: something before it is missing, drop it; a `ResendFrom` is on its way.
    OutOfOrder,
    /// The stream isn't open.
    NotOpen,
}

/// What a receiver saw, by cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RxStats {
    /// Units accepted.
    pub received: u64,
    pub lost_device: u64,
    pub lost_link: u64,
    pub lost_store: u64,
    pub duplicates: u64,
    pub late: u64,
    /// Reliable: events dropped because something before them was missing.
    pub out_of_order: u64,
    pub resend_requests: u64,
}

impl RxStats {
    fn count(&mut self, gaps: &Gaps) {
        for gap in gaps.iter() {
            match gap.cause {
                Cause::Device => self.lost_device += gap.units,
                Cause::Link => self.lost_link += gap.units,
                Cause::Store | Cause::Host => self.lost_store += gap.units,
            }
        }
    }
}

/// Window of a reliable stream by default: what the host queues for it.
pub const DEFAULT_WINDOW: u32 = 4 << 20;
/// A reliable stream acknowledges at least this often while data comes in.
pub const DEFAULT_ACK_INTERVAL_MS: u64 = 100;
/// A `ResendFrom` that brought nothing is asked again after this long.
pub const DEFAULT_RESEND_TIMEOUT_MS: u64 = 500;

#[derive(Debug, Clone)]
struct RxRun {
    /// Position where the next event of this run should start
    expect: u64,
    /// Received (OnReceive: delivered) or confirmed (Explicit) up to here
    acked: u64,
    /// Last link gap reported (counted): an event inside it arriving later is late
    link_gap: Option<(u64, u64)>,
}

impl RxRun {
    fn at(pos: u64) -> Self {
        RxRun {
            expect: pos,
            acked: pos,
            link_gap: None,
        }
    }
}

#[derive(Debug, Clone)]
struct RxBackfill {
    run: RxRun,
    end: u64,
    /// All of it arrived (or the rest was reported lost); dropped after an `Ack` that says so
    complete: bool,
}

/// Receiving state of one counted or reliable stream, see the [module docs](self).
///
/// Feed it the stream's open answer ([opened](Self::opened)), every event's head ([on_item](Self::on_item)),
/// what user code took ([delivered](Self::delivered)) or confirmed ([ack](Self::ack)) and the device's `Query`
/// ([on_query](Self::on_query)); send what [poll](Self::poll) returns.
#[derive(Debug, Clone)]
pub struct Receiver {
    delivery: Delivery,
    ack_mode: AckMode,
    window: u32,
    ack_interval_ms: u64,
    resend_timeout_ms: u64,
    session: Option<u32>,
    live_from: u64,
    live: RxRun,
    backfill: Option<RxBackfill>,
    seen: Seen,
    /// Send an Ack at the next poll
    ack_due: bool,
    bytes_since_ack: u64,
    last_ack_ms: Option<u64>,
    /// Something new arrived in the backfill run (to notice a stalled one)
    backfill_progress: bool,
    /// What the last Ack said
    last_acked: (u64, Option<u64>),
    /// Position a ResendFrom is owed for (or was sent for, and when)
    /// Per run (live, backfill)
    resend: [Option<Resend>; 2],
    stats: RxStats,
}

#[derive(Debug, Clone, Copy)]
struct Resend {
    from: u64,
    sent_ms: Option<u64>,
}

impl Receiver {
    /// Receiver of a stream of `delivery` kind, not open yet.
    pub const fn new(delivery: Delivery) -> Self {
        Receiver {
            delivery,
            ack_mode: AckMode::OnReceive,
            window: DEFAULT_WINDOW,
            ack_interval_ms: DEFAULT_ACK_INTERVAL_MS,
            resend_timeout_ms: DEFAULT_RESEND_TIMEOUT_MS,
            session: None,
            live_from: 0,
            seen: Seen {
                ranges: [(0, 0); SEEN_RANGES],
                len: 0,
            },
            live: RxRun {
                expect: 0,
                acked: 0,
                link_gap: None,
            },
            backfill: None,
            ack_due: false,
            bytes_since_ack: 0,
            last_ack_ms: None,
            backfill_progress: false,
            last_acked: (0, None),
            resend: [None; 2],
            stats: RxStats {
                received: 0,
                lost_device: 0,
                lost_link: 0,
                lost_store: 0,
                duplicates: 0,
                late: 0,
                out_of_order: 0,
                resend_requests: 0,
            },
        }
    }

    /// Bytes of items the host queues for this stream ([DEFAULT_WINDOW]); the device never has more in flight.
    pub const fn window(mut self, bytes: u32) -> Self {
        self.window = bytes;
        self
    }

    pub const fn ack_mode(mut self, mode: AckMode) -> Self {
        self.ack_mode = mode;
        self
    }

    /// Acknowledge at least this often while data comes in ([DEFAULT_ACK_INTERVAL_MS]).
    pub const fn ack_interval_ms(mut self, ms: u64) -> Self {
        self.ack_interval_ms = ms;
        self
    }

    pub fn stats(&self) -> &RxStats {
        &self.stats
    }

    pub fn session(&self) -> Option<u32> {
        self.session
    }

    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    /// A backfill run is going on.
    pub fn is_backfilling(&self) -> bool {
        self.backfill.as_ref().is_some_and(|b| !b.complete)
    }

    /// Where the next live event should start.
    pub fn live_expect(&self) -> u64 {
        self.live.expect
    }

    /// Where to resume from after a reconnect or a restart: every unit before it was received (counted), or
    /// acknowledged (reliable). While a backfill runs, that is the backfill's position: resuming from there gets
    /// the rest of the backlog, and the live events after it once more (the receiver drops them as duplicates).
    pub fn cursor(&self) -> Option<StreamCursor> {
        let pos = match self.delivery {
            Delivery::Counted => self.live.expect,
            Delivery::Reliable => match &self.backfill {
                Some(b) if !b.complete => b.run.acked,
                _ => self.live.acked,
            },
        };
        Some(StreamCursor {
            session: UVlq32(self.session?),
            pos: UVlq64(pos),
        })
    }

    /// The device answered the open (with `cursor` when it was a resume). Returns the gaps the device reported in
    /// its answer, and the old session when the device is in another one (the old one ended).
    pub fn opened(&mut self, cursor: Option<StreamCursor>, opened: &Opened) -> (Gaps, Option<u32>) {
        let mut gaps = Gaps::NONE;
        let mut ended = None;
        let mut seen = Seen::default();
        if let Some(cursor) = cursor {
            let same_session = cursor.session.0 == opened.session;
            let from = if same_session {
                cursor.pos.0
            } else {
                ended = Some(cursor.session.0);
                0
            };
            if same_session && self.session == Some(opened.session) {
                seen = self.seen;
                // what user code took (or confirmed): the rest of the old live run comes again
                seen.add(self.live_from, self.live.acked);
                seen.forget_before(from);
            }
            match opened.resume_from {
                Some(resume_from) if resume_from < opened.live_from => gaps.push(Gap {
                    run: Run::Backfill,
                    at: from,
                    units: resume_from.saturating_sub(from),
                    cause: Cause::Store,
                }),
                _ => gaps.push(Gap {
                    run: Run::Live,
                    at: from,
                    units: opened.live_from.saturating_sub(from),
                    cause: match self.delivery {
                        Delivery::Counted => Cause::Device,
                        Delivery::Reliable => Cause::Store,
                    },
                }),
            }
        }
        let gaps = seen.clip(gaps);
        self.stats.count(&gaps);
        self.session = Some(opened.session);
        self.seen = seen;
        self.live_from = opened.live_from;
        self.live = RxRun::at(opened.live_from);
        self.backfill = opened
            .resume_from
            .filter(|&from| from < opened.live_from)
            .map(|from| RxBackfill {
                run: RxRun::at(from),
                end: opened.live_from,
                complete: false,
            });
        self.ack_due = self.delivery == Delivery::Reliable;
        self.bytes_since_ack = 0;
        self.last_ack_ms = None;
        self.last_acked = (opened.live_from, None);
        self.resend = [None; 2];
        (gaps, ended)
    }

    /// The stream was closed or the link went down; [cursor](Self::cursor) still says where to resume.
    pub fn close(&mut self) {
        self.ack_due = false;
        self.resend = [None; 2];
    }

    fn run_of(&self, pos: u64) -> Run {
        match &self.backfill {
            Some(b) if pos < b.end => Run::Backfill,
            _ => Run::Live,
        }
    }

    /// An event arrived: what to do with it, see [Verdict].
    pub fn on_item(&mut self, head: &StreamHead) -> Verdict {
        if self.session.is_none() {
            return Verdict::NotOpen;
        }
        let run = self.run_of(head.pos);
        let delivery = self.delivery;
        let state = match run {
            Run::Live => &mut self.live,
            Run::Backfill => &mut self.backfill.as_mut().expect("run_of").run,
        };
        if head.pos < state.expect {
            if let Some((at, end)) = state.link_gap
                && head.pos >= at
                && head.pos < end
            {
                self.stats.late += 1;
                return Verdict::Late;
            }
            self.stats.duplicates += 1;
            return Verdict::Duplicate;
        }
        let prev_end = head.after_previous();
        let mut gaps = Gaps::NONE;
        if prev_end > state.expect {
            match delivery {
                Delivery::Counted => {
                    gaps.push(Gap {
                        run,
                        at: state.expect,
                        units: prev_end - state.expect,
                        cause: Cause::Link,
                    });
                    state.link_gap = Some((state.expect, prev_end));
                }
                Delivery::Reliable => {
                    let from = state.expect;
                    self.stats.out_of_order += 1;
                    let slot = &mut self.resend[run_index(run)];
                    if slot.is_none_or(|r| r.from != from) {
                        *slot = Some(Resend {
                            from,
                            sent_ms: None,
                        });
                    }
                    return Verdict::OutOfOrder;
                }
            }
        }
        let lost_at = prev_end.max(state.expect);
        gaps.push(Gap {
            run,
            at: lost_at,
            units: head.pos - lost_at,
            cause: Cause::Device,
        });
        state.expect = head.end();
        let gaps = self.seen.clip(gaps);
        self.stats.count(&gaps);
        self.backfill_progress |= run == Run::Backfill;
        let slot = &mut self.resend[run_index(run)];
        if slot.is_some_and(|r| r.from < head.end()) {
            *slot = None;
        }
        let mut backfill_done = false;
        if let Some(b) = &mut self.backfill
            && run == Run::Backfill
            && b.run.expect >= b.end
            && !b.complete
        {
            b.complete = true;
            backfill_done = true;
        }
        if self.seen.contains(head.pos, head.end()) {
            self.stats.duplicates += 1;
            // the reported gaps count as had too, so acknowledgements move over all of it
            for gap in gaps.iter() {
                self.seen.add(gap.at, gap.at + gap.units);
            }
            self.skip_seen(run);
            return Verdict::Had {
                gaps,
                backfill_done,
            };
        }
        self.stats.received += head.units;
        Verdict::Accept {
            run,
            gaps,
            backfill_done,
        }
    }

    /// User code took the event with `head` (`bytes` of item): with [AckMode::OnReceive] it is acknowledged.
    pub fn delivered(&mut self, head: &StreamHead, bytes: usize) {
        self.bytes_since_ack += bytes as u64;
        if self.bytes_since_ack >= self.window as u64 / 2 {
            self.ack_due = true;
        }
        if self.ack_mode == AckMode::OnReceive {
            self.advance_ack(head.pos, head.end());
        }
    }

    /// User code confirms every unit before `pos` (an event boundary), see [AckMode::Explicit].
    pub fn ack(&mut self, pos: u64) {
        self.advance_ack(pos.saturating_sub(1), pos);
    }

    fn advance_ack(&mut self, in_run_at: u64, to: u64) {
        let run = self.run_of(in_run_at);
        let state = match run {
            Run::Live => &mut self.live,
            Run::Backfill => &mut self.backfill.as_mut().expect("run_of").run,
        };
        state.acked = state.acked.max(to.min(state.expect));
        self.skip_seen(run);
    }

    /// Move a run's acknowledged position over ranges the host had before a resume.
    fn skip_seen(&mut self, run: Run) {
        let state = match (run, &mut self.backfill) {
            (Run::Backfill, Some(b)) => &mut b.run,
            _ => &mut self.live,
        };
        loop {
            let acked = state.acked;
            for &(a, b) in &self.seen.ranges[..self.seen.len] {
                if a <= state.acked && state.acked < b {
                    state.acked = b.min(state.expect);
                }
            }
            if state.acked == acked {
                break;
            }
        }
    }

    /// The device asks where the host is (`Query`): a resend when it sent more than arrived, and an `Ack` right
    /// away. A `Query` without `backfill` while a backfill runs means the device dropped the rest of the backlog:
    /// returned as a gap, the backfill run ends.
    pub fn on_query(&mut self, sent: u64, backfill: Option<u64>) -> Gaps {
        if self.session.is_none() || self.delivery == Delivery::Counted {
            return Gaps::NONE;
        }
        self.ack_due = true;
        let mut gaps = Gaps::NONE;
        match (&mut self.backfill, backfill) {
            (Some(b), Some(b_sent)) if !b.complete && b_sent > b.run.expect => {
                self.resend[1] = Some(Resend {
                    from: b.run.expect,
                    sent_ms: None,
                });
            }
            (Some(b), None) if !b.complete => {
                gaps.push(Gap {
                    run: Run::Backfill,
                    at: b.run.expect,
                    units: b.end - b.run.expect,
                    cause: Cause::Store,
                });
                gaps = self.seen.clip(gaps);
                b.run.expect = b.end;
                b.run.acked = b.end;
                b.complete = true;
                self.resend[1] = None;
            }
            _ => {}
        }
        if self.resend[0].is_none() && sent > self.live.expect {
            self.resend[0] = Some(Resend {
                from: self.live.expect,
                sent_ms: None,
            });
        }
        self.stats.count(&gaps);
        gaps
    }

    /// Sideband to send now: a `ResendFrom` (reliable, after a gap), or an `Ack` (reliable: after opening, every
    /// half window of bytes taken, at least every ack interval while acknowledgements move on, and when the device
    /// asked).
    pub fn poll(&mut self, now_ms: u64) -> Option<StreamSideband> {
        if self.session.is_none() || self.delivery == Delivery::Counted {
            return None;
        }
        let grant = UVlq32(self.window);
        for resend in self.resend.iter_mut().flatten() {
            let due = resend
                .sent_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= self.resend_timeout_ms);
            if due {
                resend.sent_ms = Some(now_ms);
                self.stats.resend_requests += 1;
                return Some(StreamSideband::ResendFrom {
                    from: UVlq64(resend.from),
                    grant,
                });
            }
        }
        let backfill = self.backfill.as_ref().map(|b| b.run.acked);
        let moved = (self.live.acked, backfill) != self.last_acked;
        let interval = self
            .last_ack_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= self.ack_interval_ms);
        // a backfill run that stopped coming: say where it stands again (the device may have ended it and the
        // Query that said so got lost)
        let stalled = self.is_backfilling()
            && !core::mem::take(&mut self.backfill_progress)
            && self
                .last_ack_ms
                .is_some_and(|t| now_ms.saturating_sub(t) >= self.resend_timeout_ms);
        if !(self.ack_due || (moved && interval) || stalled) {
            return None;
        }
        self.ack_due = false;
        self.bytes_since_ack = 0;
        self.last_ack_ms = Some(now_ms);
        self.last_acked = (self.live.acked, backfill);
        if let Some(b) = &self.backfill
            && b.complete
            && b.run.acked >= b.end
        {
            // this Ack says the backlog is all here; later ones carry no backfill, which says the same
            self.backfill = None;
        }
        Some(StreamSideband::Ack {
            have: UVlq64(self.live.acked),
            backfill: backfill.map(UVlq64),
            grant,
        })
    }
}

fn run_index(run: Run) -> usize {
    match run {
        Run::Live => 0,
        Run::Backfill => 1,
    }
}
