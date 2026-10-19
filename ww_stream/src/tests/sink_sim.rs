//! Host `SinkTx` and device `SinkCredit` connected through a link that drops, duplicates and reorders on a seeded
//! schedule, opens included. On top: reconnects that resume the session, host restarts from a saved cursor, device
//! reboots (the session is lost, the host starts a new one), and sessions that start just below a multiple of the
//! wire modulus, far past 4 Gi units too, so positions wrap on the wire.
//!
//! Every run of the device (from an open to a reboot) must accept a contiguous run of the host's items at their
//! positions, nothing twice; the last session must arrive whole.

use super::Rng;
use crate::Opened;
use crate::StreamCursor;
use crate::sink::{
    POS_MODULUS, SinkConfig, SinkCredit, SinkDue, SinkOpenError, SinkSideband, SinkTx, SinkVerdict,
};
use std::collections::VecDeque;
use ww_client_server::StreamSideband;

const M: u64 = POS_MODULUS as u64;
const MAX_ITEM: u32 = 40;

#[derive(Debug, Clone, Copy)]
struct Faults {
    drop: u64,
    dup: u64,
    reorder: u64,
}

const UDP: Faults = Faults {
    drop: 10,
    dup: 5,
    reorder: 10,
};

#[derive(Debug, Clone)]
enum ToDevice {
    /// `StreamOpen` with the cursor; `start` is where a new session starts (the protocol starts at 0, the sim near
    /// a wrap)
    Open {
        cursor: StreamCursor,
        start: u64,
    },
    Write {
        pos: u32,
        units: u64,
        data: Vec<u8>,
    },
    Query(u64),
    Close,
}

#[derive(Debug, Clone)]
enum ToHost {
    Opened(Opened),
    Sideband(StreamSideband),
}

/// One direction of the link: messages reach the other side on the next tick, in a shuffled order.
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

/// What happened over all seeds, so the faults are known to have hit.
#[derive(Default, Debug)]
struct Seen {
    gaps: usize,
    duplicates: usize,
    queries: usize,
    wraps: usize,
    reopens_kept: usize,
    reconnects: usize,
    host_restarts: usize,
    device_reboots: usize,
    sessions_lost: usize,
    closes: usize,
}

/// One run of the device: from an open that started a session to a reboot (or the end).
#[derive(Debug)]
struct DeviceRun {
    session: u32,
    start: u64,
    /// Items accepted, with their positions
    items: Vec<(u64, Vec<u8>)>,
}

struct Device {
    credit: SinkCredit,
    capacity: u32,
    runs: Vec<DeviceRun>,
    buffered: u32,
}

impl Device {
    fn new(capacity: u32) -> Self {
        Device {
            credit: SinkCredit::sized_for(capacity, Some(MAX_ITEM)),
            capacity,
            runs: Vec::new(),
            buffered: 0,
        }
    }

    fn reboot(&mut self) {
        self.credit = SinkCredit::sized_for(self.capacity, Some(MAX_ITEM));
        self.buffered = 0;
    }

    fn take(&mut self, rng: &mut Rng, msg: ToDevice, out: &mut Vec<ToHost>, seen: &mut Seen) {
        match msg {
            ToDevice::Open { cursor, start } => {
                let opened = if self.credit.session() == Some(cursor.session.0) {
                    seen.reopens_kept += 1;
                    self.credit.open(cursor)
                } else {
                    let opened = self.credit.open_at(cursor.session.0, start);
                    self.buffered = 0;
                    self.runs.push(DeviceRun {
                        session: cursor.session.0,
                        start,
                        items: Vec::new(),
                    });
                    opened
                };
                out.push(ToHost::Opened(opened));
            }
            ToDevice::Write { pos, units, data } => {
                let before = self.credit.have();
                match self.credit.on_write(pos, units, data.len() as u32) {
                    SinkVerdict::Accept => {
                        self.runs
                            .last_mut()
                            .unwrap()
                            .items
                            .push((before, data.clone()));
                        self.buffered += data.len() as u32;
                        // the write handler sometimes consumes right away
                        if rng.chance(30) {
                            self.free();
                        }
                    }
                    SinkVerdict::Gap(sb) => {
                        seen.gaps += 1;
                        out.extend(sb.map(ToHost::Sideband));
                    }
                    SinkVerdict::NoSession(sb) => out.extend(sb.map(ToHost::Sideband)),
                    SinkVerdict::Duplicate => seen.duplicates += 1,
                    SinkVerdict::Invalid => panic!("invalid write"),
                }
            }
            ToDevice::Query(sent) => {
                seen.queries += 1;
                out.extend(self.credit.on_query(sent).map(ToHost::Sideband));
                return;
            }
            ToDevice::Close => self.credit.close(),
        }
        out.extend(self.credit.take_ack().map(ToHost::Sideband));
    }

    fn free(&mut self) {
        self.credit.free(self.buffered);
        self.buffered = 0;
    }
}

/// The host's user code: the items of one session and their positions.
struct Plan {
    session: u32,
    start: u64,
    items: Vec<(Vec<u8>, u64)>,
    /// Position of each item's first unit
    positions: Vec<u64>,
    /// Next item to hand to the sink
    next: usize,
}

impl Plan {
    fn new(rng: &mut Rng, session: u32, start: u64, n: u64, byte_sink: bool) -> Self {
        let mut items = Vec::new();
        let mut positions = Vec::new();
        let mut pos = start;
        for _ in 0..n {
            let len = rng.range(0, MAX_ITEM as u64) as usize;
            let data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            // a byte sink names the payload's bytes (one less than the item, for a length prefix); typed: 1
            let units = if byte_sink {
                (len as u64).saturating_sub(1).max(1)
            } else {
                1
            };
            positions.push(pos);
            pos += units;
            items.push((data, units));
        }
        Plan {
            session,
            start,
            items,
            positions,
            next: 0,
        }
    }

    fn end(&self) -> u64 {
        self.positions
            .last()
            .map_or(self.start, |p| p + self.items.last().unwrap().1)
    }

    /// User code continues from `pos`, where the device is: an item boundary.
    fn continue_at(&mut self, pos: u64) {
        self.next = if pos == self.end() {
            self.items.len()
        } else {
            self.positions
                .binary_search(&pos)
                .expect("an item boundary")
        };
    }
}

/// Runs one seed; returns the plans of all sessions and the device's runs.
fn run(seed: u64, seen: &mut Seen) -> (Vec<Plan>, Vec<DeviceRun>) {
    let mut rng = Rng::new(seed);
    let capacity = rng.range(MAX_ITEM as u64, 400) as u32;
    let byte_sink = rng.chance(50);
    let mut device = Device::new(capacity);
    let mut to_device = Link::new(UDP);
    let mut to_host = Link::new(UDP);
    let config = SinkConfig {
        query_interval_ms: 20,
        stall_timeout_ms: Some(60_000),
    };
    let mut host = SinkTx::new(config);
    let mut now = 0u64;
    let mut plans: Vec<Plan> = Vec::new();
    let mut next_session = rng.range(1, 1000) as u32;
    let sessions = rng.range(1, 4);
    let mut session = 0;
    while session < sessions {
        session += 1;
        let last = session == sessions;
        // a new session: what was in flight for the old one is gone (a late open of an older session would start
        // that one again, the limit of host-named sessions without a close that every packet sees)
        to_device.take();
        to_host.take();
        if host.session().is_some() {
            host.closed();
            to_device.send(&mut rng, ToDevice::Close);
        }
        let start = loop {
            let wraps = [0, 1, 2, 17, 1 << 12][rng.range(0, 4) as usize];
            let start = (wraps * M).saturating_sub(rng.range(1, 300));
            if plans.iter().all(|p| p.start != start) {
                break start;
            }
        };
        let id = next_session;
        next_session += 1;
        let items = if last { 300 } else { rng.range(0, 300) };
        let mut plan = Plan::new(&mut rng, id, start, items, byte_sink);
        let mut cursor = host.open_at(id, start, now);
        to_device.send(&mut rng, ToDevice::Open { cursor, start });
        let mut saved = cursor;
        let mut ticks = 0;
        let mut lost = false;
        let mut restarted = false;
        loop {
            ticks += 1;
            assert!(ticks < 400_000, "seed {seed}: no progress");
            now += 1;
            for msg in to_host.take() {
                match msg {
                    ToHost::Opened(opened) => match host.opened(&opened, Some(capacity), now) {
                        Ok(at) => {
                            // a restarted host has nothing kept: user code goes on from where the device is; after
                            // a reconnect the sink resends its own copies
                            if restarted {
                                plan.continue_at(at);
                                restarted = false;
                            }
                        }
                        Err(SinkOpenError::Stale) => {}
                        Err(SinkOpenError::SessionLost { .. }) => {
                            seen.sessions_lost += 1;
                            lost = true;
                        }
                        Err(e @ SinkOpenError::Ahead { .. }) => panic!("seed {seed}: {e:?}"),
                    },
                    ToHost::Sideband(sb) => {
                        if host.on_sideband(&sb, now) == SinkSideband::DeviceClosed {
                            seen.closes += 1;
                            cursor = host.resume(now).unwrap();
                            to_device.send(&mut rng, ToDevice::Open { cursor, start });
                        }
                    }
                }
            }
            if lost {
                break;
            }
            let mut wrote = false;
            for _ in 0..rng.range(1, 8) {
                if let Some(w) = host.next_resend() {
                    let msg = write_msg(&w);
                    to_device.send(&mut rng, msg);
                    wrote = true;
                } else if let Some((data, units)) = plan.items.get(plan.next) {
                    match host.try_new(data, *units).unwrap() {
                        Some(w) => {
                            assert_eq!(w.pos.0 as u64, plan.positions[plan.next] % M);
                            let msg = write_msg(&w);
                            to_device.send(&mut rng, msg);
                            plan.next += 1;
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
                    Some(SinkDue::Open(c)) => {
                        to_device.send(&mut rng, ToDevice::Open { cursor: c, start })
                    }
                    Some(SinkDue::Query(StreamSideband::Query { sent, .. })) => {
                        to_device.send(&mut rng, ToDevice::Query(sent.0))
                    }
                    Some(SinkDue::Query(other)) => panic!("{other:?}"),
                    Some(SinkDue::Stalled) => panic!("seed {seed}: stalled"),
                    None => {}
                }
            }
            let mut out = Vec::new();
            for msg in to_device.take() {
                device.take(&mut rng, msg, &mut out, seen);
            }
            // the device consumes on its own, outside the write handler
            if rng.chance(20) {
                device.free();
                out.extend(device.credit.take_ack().map(ToHost::Sideband));
            }
            for msg in out {
                to_host.send(&mut rng, msg);
            }
            if host.is_synced() && rng.chance(2) {
                saved = host.cursor().unwrap();
            }
            if plan.next == plan.items.len() && host.all_acked() {
                break;
            }
            // trouble, now and then
            if rng.chance(1) {
                match rng.range(0, 9) {
                    0..=4 => {
                        // a reconnect: what was in flight is gone, the host resumes its session
                        seen.reconnects += 1;
                        to_device.take();
                        to_host.take();
                        host.link_lost();
                        cursor = host.resume(now).unwrap();
                        to_device.send(&mut rng, ToDevice::Open { cursor, start });
                    }
                    5..=7 => {
                        // the host process restarts from the cursor it saved with its data
                        seen.host_restarts += 1;
                        to_device.take();
                        to_host.take();
                        host = SinkTx::new(config);
                        cursor = host.resume_from(saved, now);
                        plan.continue_at(saved.pos.0);
                        restarted = true;
                        to_device.send(&mut rng, ToDevice::Open { cursor, start });
                    }
                    _ if !last => {
                        // the device reboots: its session is gone
                        seen.device_reboots += 1;
                        device.reboot();
                    }
                    _ => {}
                }
            }
            if !last && rng.chance(1) {
                break;
            }
        }
        seen.wraps += (host.acked() / M).saturating_sub(start / M) as usize;
        if lost {
            // user code starts over in a new session
            if last {
                session -= 1;
            }
        } else if last {
            assert_eq!(host.sent(), host.acked());
            assert_eq!(host.acked(), plan.end(), "seed {seed}");
        }
        plans.push(plan);
    }
    (plans, device.runs)
}

fn write_msg(w: &crate::sink::SinkWriteOut<'_>) -> ToDevice {
    ToDevice::Write {
        pos: w.pos.0,
        units: w.units.map_or(1, |u| u.0),
        data: w.data.to_vec(),
    }
}

#[test]
fn lossy_link_sessions_resume_many_seeds() {
    let seeds: u64 = std::env::var("SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let mut seen = Seen::default();
    for seed in 0..seeds {
        let (plans, runs) = run(seed, &mut seen);
        for (i, r) in runs.iter().enumerate() {
            let plan = plans.iter().find(|p| p.session == r.session).unwrap();
            // contiguous from where the run started, each item the one the host has at that position
            let mut expect = r.items.first().map_or(r.start, |(p, _)| *p);
            for (pos, data) in &r.items {
                assert_eq!(*pos, expect, "seed {seed}, run {i}: contiguous");
                let idx = plan.positions.binary_search(pos).expect("an item boundary");
                assert_eq!(
                    data, &plan.items[idx].0,
                    "seed {seed}, run {i}: the host's item"
                );
                expect = pos + plan.items[idx].1;
            }
        }
        // the last session arrives whole, in one run of the device
        let last = plans.last().unwrap();
        let last_runs: Vec<&DeviceRun> =
            runs.iter().filter(|r| r.session == last.session).collect();
        assert_eq!(
            last_runs.len(),
            1,
            "seed {seed}: no reboot in the last session"
        );
        let got: Vec<&Vec<u8>> = last_runs[0].items.iter().map(|(_, d)| d).collect();
        let want: Vec<&Vec<u8>> = last.items.iter().map(|(d, _)| d).collect();
        assert_eq!(got, want, "seed {seed}: last session complete");
    }
    eprintln!("{seen:?}");
    if seeds >= 60 {
        assert!(
            seen.gaps > 100 && seen.duplicates > 100 && seen.queries > 10,
            "{seen:?}"
        );
        assert!(seen.wraps >= 30, "{seen:?}");
        assert!(
            seen.reopens_kept > 50 && seen.reconnects > 10 && seen.host_restarts > 10,
            "{seen:?}"
        );
        assert!(seen.device_reboots > 3 && seen.closes > 3, "{seen:?}");
    }
}
