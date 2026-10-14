//! Device and host state machines connected through a link that drops, duplicates and reorders on a seeded
//! schedule; every unit the producer made must arrive exactly once or be reported in a gap at its exact place.

use super::{Rng, block_on};
use crate::rx::{AckMode, Cause, Delivery, Gap, Receiver, Verdict};
use crate::sched::{Priority, Scheduler};
use crate::store::{FullPolicy, RamRing, StreamStore};
use crate::tx::{Counted, Pushed, Reliable};
use crate::{Opened, Run, StreamCursor, StreamHead};
use std::collections::VecDeque;
use ww_client_server::StreamSideband;

#[derive(Debug, Clone, Copy, Default)]
struct Faults {
    drop: u64,
    dup: u64,
    reorder: u64,
}

impl Faults {
    const NONE: Faults = Faults {
        drop: 0,
        dup: 0,
        reorder: 0,
    };
    const UDP: Faults = Faults {
        drop: 10,
        dup: 5,
        reorder: 10,
    };
}

/// One direction of the link.
struct Link<T> {
    queue: VecDeque<T>,
    faults: Faults,
}

impl<T: Clone> Link<T> {
    fn new(faults: Faults) -> Self {
        Link {
            queue: VecDeque::new(),
            faults,
        }
    }

    fn send(&mut self, rng: &mut Rng, msg: T) {
        if rng.chance(self.faults.drop) {
            return;
        }
        if rng.chance(self.faults.dup) {
            self.queue.push_back(msg.clone());
        }
        self.queue.push_back(msg);
        let n = self.queue.len();
        if n >= 2 && rng.chance(self.faults.reorder) {
            let other = rng.range(0, n as u64 - 2) as usize;
            self.queue.swap(n - 1, other);
        }
    }

    fn take(&mut self) -> Vec<T> {
        self.queue.drain(..).collect()
    }
}

#[derive(Debug, Clone)]
enum ToHost {
    Item(StreamHead, Vec<u8>),
    Sideband(StreamSideband),
}

/// What user code on the host got.
#[derive(Default)]
struct Got {
    items: Vec<(Run, StreamHead, Vec<u8>)>,
    gaps: Vec<Gap>,
    backfill_done: usize,
}

impl Got {
    fn units_of(&self, run: Run) -> Vec<(u64, u64)> {
        self.items
            .iter()
            .filter(|(r, ..)| *r == run)
            .map(|(_, h, _)| (h.pos, h.end()))
            .collect()
    }
}

fn host_takes(rx: &mut Receiver, got: &mut Got, msg: ToHost, ack_now: bool) {
    match msg {
        ToHost::Item(head, item) => match rx.on_item(&head) {
            Verdict::Accept {
                run,
                gaps,
                backfill_done,
            } => {
                got.gaps.extend(gaps.iter().copied());
                got.backfill_done += backfill_done as usize;
                // the item carries its own position: proves the right bytes came with the head
                assert_eq!(u64::from_le_bytes(item[..8].try_into().unwrap()), head.pos);
                if ack_now {
                    rx.delivered(&head, item.len());
                }
                got.items.push((run, head, item));
            }
            Verdict::Had {
                gaps,
                backfill_done,
            } => {
                got.gaps.extend(gaps.iter().copied());
                got.backfill_done += backfill_done as usize;
            }
            Verdict::Duplicate | Verdict::Late | Verdict::OutOfOrder => {}
            Verdict::NotOpen => panic!("not open"),
        },
        ToHost::Sideband(StreamSideband::Query { sent, backfill }) => {
            let gaps = rx.on_query(sent.0, backfill.map(|b| b.0));
            got.gaps.extend(gaps.iter().copied());
        }
        ToHost::Sideband(other) => panic!("{other:?}"),
    }
}

struct ReliableSim<const N: usize> {
    rng: Rng,
    dev: Reliable<RamRing<N>, 8>,
    rx: Receiver,
    down: Link<ToHost>,
    up: Link<StreamSideband>,
    got: Got,
    now: u64,
    /// (pos, end) of every event the producer stored
    made: Vec<(u64, u64)>,
}

impl<const N: usize> ReliableSim<N> {
    fn new(seed: u64, store: RamRing<N>, down: Faults, up: Faults) -> Self {
        ReliableSim {
            rng: Rng::new(seed),
            dev: Reliable::new(store).initial_grant(256),
            rx: Receiver::new(Delivery::Reliable).window(1024),
            down: Link::new(down),
            up: Link::new(up),
            got: Got::default(),
            now: 0,
            made: Vec::new(),
        }
    }

    fn produce(&mut self, events: usize) {
        for _ in 0..events {
            let units = self.rng.range(1, 4);
            let pos = self.dev.next_pos();
            let mut item = pos.to_le_bytes().to_vec();
            item.resize(self.rng.range(8, 24) as usize, 0xaa);
            if block_on(self.dev.push(&item, units, None)).unwrap() == Pushed::Stored {
                self.made.push((pos, pos + units));
            }
        }
    }

    fn open(&mut self, session: u32) {
        let opened = block_on(self.dev.open(session)).unwrap();
        self.rx.opened(None, &opened);
    }

    fn resume(&mut self) -> Opened {
        let cursor = self.rx.cursor().unwrap();
        self.resume_from(cursor)
    }

    fn resume_from(&mut self, cursor: StreamCursor) -> Opened {
        let opened = block_on(self.dev.resume(cursor)).unwrap();
        let before = format!("{:?}", self.rx);
        let (gaps, _) = self.rx.opened(Some(cursor), &opened);
        if std::env::var("SIM_TRACE").is_ok() {
            std::eprintln!(
                "RESUME {cursor:?} -> {opened:?} gaps {gaps:?}\n  before {before}\n  after {:?}",
                self.rx
            );
        }
        self.got.gaps.extend(gaps.iter().copied());
        opened
    }

    fn disconnect(&mut self) {
        self.dev.detach();
        self.rx.close();
        self.down.take();
        self.up.take();
    }

    /// One tick: the device sends what it may, live before backfill, the link carries, the host answers.
    fn step(&mut self) {
        let mut buf = [0u8; 64];
        let mut sched = Scheduler::<1>::new(200);
        sched.start_turn();
        while let Some((_, run)) = sched.pick(&[self.dev.ready()]) {
            match block_on(self.dev.next_event(run, &mut buf)).unwrap() {
                Some(ev) => {
                    sched.sent(ev.item.len());
                    self.down
                        .send(&mut self.rng, ToHost::Item(ev.head, ev.item.to_vec()));
                }
                None => sched.skip(0, run),
            }
        }
        if let Some(q) = self.dev.poll_query(self.now) {
            self.down.send(&mut self.rng, ToHost::Sideband(q));
        }
        for msg in self.down.take() {
            host_takes(&mut self.rx, &mut self.got, msg, true);
        }
        while let Some(sb) = self.rx.poll(self.now) {
            self.up.send(&mut self.rng, sb);
        }
        for sb in self.up.take() {
            block_on(self.dev.on_sideband(&sb, self.now)).unwrap();
        }
        self.now += 10;
    }

    /// Run until the host has everything the device stored, or panic.
    fn drain(&mut self) {
        for _ in 0..20_000 {
            if self.dev.in_flight() == 0
                && !self.dev.ready().live
                && !self.dev.ready().backfill
                && !self.rx.is_backfilling()
            {
                return;
            }
            self.step();
        }
        panic!(
            "no progress: in flight {}, ready {:?}, {}",
            self.dev.in_flight(),
            self.dev.ready(),
            self.debug()
        );
    }

    fn debug(&self) -> String {
        format!("rx {:?} dev {}", self.rx, self.dev.debug_state())
    }
}

/// Every made event arrived once, in order within its run, nothing reported missing.
fn assert_complete(made: &[(u64, u64)], got: &Got) {
    let mut all: Vec<(u64, u64)> = got.items.iter().map(|(_, h, _)| (h.pos, h.end())).collect();
    for run in [Run::Live, Run::Backfill] {
        let units = got.units_of(run);
        assert!(
            units.windows(2).all(|w| w[0].1 <= w[1].0),
            "{run:?} out of order"
        );
    }
    all.sort();
    assert_eq!(all, made);
}

#[test]
fn reliable_clean_link() {
    let mut sim = ReliableSim::new(1, RamRing::<8192>::new(), Faults::NONE, Faults::NONE);
    sim.open(1);
    for _ in 0..50 {
        sim.produce(5);
        sim.step();
    }
    sim.drain();
    assert_complete(&sim.made, &sim.got);
    assert!(sim.got.gaps.is_empty());
    assert_eq!(sim.dev.stats().resent, 0);
    assert_eq!(sim.dev.stats().resend_requests, 0);
    assert_eq!(sim.rx.stats().received, sim.dev.next_pos());
    assert!(sim.dev.store().state().oldest.is_none(), "all released");
}

#[test]
fn reliable_lossy_link_many_seeds() {
    for seed in 0..40 {
        let mut sim = ReliableSim::new(seed, RamRing::<16384>::new(), Faults::UDP, Faults::UDP);
        sim.open(1);
        for _ in 0..60 {
            sim.produce(4);
            sim.step();
        }
        sim.drain();
        assert_complete(&sim.made, &sim.got);
        assert!(sim.got.gaps.is_empty(), "seed {seed}: {:?}", sim.got.gaps);
        assert_eq!(sim.rx.stats().received, sim.dev.next_pos(), "seed {seed}");
        assert!(
            sim.dev.stats().resent > 0,
            "seed {seed}: the link lost something"
        );
    }
}

#[test]
fn reliable_very_lossy_link() {
    let faults = Faults {
        drop: 40,
        dup: 20,
        reorder: 30,
    };
    for seed in 100..110 {
        let mut sim = ReliableSim::new(seed, RamRing::<16384>::new(), faults, faults);
        sim.open(1);
        for _ in 0..30 {
            sim.produce(4);
            sim.step();
        }
        sim.drain();
        assert_complete(&sim.made, &sim.got);
    }
}

#[test]
fn reliable_positions_past_u32_and_2_pow_50() {
    let mut sim = ReliableSim::new(7, RamRing::<8192>::new(), Faults::UDP, Faults::UDP);
    sim.open(1);
    sim.produce(3);
    sim.dev.lost(u32::MAX as u64);
    sim.produce(3);
    for _ in 0..5 {
        sim.step();
    }
    sim.dev.lost(1 << 50);
    sim.produce(10);
    sim.drain();
    let lost: u64 = sim.got.gaps.iter().map(|g| g.units).sum();
    assert_eq!(lost, u32::MAX as u64 + (1 << 50));
    assert!(
        sim.got.gaps.iter().all(|g| g.cause == Cause::Device),
        "{:?}",
        sim.got.gaps
    );
    let received: u64 = sim.got.items.iter().map(|(_, h, _)| h.units).sum();
    assert_eq!(received + lost, sim.dev.next_pos());
    assert!(sim.dev.next_pos() > 1 << 50);
}

#[test]
fn reliable_host_stuck_store_absorbs() {
    // the host stops taking data: no acks, the device stops at the grant, the store fills, nothing is lost
    let mut sim = ReliableSim::new(3, RamRing::<16384>::new(), Faults::NONE, Faults::NONE);
    sim.open(1);
    let mut stuck = Vec::new();
    for _ in 0..20 {
        sim.produce(5);
        // device sends, host receives but user code doesn't take: no `delivered`
        let mut buf = [0u8; 64];
        while let Some(ev) = block_on(sim.dev.next_event(Run::Live, &mut buf)).unwrap() {
            let msg = ToHost::Item(ev.head, ev.item.to_vec());
            stuck.push(msg);
        }
    }
    assert!(
        sim.dev.in_flight() <= 256 + 24,
        "grant respected: {}",
        sim.dev.in_flight()
    );
    assert!(sim.dev.store().state().used_bytes > 2000);
    for msg in stuck {
        host_takes(&mut sim.rx, &mut sim.got, msg, true);
    }
    sim.drain();
    assert_complete(&sim.made, &sim.got);
}

#[test]
fn reliable_resume_after_disconnect_with_backfill() {
    for seed in 0..20 {
        let mut sim = ReliableSim::new(seed, RamRing::<32768>::new(), Faults::UDP, Faults::UDP);
        sim.open(1);
        for _ in 0..20 {
            sim.produce(3);
            sim.step();
        }
        sim.disconnect();
        // the host is gone for a while, the device keeps storing
        sim.produce(100);
        let opened = sim.resume();
        assert!(opened.resume_from.is_some(), "seed {seed}");
        // the latest value comes first
        let before = sim.got.items.len();
        sim.step();
        let first_live = sim.got.items[before..]
            .iter()
            .find(|(r, ..)| *r == Run::Live)
            .map(|(_, h, _)| h.pos);
        if !sim.rx.is_backfilling() {
            continue;
        }
        for _ in 0..30 {
            sim.produce(2);
            sim.step();
        }
        sim.drain();
        assert_eq!(sim.got.backfill_done, 1, "seed {seed}");
        assert!(sim.got.gaps.is_empty(), "seed {seed}: {:?}", sim.got.gaps);
        // everything arrived, possibly twice: what was in flight at the disconnect comes again in the backlog
        let mut all: Vec<_> = sim
            .got
            .items
            .iter()
            .map(|(_, h, _)| (h.pos, h.end()))
            .collect();
        all.sort();
        all.dedup();
        assert_eq!(all, sim.made, "seed {seed}");
        assert!(first_live.is_none_or(|p| p >= opened.live_from));
    }
}

#[test]
fn reliable_live_first_during_backfill() {
    // a logger with a backlog: the live run's first event is the newest one, and live events go out before
    // backfill events on every turn
    let mut sim = ReliableSim::new(5, RamRing::<32768>::new(), Faults::NONE, Faults::NONE);
    block_on(sim.dev.start_session(9)).unwrap();
    sim.produce(200);
    let newest = sim.made.last().unwrap().0;
    let opened = sim.resume_from(StreamCursor {
        session: wire_weaver::shrink_wrap::UVlq32(9),
        pos: wire_weaver::shrink_wrap::UVlq64(0),
    });
    assert_eq!(opened.live_from, newest);
    assert_eq!(opened.resume_from, Some(0));
    sim.step();
    assert_eq!(sim.got.items[0].0, Run::Live);
    assert_eq!(sim.got.items[0].1.pos, newest);
    for _ in 0..200 {
        sim.produce(1);
        let before = sim.got.items.len();
        sim.step();
        let new = &sim.got.items[before..];
        let first_backfill = new.iter().position(|(r, ..)| *r == Run::Backfill);
        let last_live = new.iter().rposition(|(r, ..)| *r == Run::Live);
        if let (Some(b), Some(l)) = (first_backfill, last_live) {
            assert!(l < b, "live after backfill in one turn");
        }
    }
    sim.drain();
    assert_eq!(sim.got.backfill_done, 1);
    assert_complete(&sim.made, &sim.got);
}

#[test]
fn reliable_explicit_ack_crash_loses_nothing() {
    // the host receives, but confirms only what it saved; it crashes and resumes from the saved cursor
    let mut sim = ReliableSim::new(11, RamRing::<32768>::new(), Faults::NONE, Faults::NONE);
    sim.rx = Receiver::new(Delivery::Reliable)
        .window(1024)
        .ack_mode(AckMode::Explicit);
    sim.open(1);
    let mut saved = Vec::new();
    for i in 0..30 {
        sim.produce(3);
        sim.step();
        // save half of what came, confirm it
        if i % 2 == 0 {
            let (_, h, _) = sim.got.items.last().unwrap().clone();
            saved.extend(sim.got.items.drain(..).map(|(_, h, _)| (h.pos, h.end())));
            sim.rx.ack(h.end());
        }
    }
    let cursor = sim.rx.cursor().unwrap();
    // crash: a new host process with the saved cursor
    sim.disconnect();
    sim.got = Got::default();
    sim.rx = Receiver::new(Delivery::Reliable)
        .window(1024)
        .ack_mode(AckMode::Explicit);
    sim.resume_from(cursor);
    for _ in 0..2000 {
        if let Some((_, h, _)) = sim.got.items.last() {
            let end = h.end();
            saved.extend(sim.got.items.drain(..).map(|(_, h, _)| (h.pos, h.end())));
            sim.rx.ack(end);
        }
        sim.step();
        if sim.dev.in_flight() == 0 && !sim.dev.ready().live && !sim.dev.ready().backfill {
            break;
        }
    }
    saved.sort();
    assert_eq!(saved, sim.made);
}

#[test]
fn reliable_store_full_drop_newest_counted_exactly() {
    // no host for a while, a small store: what didn't fit is reported at its place, the rest arrives
    let mut sim = ReliableSim::new(13, RamRing::<2048>::new(), Faults::UDP, Faults::UDP);
    block_on(sim.dev.start_session(2)).unwrap();
    sim.produce(300);
    let lost = sim.dev.stats().lost_store_full;
    assert!(lost > 0);
    sim.resume_from(StreamCursor {
        session: wire_weaver::shrink_wrap::UVlq32(2),
        pos: wire_weaver::shrink_wrap::UVlq64(0),
    });
    for _ in 0..10 {
        sim.produce(3);
        sim.step();
    }
    sim.drain();
    // what didn't fit before the resume, and whatever didn't fit after it
    let reported: u64 = sim.got.gaps.iter().map(|g| g.units).sum();
    assert!(
        sim.got.gaps.iter().all(|g| g.cause == Cause::Device),
        "{:?}",
        sim.got.gaps
    );
    assert!(reported >= lost);
    assert_eq!(reported, sim.dev.stats().lost_store_full);
    let received: u64 = sim.got.items.iter().map(|(_, h, _)| h.units).sum();
    assert_eq!(received + reported, sim.dev.next_pos());
}

#[test]
fn reliable_store_full_drop_oldest_counted_exactly() {
    let mut sim = ReliableSim::new(
        17,
        RamRing::<2048>::new().with_policy(FullPolicy::DropOldest),
        Faults::UDP,
        Faults::UDP,
    );
    block_on(sim.dev.start_session(2)).unwrap();
    sim.produce(300);
    let lost = sim.dev.stats().lost_store_full;
    assert!(lost > 0);
    let opened = sim.resume_from(StreamCursor {
        session: wire_weaver::shrink_wrap::UVlq32(2),
        pos: wire_weaver::shrink_wrap::UVlq64(0),
    });
    assert_eq!(opened.resume_from, Some(opened.oldest));
    for _ in 0..10 {
        sim.produce(3);
        sim.step();
    }
    sim.drain();
    // the oldest were dropped: reported as the store's at the resume
    let store: u64 = sim
        .got
        .gaps
        .iter()
        .filter(|g| g.cause == Cause::Store)
        .map(|g| g.units)
        .sum();
    assert_eq!(store, lost);
    // later drops (events pushed out while in flight) arrive as device gaps, or the events themselves if they made
    // it before; every unit is accounted for exactly once
    let reported: u64 = sim.got.gaps.iter().map(|g| g.units).sum();
    assert!(reported <= sim.dev.stats().lost_store_full);
    let received: u64 = sim.got.items.iter().map(|(_, h, _)| h.units).sum();
    assert_eq!(received + reported, sim.dev.next_pos());
}

#[test]
fn scheduler_orders_two_streams() {
    // a high-priority stream gets the link first, every turn
    let mut hi = Reliable::<RamRing<4096>, 8>::new(RamRing::new())
        .priority(Priority::High)
        .initial_grant(10_000);
    let mut lo = Reliable::<RamRing<4096>, 8>::new(RamRing::new()).initial_grant(10_000);
    block_on(hi.open(1)).unwrap();
    block_on(lo.open(1)).unwrap();
    for i in 0..10u64 {
        block_on(hi.push(&i.to_le_bytes(), 1, None)).unwrap();
        block_on(lo.push(&i.to_le_bytes(), 1, None)).unwrap();
    }
    let mut sched = Scheduler::<2>::new(8 * 12);
    let mut order = Vec::new();
    let mut buf = [0u8; 16];
    for _ in 0..2 {
        sched.start_turn();
        while let Some((i, run)) = sched.pick(&[hi.ready(), lo.ready()]) {
            let stream = if i == 0 { &mut hi } else { &mut lo };
            match block_on(stream.next_event(run, &mut buf)).unwrap() {
                Some(ev) => {
                    sched.sent(ev.item.len());
                    order.push(i);
                }
                None => sched.skip(i, run),
            }
        }
    }
    assert_eq!(
        order,
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    );
}

/// Counted stream over a lossy link: every unit is received or in exactly one gap, at its exact place.
#[test]
fn counted_lossy_link_accounts_every_unit() {
    for seed in 0..40 {
        let mut rng = Rng::new(seed);
        let mut tx = Counted::new();
        let mut rx = Receiver::new(Delivery::Counted);
        let mut link = Link::new(Faults::UDP);
        let mut got = Got::default();
        rx.opened(None, &tx.open(1));
        let mut device_lost = 0;
        for _ in 0..500 {
            if rng.chance(5) {
                let n = rng.range(1, 50);
                tx.lost(n);
                device_lost += n;
            }
            let units = rng.range(1, 10);
            let head = tx.head(units, None).unwrap();
            if rng.chance(3) {
                // the medium refused it: the device counts it
                tx.unsent(&head);
                device_lost += units;
                continue;
            }
            link.send(
                &mut rng,
                ToHost::Item(head, head.pos.to_le_bytes().to_vec()),
            );
            if rng.chance(30) {
                for msg in link.take() {
                    host_takes(&mut rx, &mut got, msg, false);
                }
            }
        }
        // the last event tells where the stream is
        let last = tx.head(1, None).unwrap();
        for msg in link.take() {
            host_takes(&mut rx, &mut got, msg, false);
        }
        host_takes(
            &mut rx,
            &mut got,
            ToHost::Item(last, last.pos.to_le_bytes().to_vec()),
            false,
        );

        let mut covered: Vec<(u64, u64)> =
            got.items.iter().map(|(_, h, _)| (h.pos, h.end())).collect();
        covered.extend(got.gaps.iter().map(|g| (g.at, g.at + g.units)));
        covered.sort();
        // no overlap, no hole: [0, end) exactly once
        let mut at = 0;
        for (a, b) in &covered {
            assert_eq!(*a, at, "seed {seed}: {covered:?}");
            at = *b;
        }
        assert_eq!(at, tx.next_pos());
        let device: u64 = got
            .gaps
            .iter()
            .filter(|g| g.cause == Cause::Device)
            .map(|g| g.units)
            .sum();
        // device gaps are what the device dropped; units in events lost on the link with a `lost` inside are link
        // gaps from the host's view, so device gaps can only be fewer
        assert!(device <= device_lost, "seed {seed}");
        let st = rx.stats();
        assert_eq!(
            st.received + st.lost_device + st.lost_link,
            tx.next_pos(),
            "seed {seed}"
        );
    }
}

/// Random disconnects and resumes from the host's cursor, both store policies, a lossy link: every unit is either
/// received (maybe twice, after a resume) or reported in a gap, never both, never neither.
#[test]
fn reliable_chaos() {
    // SIM_SEEDS=5000 for a longer search
    let env = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    };
    let from = env("SIM_SEED_FROM", 0);
    for seed in from..from + env("SIM_SEEDS", 60) {
        let policy = if seed % 2 == 0 {
            FullPolicy::DropNewest
        } else {
            FullPolicy::DropOldest
        };
        let mut sim = ReliableSim::new(
            seed,
            RamRing::<3000>::new().with_policy(policy),
            Faults::UDP,
            Faults::UDP,
        );
        sim.open(1);
        let mut connected = true;
        for _ in 0..300 {
            let events = sim.rng.range(0, 4) as usize;
            sim.produce(events);
            if sim.rng.chance(2) {
                let units = sim.rng.range(1, 30);
                sim.dev.lost(units);
            }
            if connected {
                sim.step();
                if sim.rng.chance(3) {
                    sim.disconnect();
                    connected = false;
                }
            } else if sim.rng.chance(10) {
                sim.resume();
                connected = true;
            }
        }
        if !connected {
            sim.resume();
        }
        sim.drain();

        // units lost after the last stored event are reported with the next one, if there is one
        let end = sim.made.last().unwrap().1;
        let mut received: Vec<(u64, u64)> = sim
            .got
            .items
            .iter()
            .map(|(_, h, _)| (h.pos, h.end()))
            .collect();
        received.sort();
        received.dedup();
        let mut gaps: Vec<(u64, u64)> = sim
            .got
            .gaps
            .iter()
            .map(|g| (g.at, g.at + g.units))
            .collect();
        gaps.sort();
        let mut all: Vec<_> = received.iter().chain(gaps.iter()).copied().collect();
        all.sort();
        let mut at = 0;
        for (a, b) in &all {
            if *a != at {
                let near: Vec<_> = sim
                    .got
                    .items
                    .iter()
                    .filter(|(_, h, _)| h.end() + 30 > at && h.pos < at + 30)
                    .map(|(r, h, _)| (*r, h.pos, h.end(), h.lost))
                    .collect();
                let g: Vec<_> = sim
                    .got
                    .gaps
                    .iter()
                    .filter(|g| g.at + g.units + 30 > at && g.at < at + 30)
                    .collect();
                panic!(
                    "seed {seed}: hole or overlap at {at} ({a}..{b}): items {near:?} gaps {g:?}"
                );
            }
            at = *b;
        }
        // a resume reports units lost after the last stored event too
        assert!(
            at >= end && at <= sim.dev.next_pos(),
            "seed {seed}: {at} {end}"
        );
        assert!(
            sim.dev.store().state().oldest.is_none(),
            "seed {seed}: all released"
        );
    }
}
