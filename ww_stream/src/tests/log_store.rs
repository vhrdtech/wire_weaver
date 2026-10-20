//! LogStore on simulated SD and NOR media: the StreamStore contract, reboots, a power cut at every write, S3, S5.

use super::{Rng, block_on};
use crate::store::log::{
    LogConfig, LogError, LogMedium, LogStore, SimError, SimMedium, SimNor, SimSd,
};
use crate::store::{FullPolicy, PushError, StreamStore};
use crate::{HeadExt, StreamHead};
use wire_weaver::shrink_wrap::UVlq32;

type Store<M> = LogStore<M, 512>;

const SESSION: u32 = 0x1234;

fn sd(size: usize) -> SimSd {
    SimSd::new(size, 1)
}

fn nor(size: usize) -> SimNor {
    SimNor::new(size, 1)
}

fn config(segment_size: u32, policy: FullPolicy) -> LogConfig {
    LogConfig {
        segment_size,
        policy,
    }
}

fn mount<M: LogMedium>(medium: M, cfg: LogConfig) -> Store<M>
where
    M::Error: core::fmt::Debug,
{
    block_on(Store::mount(medium, cfg, SESSION)).unwrap()
}

/// Event `i` of a test stream: head and item, positions from `start`.
#[derive(Debug, Clone, PartialEq)]
struct Ev {
    head: StreamHead,
    item: Vec<u8>,
}

fn time(i: u64) -> ww_si::Second {
    ww_si::Second::from_parts(
        ww_si::Prefix::Micro,
        ww_si::ww_numeric::NumericValue::U32((i * 1000) as u32),
    )
}

/// A stream of events: varied sizes, units, an occasional `lost` and a timestamp on every event.
struct Gen {
    i: u64,
    next: u64,
}

impl Gen {
    fn at(next: u64) -> Self {
        Gen { i: 0, next }
    }

    fn event(&mut self) -> Ev {
        let i = self.i;
        self.i += 1;
        let lost = (i % 11 == 5).then_some(3);
        let pos = self.next + lost.unwrap_or(0);
        let units = 1 + (i % 5) * 7;
        self.next = pos + units;
        let len = 1 + (i * 13 % 61) as usize;
        Ev {
            head: StreamHead {
                pos,
                units,
                lost,
                t: Some(time(i)),
                ext: None,
            },
            item: (0..len).map(|j| (i as usize * 7 + j) as u8).collect(),
        }
    }
}

fn push<M: LogMedium>(s: &mut Store<M>, ev: &Ev) -> Result<(), PushError<LogError<M::Error>>> {
    block_on(s.push(&ev.head, &ev.item))
}

fn read<M: LogMedium>(s: &mut Store<M>, pos: u64) -> Option<Ev>
where
    M::Error: core::fmt::Debug,
{
    let mut buf = [0u8; 256];
    block_on(s.read(pos, &mut buf)).unwrap().map(|e| Ev {
        head: e.head,
        item: e.item.to_vec(),
    })
}

/// Everything the store gives back, reading from `from` on as a sender does.
fn read_all<M: LogMedium>(s: &mut Store<M>, from: u64) -> Vec<Ev>
where
    M::Error: core::fmt::Debug,
{
    let mut out = Vec::new();
    let mut pos = from;
    while let Some(ev) = read(s, pos) {
        pos = ev.head.end().max(ev.head.pos + 1);
        out.push(ev);
    }
    out
}

/// What the sender stored, with the clock epoch the store added to the first event after a mount.
fn with_epoch(mut ev: Ev, epoch: u32) -> Ev {
    ev.head.ext = Some(HeadExt {
        codec_restart: false,
        clock_epoch: Some(UVlq32(epoch)),
    });
    ev
}

fn contract<M: LogMedium>(medium: M, segment_size: u32)
where
    M::Error: core::fmt::Debug,
{
    let mut s = mount(medium, config(segment_size, FullPolicy::DropNewest));
    assert!(s.recovery().formatted);
    let st = s.state();
    assert_eq!(
        (st.session, st.oldest, st.newest, st.next),
        (SESSION, None, None, 0)
    );
    assert!(st.persistent);
    assert_eq!(read(&mut s, 0), None);

    let mut g = Gen::at(0);
    let evs: Vec<Ev> = (0..40).map(|_| g.event()).collect();
    for ev in &evs {
        push(&mut s, ev).unwrap();
    }
    let st = s.state();
    assert_eq!(st.oldest, Some(evs[0].head.pos));
    assert_eq!(st.newest, Some(evs[39].head.pos));
    assert_eq!(st.next, evs[39].head.end());
    assert_eq!(read_all(&mut s, 0), evs);

    // inside an event: the event that holds it; random access after sequential reads
    let mid = &evs[21];
    assert!(mid.head.units > 1);
    assert_eq!(read(&mut s, mid.head.pos + 1), Some(mid.clone()));
    assert_eq!(read(&mut s, evs[3].head.pos), Some(evs[3].clone()));
    assert_eq!(read(&mut s, evs[39].head.end() - 1), Some(evs[39].clone()));
    assert_eq!(read(&mut s, evs[39].head.end()), None);
    // forward again from inside an event, after the hint moved past it
    assert_eq!(read(&mut s, evs[22].head.pos), Some(evs[22].clone()));
    assert_eq!(read(&mut s, evs[21].head.end() - 1), Some(evs[21].clone()));

    // release: events that end at or before the position go
    block_on(s.release(evs[10].head.end())).unwrap();
    assert_eq!(s.state().oldest, Some(evs[11].head.pos));
    assert_eq!(read(&mut s, 0), Some(evs[11].clone()));
    assert!(evs[26].head.units > 1);
    block_on(s.release(evs[26].head.pos + 1)).unwrap();
    assert_eq!(
        s.state().oldest,
        Some(evs[26].head.pos),
        "an event spanning the position is kept"
    );
    block_on(s.release(evs[39].head.end())).unwrap();
    let st = s.state();
    assert_eq!(
        (st.oldest, st.newest, st.next),
        (None, None, evs[39].head.end())
    );
    assert_eq!(read(&mut s, 0), None);

    // buffer too small
    let ev = g.event();
    push(&mut s, &ev).unwrap();
    let mut small = [0u8; 0];
    assert!(matches!(
        block_on(s.read(0, &mut small)),
        Err(LogError::BufferTooSmall { .. })
    ));
    // too big for a segment
    let big = vec![0u8; segment_size as usize];
    assert!(matches!(
        block_on(s.push(&StreamHead::at(ev.head.end()), &big)),
        Err(PushError::TooBig)
    ));
}

#[test]
fn contract_sd() {
    contract(sd(512 * 40), 2048);
}

#[test]
fn contract_nor() {
    contract(nor(256 * 40), 1024);
}

fn drop_newest<M: LogMedium>(medium: M, segment_size: u32)
where
    M::Error: core::fmt::Debug,
{
    let mut s = mount(medium, config(segment_size, FullPolicy::DropNewest));
    let mut g = Gen::at(0);
    let mut stored = Vec::new();
    let mut lost_units = 0;
    for _ in 0..400 {
        let ev = g.event();
        match push(&mut s, &ev) {
            Ok(()) => stored.push(ev),
            Err(PushError::Full) => lost_units += ev.head.units,
            Err(e) => panic!("{e:?}"),
        }
    }
    assert!(lost_units > 0, "the store filled up");
    assert_eq!(
        s.state().dropped_units,
        0,
        "refused events are not dropped ones"
    );
    assert!(s.state().used_bytes <= s.state().capacity_bytes);
    // what is kept is the start, contiguous
    assert_eq!(read_all(&mut s, 0), stored);

    // releasing the oldest segments makes room again
    let half = stored[stored.len() / 2].head.pos;
    block_on(s.release(half)).unwrap();
    let ev = g.event();
    push(&mut s, &ev).unwrap();
    assert_eq!(read_all(&mut s, half).last(), Some(&ev));
}

#[test]
fn drop_newest_sd() {
    drop_newest(sd(512 * 20), 1024);
}

#[test]
fn drop_newest_nor() {
    drop_newest(nor(256 * 20), 512);
}

fn drop_oldest<M: LogMedium>(medium: M, segment_size: u32)
where
    M::Error: core::fmt::Debug,
{
    let mut s = mount(medium, config(segment_size, FullPolicy::DropOldest));
    let mut g = Gen::at(0);
    let mut stored = Vec::new();
    for _ in 0..600 {
        let ev = g.event();
        push(&mut s, &ev).unwrap();
        stored.push(ev);
    }
    let st = s.state();
    let oldest = st.oldest.unwrap();
    assert!(oldest > 0, "the oldest moved on");
    let kept = read_all(&mut s, 0);
    let first = stored.iter().position(|e| e.head.pos == oldest).unwrap();
    assert_eq!(kept, stored[first..]);
    let forgotten: u64 = stored[..first].iter().map(|e| e.head.units).sum();
    assert_eq!(st.dropped_units, forgotten);
    assert_eq!(st.newest, Some(stored.last().unwrap().head.pos));

    // released units don't count as dropped
    let mut s2 = mount(
        SimSd::new(512 * 12, 2),
        config(1024, FullPolicy::DropOldest),
    );
    let mut g = Gen::at(0);
    for _ in 0..300 {
        let ev = g.event();
        push(&mut s2, &ev).unwrap();
        block_on(s2.release(ev.head.end())).unwrap();
    }
    assert_eq!(s2.state().dropped_units, 0);
    assert_eq!(s2.state().oldest, None);
}

#[test]
fn drop_oldest_sd() {
    drop_oldest(sd(512 * 20), 1024);
}

#[test]
fn drop_oldest_nor() {
    drop_oldest(nor(256 * 20), 512);
}

fn reboot<const W: usize, const E: usize, const NOR: bool>(
    s: Store<SimMedium<W, E, NOR>>,
    cfg: LogConfig,
) -> Store<SimMedium<W, E, NOR>> {
    let image = s.into_medium().image();
    mount(SimMedium::from_image(image, 9), cfg)
}

fn reboots<const W: usize, const E: usize, const NOR: bool>(
    medium: SimMedium<W, E, NOR>,
    segment_size: u32,
) {
    let cfg = config(segment_size, FullPolicy::DropNewest);
    let mut s = mount(medium, cfg);
    let mut g = Gen::at(0);
    let mut stored = Vec::new();
    for _ in 0..30 {
        let ev = g.event();
        push(&mut s, &ev).unwrap();
        stored.push(ev);
    }
    block_on(s.release(stored[4].head.end())).unwrap();
    let before = s.state();
    assert_eq!(s.clock_epoch(), 0);

    let mut s = reboot(s, cfg);
    let rec = s.recovery();
    assert!(!rec.formatted);
    assert_eq!(rec.torn_records, 0);
    assert_eq!(rec.clock_epoch, 1);
    let after = s.state();
    assert_eq!(after.session, before.session);
    assert_eq!(after.next, before.next);
    assert_eq!(after.newest, before.newest);
    // the released position comes back as of the last superblock: never after what was released
    assert!(after.oldest <= before.oldest);
    assert_eq!(read_all(&mut s, before.oldest.unwrap()), stored[5..]);

    // a reboot without pushing still starts a new epoch
    let mut s = reboot(s, cfg);
    assert_eq!(s.clock_epoch(), 2);
    let ev = g.event();
    push(&mut s, &ev).unwrap();
    stored.push(with_epoch(ev, 2));
    let ev = g.event();
    push(&mut s, &ev).unwrap();
    stored.push(ev);
    assert_eq!(read_all(&mut s, before.oldest.unwrap()), stored[5..]);

    // reset: a new session, positions from 0, also after a reboot
    block_on(s.reset(77)).unwrap();
    assert_eq!(read(&mut s, 0), None);
    let mut s = reboot(s, cfg);
    let st = s.state();
    assert_eq!((st.session, st.oldest, st.next), (77, None, 0));
    let ev = Gen::at(0).event();
    push(&mut s, &ev).unwrap();
    assert_eq!(read_all(&mut s, 0), vec![with_epoch(ev, 3)]);
}

#[test]
fn reboots_sd() {
    reboots(sd(512 * 40), 2048);
}

#[test]
fn reboots_nor() {
    reboots(nor(256 * 40), 1024);
}

#[test]
fn geometry_and_format() {
    let cfg = config(2048, FullPolicy::DropNewest);
    let s = mount(sd(512 * 40), cfg);
    let image = s.into_medium().image();
    // the same medium with another segment size
    assert!(matches!(
        block_on(Store::mount(
            SimSd::from_image(image.clone(), 1),
            config(1024, FullPolicy::DropNewest),
            SESSION
        )),
        Err(LogError::Geometry)
    ));
    assert!(matches!(
        block_on(Store::mount(
            sd(512 * 40),
            config(1000, FullPolicy::DropNewest),
            SESSION
        )),
        Err(LogError::Geometry)
    ));
    assert!(matches!(
        block_on(Store::mount(sd(512 * 6), cfg, SESSION)),
        Err(LogError::Geometry)
    ));
    // format starts over, nothing of the old log reads back
    let mut s = mount(SimSd::from_image(image, 1), cfg);
    let mut g = Gen::at(0);
    for _ in 0..50 {
        push(&mut s, &g.event()).unwrap();
    }
    let image = s.into_medium().image();
    let s = block_on(Store::format(SimSd::from_image(image, 1), cfg, 5)).unwrap();
    let mut s = reboot(s, cfg);
    let st = s.state();
    assert_eq!((st.session, st.oldest, st.next), (5, None, 0));
    assert_eq!(read(&mut s, 0), None);
}

/// The workload of the power cut test: pushes, a release now and then. Returns the events whose push returned
/// `Ok` and the one being pushed when the power failed.
fn workload<M: LogMedium<Error = SimError>>(
    s: &mut Store<M>,
    g: &mut Gen,
    n: usize,
    acked: &mut Vec<Ev>,
    released: &mut u64,
) -> Option<Ev> {
    for i in 0..n {
        let ev = g.event();
        match push(s, &ev) {
            Ok(()) => acked.push(ev),
            Err(PushError::Full) => {}
            Err(PushError::Store(LogError::Medium(SimError::PowerCut))) => return Some(ev),
            Err(e) => panic!("{e:?}"),
        }
        if i % 9 == 8 {
            let to = acked[acked.len() / 2].head.pos;
            match block_on(s.release(to)) {
                Ok(()) => *released = to,
                Err(LogError::Medium(SimError::PowerCut)) => return None,
                Err(e) => panic!("{e:?}"),
            }
        }
    }
    None
}

/// Checks a log read back after a power cut against what was pushed: a run of the pushed events (bit-identical, in
/// order) that ends with the last acked event or the one in flight, and that holds every acked event not released
/// (drop newest). Returns whether the event in flight is there.
fn check_after_cut(
    got: &[Ev],
    acked: &[Ev],
    in_flight: Option<&Ev>,
    released: u64,
    policy: FullPolicy,
    what: &str,
) -> bool {
    let mut all = acked.to_vec();
    all.extend(in_flight.cloned());
    let unreleased = acked.iter().position(|e| e.head.end() > released);
    let Some(first) = got.first() else {
        assert!(
            unreleased.is_none() || policy == FullPolicy::DropOldest,
            "{what}: log empty, {} acked",
            acked.len()
        );
        return false;
    };
    let start = all
        .iter()
        .position(|e| e.head.pos == first.head.pos)
        .unwrap_or_else(|| panic!("{what}: unknown event {first:?}"));
    assert_eq!(got[..], all[start..start + got.len()], "{what}");
    let end = start + got.len();
    assert!(
        end >= acked.len(),
        "{what}: acked events lost ({end} of {})",
        acked.len()
    );
    if policy == FullPolicy::DropNewest {
        // released ones may come back (the superblock lags behind), unreleased ones never go
        assert!(
            unreleased.is_none_or(|u| start <= u),
            "{what}: unreleased events lost"
        );
    }
    end > acked.len()
}

/// Power cut at every write and erase of the workload: after the reboot every event whose push returned is there,
/// bit-identical and in order; the one being pushed is there or not, and counted when found cut short. Then the
/// stream goes on from where the store says, with a second power cut at a random point of that (the recovery mount
/// included), and a clean reboot after it still has everything acked in both runs.
fn power_cut_everywhere<const W: usize, const E: usize, const NOR: bool>(
    size: usize,
    segment_size: u32,
    policy: FullPolicy,
    n: usize,
) {
    type Sim<const W: usize, const E: usize, const NOR: bool> = SimMedium<W, E, NOR>;
    let cfg = config(segment_size, policy);
    let ops = {
        let mut s = mount(Sim::<W, E, NOR>::new(size, 3), cfg);
        let mut acked = Vec::new();
        workload(&mut s, &mut Gen::at(0), n, &mut acked, &mut 0);
        let st = s.state();
        // the workload goes round the ring
        match policy {
            FullPolicy::DropOldest => assert!(st.dropped_units > 0),
            FullPolicy::DropNewest => assert!(acked.len() < n, "some pushes were refused"),
        }
        s.into_medium().ops()
    };
    let mut torn_seen = 0;
    let mut in_flight_kept = 0;
    let mut rng = Rng::new(7);
    for cut in 0..ops {
        let what = format!("cut {cut}");
        let mut medium = Sim::<W, E, NOR>::new(size, 100 + cut);
        medium.cut_power_at(cut);
        let Ok(mut s) = block_on(Store::mount(medium, cfg, SESSION)) else {
            continue; // cut while formatting: nothing was stored
        };
        let mut g = Gen::at(0);
        let mut acked = Vec::new();
        let mut released = 0;
        let in_flight = workload(&mut s, &mut g, n, &mut acked, &mut released);
        let image = s.into_medium().image();

        let mut s = mount(Sim::<W, E, NOR>::from_image(image.clone(), rng.next()), cfg);
        let rec = s.recovery();
        assert!(rec.torn_records <= 1, "{what}: {rec:?}");
        torn_seen += rec.torn_records;
        assert_eq!(s.state().session, SESSION, "{what}");
        let got = read_all(&mut s, 0);
        if check_after_cut(&got, &acked, in_flight.as_ref(), released, policy, &what) {
            in_flight_kept += 1;
        }
        let st = s.state();
        if let Some(last) = got.last() {
            assert_eq!(st.next, last.head.end(), "{what}");
        }
        let continue_at = st.next;
        let mut kept: Vec<Ev> = got.clone();

        // again from the same image, with a second power cut somewhere in the mount or the pushes after it
        let mut medium = Sim::<W, E, NOR>::from_image(image, rng.next());
        let cut2 = rng.range(0, 30);
        medium.cut_power_at(cut2);
        let what = format!("cut {cut}, then {cut2}");
        let mut more = Vec::new();
        let mut in_flight2 = None;
        if let Ok(mut s) = block_on(Store::mount(medium, cfg, SESSION)) {
            let epoch = s.clock_epoch();
            let mut g = Gen::at(s.state().next);
            g.i = 1000;
            for i in 0..12 {
                let mut ev = g.event();
                if i == 0 {
                    ev = with_epoch(ev, epoch);
                }
                match push(&mut s, &ev) {
                    Ok(()) => more.push(ev),
                    Err(PushError::Full) => {}
                    Err(PushError::Store(LogError::Medium(SimError::PowerCut))) => {
                        in_flight2 = Some(ev);
                        break;
                    }
                    Err(e) => panic!("{what}: {e:?}"),
                }
            }
            let image = s.into_medium().image();
            let mut s = mount(Sim::<W, E, NOR>::from_image(image, rng.next()), cfg);
            assert!(s.recovery().torn_records <= 1, "{what}");
            let got2 = read_all(&mut s, 0);
            // the first run's log, then the second run's events
            kept.retain(|e| got2.first().is_some_and(|f| e.head.pos >= f.head.pos));
            let mut acked2 = kept.clone();
            acked2.extend(more.iter().cloned());
            check_after_cut(
                &got2,
                &acked2,
                in_flight2.as_ref(),
                0,
                FullPolicy::DropOldest,
                &what,
            );
            if policy == FullPolicy::DropNewest && !more.is_empty() {
                let mut with_flight = more.clone();
                with_flight.extend(in_flight2.clone());
                assert!(
                    got2.ends_with(&more) || got2.ends_with(&with_flight),
                    "{what}"
                );
            }
            assert!(
                more.first().is_none_or(|m| m.head.pos >= continue_at),
                "{what}"
            );
        }
    }
    assert!(torn_seen > 0, "some cut left a torn record");
    assert!(
        in_flight_kept > 0,
        "some cut came after the record was whole"
    );
}

#[test]
fn power_cut_nor_drop_newest() {
    power_cut_everywhere::<4, 256, true>(256 * 22, 256, FullPolicy::DropNewest, 200);
}

#[test]
fn power_cut_nor_drop_oldest() {
    power_cut_everywhere::<4, 256, true>(256 * 22, 256, FullPolicy::DropOldest, 200);
}

#[test]
fn power_cut_sd_drop_newest() {
    power_cut_everywhere::<512, 512, false>(512 * 22, 1024, FullPolicy::DropNewest, 400);
}

#[test]
fn power_cut_sd_drop_oldest() {
    power_cut_everywhere::<512, 512, false>(512 * 22, 1024, FullPolicy::DropOldest, 400);
}

/// S3: a logger samples while no host is there. The store fills (drop newest), what doesn't fit is counted; the
/// host comes back with its cursor and gets the backlog from there, timestamps as made, then acknowledges it and
/// the logger has room again.
#[test]
fn s3_logger_host_gone() {
    let cfg = config(2048, FullPolicy::DropNewest);
    let mut s = mount(sd(512 * 64), cfg);
    let mut g = Gen::at(0);
    // the host read the first 20 events, then left
    let mut stored = Vec::new();
    for _ in 0..20 {
        let ev = g.event();
        push(&mut s, &ev).unwrap();
        stored.push(ev);
    }
    let cursor = stored[19].head.end();
    block_on(s.release(cursor)).unwrap();
    // host gone: sample until full
    let mut refused = 0u64;
    for _ in 0..2000 {
        let ev = g.event();
        match push(&mut s, &ev) {
            Ok(()) => stored.push(ev),
            Err(PushError::Full) => refused += ev.head.units,
            Err(e) => panic!("{e:?}"),
        }
    }
    assert!(refused > 0);
    // the host resumes with its cursor: everything after it, as made
    let backlog = read_all(&mut s, cursor);
    assert_eq!(backlog, stored[20..]);
    assert!(backlog.iter().all(|e| e.head.t.is_some()));
    // the gap after the backlog is the units the store refused; the sender puts them in `lost` of its next event
    assert_eq!(s.state().next, backlog.last().unwrap().head.end());
    // acknowledged: room again
    block_on(s.release(s.state().next)).unwrap();
    assert_eq!(s.state().oldest, None);
    push(&mut s, &g.event()).unwrap();
}

/// S3 with drop oldest: the host comes back with a cursor older than what is kept and learns exactly how much is
/// gone: the units from its cursor to the oldest kept event are the dropped ones (and the events' own `lost`).
#[test]
fn s3_cursor_older_than_kept() {
    let mut s = mount(nor(256 * 24), config(512, FullPolicy::DropOldest));
    let mut g = Gen::at(0);
    let mut stored = Vec::new();
    for _ in 0..1000 {
        let ev = g.event();
        push(&mut s, &ev).unwrap();
        stored.push(ev);
    }
    let st = s.state();
    let oldest = st.oldest.unwrap();
    let first = read(&mut s, 0).unwrap();
    assert_eq!(first.head.pos, oldest);
    let gone = &stored[..stored.iter().position(|e| e.head.pos == oldest).unwrap()];
    assert!(!gone.is_empty());
    assert_eq!(
        st.dropped_units,
        gone.iter().map(|e| e.head.units).sum::<u64>()
    );
    // from cursor 0 to the oldest kept event: the dropped units and the `lost` before each event
    let lost: u64 = gone.iter().map(|e| e.head.lost_units()).sum::<u64>() + first.head.lost_units();
    assert_eq!(oldest, st.dropped_units + lost);
}

/// S5: the logger reboots while the host is away. Session and positions are kept, the first event after each
/// reboot carries the new clock epoch, and the host gets one session with the epoch changes in it.
#[test]
fn s5_reboot_keeps_session_marks_epoch() {
    let cfg = config(1024, FullPolicy::DropNewest);
    let mut s = mount(sd(512 * 64), cfg);
    let mut g = Gen::at(0);
    let mut stored = Vec::new();
    for boot in 0..4u32 {
        if boot > 0 {
            s = reboot(s, cfg);
            assert_eq!(s.clock_epoch(), boot);
            assert_eq!(s.state().session, SESSION);
            assert_eq!(s.state().next, g.next);
        }
        for i in 0..15 {
            let ev = g.event();
            push(&mut s, &ev).unwrap();
            stored.push(if boot > 0 && i == 0 {
                with_epoch(ev, boot)
            } else {
                ev
            });
        }
    }
    let got = read_all(&mut s, 0);
    assert_eq!(got, stored);
    let epochs: Vec<(u64, u32)> = got
        .iter()
        .filter_map(|e| Some((e.head.pos, e.head.ext?.clock_epoch?.0)))
        .collect();
    assert_eq!(epochs.len(), 3);
    assert_eq!(epochs.iter().map(|e| e.1).collect::<Vec<_>>(), [1, 2, 3]);
    // a sender that marks the epoch itself keeps its own marker
    let mut s = reboot(s, cfg);
    let mut ev = g.event();
    ev.head.ext = Some(HeadExt {
        codec_restart: false,
        clock_epoch: Some(UVlq32(99)),
    });
    push(&mut s, &ev).unwrap();
    assert_eq!(read(&mut s, ev.head.pos), Some(ev));
}

#[test]
fn many_segments_since_superblock() {
    // a mount walks the segment headers opened after the last superblock
    let cfg = config(512, FullPolicy::DropOldest);
    let mut s = mount(sd(512 * 200), cfg);
    let mut g = Gen::at(0);
    let mut last = None;
    for _ in 0..3000 {
        let ev = g.event();
        push(&mut s, &ev).unwrap();
        last = Some(ev);
    }
    let st = s.state();
    let mut s = reboot(s, cfg);
    assert!(s.recovery().segments_walked < 8);
    assert_eq!(s.state().next, st.next);
    assert_eq!(s.state().oldest, st.oldest);
    assert_eq!(read(&mut s, last.as_ref().unwrap().head.pos), last);
}
