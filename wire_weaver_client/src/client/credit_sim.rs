//! Host [CreditTx] and device [SinkCredit] connected through a link that drops, duplicates and reorders on a seeded
//! schedule, with reconnects that open a new session. Sessions start just below a multiple of the wire modulus, far
//! past 4 GiB too, so positions wrap on the wire in every one of them. Every session's accepted items must be a prefix
//! of what the host sent in it, the last one all of it.

use super::credit::{CreditTx, Due};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use wire_weaver::shrink_wrap::{BufReader, DeserializeShrinkWrap};
use ww_client_server::credit::{CreditVerdict, POS_MODULUS, SinkCredit};
use ww_client_server::{CreditWrite, StreamSideband};

const M: u64 = POS_MODULUS as u64;
const MAX_ITEM: u32 = 40;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// True with `per_mille` / 1000 probability.
    fn chance(&mut self, per_mille: u64) -> bool {
        self.next() % 1000 < per_mille
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

#[derive(Debug, Clone, Copy)]
struct Faults {
    drop: u64,
    dup: u64,
    reorder: u64,
}

const UDP: Faults = Faults {
    drop: 100,
    dup: 50,
    reorder: 100,
};

#[derive(Debug, Clone)]
enum ToDevice {
    /// Open, with the start of the session the device has to agree on (the protocol always starts at 0)
    Open(u64),
    Write(Vec<u8>),
    Sideband(StreamSideband),
}

/// One direction of the link: messages reach the other side on the next tick, in a shuffled order. Nothing is moved
/// before an Open: a late Open after writes would reset the session, a link that duplicates Opens needs session ids.
struct Link<T> {
    queue: VecDeque<T>,
    faults: Faults,
    /// Messages before this index are not reordered with later ones
    barrier: usize,
}

impl<T: Clone> Link<T> {
    fn new(faults: Faults) -> Self {
        Link {
            queue: VecDeque::new(),
            faults,
            barrier: 0,
        }
    }

    fn send(&mut self, rng: &mut Rng, msg: T, barrier: bool) {
        if rng.chance(self.faults.drop) {
            return;
        }
        if barrier {
            self.queue.push_back(msg);
            self.barrier = self.queue.len();
            return;
        }
        if rng.chance(self.faults.dup) {
            self.queue.push_back(msg.clone());
        }
        self.queue.push_back(msg);
        let n = self.queue.len();
        if n >= self.barrier + 2 && rng.chance(self.faults.reorder) {
            let other = rng.range(self.barrier as u64, n as u64 - 2) as usize;
            self.queue.swap(n - 1, other);
        }
    }

    fn take(&mut self) -> Vec<T> {
        self.barrier = 0;
        self.queue.drain(..).collect()
    }
}

/// What happened over all seeds, so the faults are known to have hit.
#[derive(Default)]
struct Seen {
    gaps: usize,
    duplicates: usize,
    queries: usize,
    wraps: usize,
}

struct Device {
    credit: SinkCredit,
    /// Items accepted, per session start
    sessions: Vec<(u64, Items)>,
    buffered: u32,
}

impl Device {
    fn take(
        &mut self,
        rng: &mut Rng,
        msg: ToDevice,
        out: &mut Vec<StreamSideband>,
        seen: &mut Seen,
    ) {
        match msg {
            ToDevice::Open(start) => {
                self.credit.open_at(start);
                // an Open sent again starts the same session over
                if self.sessions.last().is_some_and(|(s, _)| *s == start) {
                    self.sessions.pop();
                }
                self.sessions.push((start, Vec::new()));
                self.buffered = 0;
            }
            ToDevice::Write(bytes) => {
                let mut rd = BufReader::new(&bytes);
                let write = CreditWrite::des_shrink_wrap(&mut rd).unwrap();
                match self.credit.on_write(write.pos.0, write.data.0.len() as u32) {
                    CreditVerdict::Accept => {
                        self.sessions
                            .last_mut()
                            .unwrap()
                            .1
                            .push(write.data.0.to_vec());
                        self.buffered += write.data.0.len() as u32;
                        // the write handler sometimes consumes right away
                        if rng.chance(300) {
                            self.free();
                        }
                    }
                    CreditVerdict::Gap(Some(sb)) => {
                        seen.gaps += 1;
                        out.push(sb);
                    }
                    CreditVerdict::Gap(None) => seen.gaps += 1,
                    CreditVerdict::Duplicate => seen.duplicates += 1,
                    CreditVerdict::Invalid => panic!("invalid position"),
                }
            }
            ToDevice::Sideband(StreamSideband::CreditQuery { sent }) => {
                seen.queries += 1;
                out.push(self.credit.query(sent.0));
                return;
            }
            ToDevice::Sideband(other) => panic!("{other:?}"),
        }
        out.extend(self.credit.take_credit());
    }

    fn free(&mut self) {
        self.credit.free(self.buffered);
        self.buffered = 0;
    }
}

/// Items of one session, in order.
type Items = Vec<Vec<u8>>;

/// Sessions of one seed: (items sent, items accepted) per session.
fn run(seed: u64, seen: &mut Seen) -> Vec<(Items, Items)> {
    let mut rng = Rng::new(seed);
    let capacity = rng.range(MAX_ITEM as u64, 400) as u32;
    let mut device = Device {
        credit: SinkCredit::sized_for(capacity, Some(MAX_ITEM)),
        sessions: Vec::new(),
        buffered: 0,
    };
    let mut to_device = Link::new(UDP);
    let mut to_host = Link::new(UDP);
    let mut now = Instant::now();
    let mut host = CreditTx::new(now);
    host.config.query_interval = Duration::from_millis(20);
    host.config.stall_timeout = Some(Duration::from_secs(60));

    let sessions = rng.range(1, 4);
    let mut sent: Vec<(u64, Items)> = Vec::new();
    for session in 0..sessions {
        // a reconnect: what was in flight is gone
        to_device.take();
        to_host.take();
        // near a multiple of the modulus, sometimes far past 4 GiB
        let start = loop {
            let wraps = [1, 2, 17, 1 << 12][rng.range(0, 3) as usize];
            // at most as far below the wrap as a last session's items take at least
            let start = wraps * M - rng.range(1, 300);
            if sent.iter().all(|(s, _)| *s != start) {
                break start;
            }
        };
        host.closed();
        host.opening_at(now, start);
        to_device.send(&mut rng, ToDevice::Open(start), true);
        sent.push((start, Vec::new()));
        let last = session + 1 == sessions;
        // a session that isn't the last one ends early, at a random point
        let items = if last { 300 } else { rng.range(0, 300) };
        let mut plan: VecDeque<Vec<u8>> = (0..items)
            .map(|_| {
                let len = rng.range(0, MAX_ITEM as u64) as usize;
                (0..len).map(|_| rng.next() as u8).collect()
            })
            .collect();
        let mut ticks = 0;
        loop {
            ticks += 1;
            assert!(ticks < 200_000, "seed {seed}: no progress");
            now += Duration::from_millis(1);
            for sb in to_host.take() {
                host.on_sideband(sb, now);
            }
            let mut wrote = false;
            for _ in 0..rng.range(1, 8) {
                if let Some(bytes) = host.next_resend().unwrap() {
                    to_device.send(&mut rng, ToDevice::Write(bytes), false);
                    wrote = true;
                } else if let Some(item) = plan.front() {
                    match host.try_new(item).unwrap() {
                        Some(bytes) => {
                            to_device.send(&mut rng, ToDevice::Write(bytes), false);
                            sent.last_mut().unwrap().1.push(plan.pop_front().unwrap());
                            wrote = true;
                        }
                        None => break,
                    }
                } else {
                    break;
                }
            }
            if !wrote {
                match host.due(now) {
                    Some(Due::Open) => to_device.send(&mut rng, ToDevice::Open(start), true),
                    Some(Due::Query(sb)) => to_device.send(&mut rng, ToDevice::Sideband(sb), false),
                    Some(Due::Stalled) => panic!("seed {seed}: stalled"),
                    None => {}
                }
            }
            let mut out = Vec::new();
            for msg in to_device.take() {
                device.take(&mut rng, msg, &mut out, seen);
            }
            // the device consumes on its own, outside the write handler
            if rng.chance(200) {
                device.free();
                out.extend(device.credit.take_credit());
            }
            for sb in out {
                to_host.send(&mut rng, sb, false);
            }
            if plan.is_empty() && host.all_acked() {
                break;
            }
            if !last && rng.chance(5) {
                break;
            }
        }
        seen.wraps += (host.acked() / M - start / M) as usize;
        if last {
            assert_eq!(host.sent(), host.acked());
            assert!(host.acked() / M > start / M, "seed {seed}: wrapped");
            assert!(host.sent() >= start, "positions are not reset");
        }
    }
    // a session cut short may never have reached the device (all its Opens lost)
    sent.into_iter()
        .map(|(start, items)| {
            let accepted = device
                .sessions
                .iter()
                .find(|(s, _)| *s == start)
                .map(|(_, items)| items.clone())
                .unwrap_or_default();
            (items, accepted)
        })
        .collect()
}

#[test]
fn lossy_link_across_wraps_many_seeds() {
    let mut seen = Seen::default();
    for seed in 0..60 {
        let sessions = run(seed, &mut seen);
        let (sent, accepted) = sessions.last().unwrap();
        assert_eq!(accepted, sent, "seed {seed}: last session complete");
        for (i, (sent, accepted)) in sessions.iter().enumerate() {
            assert!(
                accepted.len() <= sent.len() && accepted[..] == sent[..accepted.len()],
                "seed {seed}, session {i}: accepted items are a prefix of the sent ones"
            );
        }
    }
    assert!(seen.gaps > 100 && seen.duplicates > 100 && seen.queries > 10);
    assert!(seen.wraps >= 60);
}
