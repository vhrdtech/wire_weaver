use super::*;
use wire_weaver::shrink_wrap::RefVec;

const DESC: LevelDesc<'static> = LevelDesc {
    item_count: 6,
    observable: &[1, 3, 4],
    batched: &[false; 3],
};

/// Like [DESC], index 3 `+batched` (API-39).
const DESC_B: LevelDesc<'static> = LevelDesc {
    item_count: 6,
    observable: &[1, 3, 4],
    batched: &[false, true, false],
};

/// Like [DESC], indices 1 and 3 `+batched`.
const DESC_BB: LevelDesc<'static> = LevelDesc {
    item_count: 6,
    observable: &[1, 3, 4],
    batched: &[true, true, false],
};

const LIMITS: ObserveLimits = ObserveLimits {
    min_period_us: 100,
    default_budget: 1_000,
    max_level_budget: 10_000,
    total_budget: 15_000,
    timestamps: false,
    samples_max_age_us: (1_000, 1_000_000),
    samples_min_every_us: 10,
};

fn entry(index: u32, mode: ObserveMode) -> ObserveEntry {
    ObserveEntry {
        index: UNib32(index),
        mode,
    }
}

fn periodic(every_us: u32) -> ObserveMode {
    ObserveMode::Periodic {
        every_us: UNib32(every_us),
    }
}

fn on_change(min_us: u32) -> ObserveMode {
    ObserveMode::OnChange {
        min_us: UNib32(min_us),
    }
}

fn apply(
    table: &mut ObserveTable,
    desc: &LevelDesc<'_>,
    entries: &[ObserveEntry],
    patch: bool,
    budget: Option<u32>,
) -> Result<Vec<ObserveRefused>, ObserveError> {
    let req = ObserveRequest {
        entries: RefVec::Slice { slice: entries },
        patch,
        budget: budget.map(UNib32),
    };
    let mut refused = [ObserveRefused {
        index: UNib32(0),
        reason: ObserveRefusal::NoSuchIndex,
    }; 4];
    let len = table.apply(&req, desc, &LIMITS, 0, &mut refused)?;
    Ok(refused[..len].to_vec())
}

fn granted(table: &ObserveTable, desc: &LevelDesc<'_>) -> Vec<ObserveEntry> {
    let mut out = vec![entry(0, ObserveMode::Off); table.len()];
    let len = table.granted(desc, &mut out);
    out[..len].to_vec()
}

/// One poll as the generated code does it, with value sizes `size(k)`, events of 4 bytes plus data: returns the
/// chosen slots and the event length (0 when nothing was sent).
fn poll(
    table: &mut ObserveTable,
    desc: &LevelDesc<'_>,
    now: u64,
    size: impl Fn(usize) -> usize,
) -> (Vec<usize>, usize) {
    let mut due = vec![Due::EMPTY; table.len()];
    let mut marks = vec![Mark::NONE; table.len()];
    let plan = table.plan(now, &LIMITS, &mut due);
    if plan.is_empty() {
        return (vec![], 0);
    }
    let mut sel = plan.selection(desc, 4, 1024, &mut marks);
    for k in plan.edf() {
        if !sel.offer(k, Some(size(k))) {
            break;
        }
    }
    let chosen: Vec<usize> = (0..table.len()).filter(|k| sel.is_chosen(*k)).collect();
    let len = if chosen.is_empty() {
        0
    } else {
        4 + chosen.iter().map(|k| size(*k)).sum::<usize>()
    };
    table.commit(&plan, &sel, now, (len > 0).then_some(len));
    (chosen, len)
}

#[test]
fn refusals() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    let refused = apply(
        &mut t,
        &DESC,
        &[
            entry(1, periodic(1000)),
            entry(9, periodic(1000)),
            entry(2, periodic(1000)),
            entry(1, periodic(5000)),
            entry(3, ObserveMode::Unknown(12)),
        ],
        false,
        None,
    )
    .unwrap();
    let reasons: Vec<_> = refused.iter().map(|r| (r.index.0, r.reason)).collect();
    assert_eq!(
        reasons,
        [
            (9, ObserveRefusal::NoSuchIndex),
            (2, ObserveRefusal::NotObservable),
            (1, ObserveRefusal::Duplicate),
            (3, ObserveRefusal::ModeNotSupported),
        ]
    );
    // the first entry of index 1 is applied
    assert_eq!(granted(&t, &DESC), [entry(1, periodic(1000))]);
    assert_eq!(t.budget(), 1_000);
}

#[test]
fn too_many_refused_changes_nothing() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(&mut t, &DESC, &[entry(4, on_change(500))], false, None).unwrap();
    let bad: Vec<_> = (10..15).map(|i| entry(i, periodic(1000))).collect();
    let mut entries = vec![entry(1, periodic(1000))];
    entries.extend(bad);
    assert_eq!(
        apply(&mut t, &DESC, &entries, false, None),
        Err(ObserveError::TooManyRefused)
    );
    assert_eq!(granted(&t, &DESC), [entry(4, on_change(500))]);
}

#[test]
fn clamps() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC,
        &[
            entry(1, periodic(0)),
            entry(3, on_change(5)),
            entry(
                4,
                ObserveMode::OnChangeOrBeat {
                    min_us: UNib32(1000),
                    max_us: UNib32(10),
                },
            ),
        ],
        false,
        Some(1_000_000),
    )
    .unwrap();
    assert_eq!(
        granted(&t, &DESC),
        [
            entry(1, periodic(100)),
            entry(3, on_change(100)),
            entry(
                4,
                ObserveMode::OnChangeOrBeat {
                    min_us: UNib32(1000),
                    max_us: UNib32(1000),
                }
            ),
        ]
    );
    assert_eq!(t.budget(), 10_000);
    // the other levels hold 12 kB/s of 15
    let req = ObserveRequest {
        entries: RefVec::Slice { slice: &[] },
        patch: true,
        budget: Some(UNib32(10_000)),
    };
    t.apply(&req, &DESC, &LIMITS, 12_000, &mut []).unwrap();
    assert_eq!(t.budget(), 3_000);
}

#[test]
fn patch_and_off() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(&mut t, &DESC, &[entry(1, periodic(1000))], false, None).unwrap();
    apply(&mut t, &DESC, &[entry(3, on_change(1000))], true, None).unwrap();
    assert_eq!(
        granted(&t, &DESC),
        [entry(1, periodic(1000)), entry(3, on_change(1000))]
    );
    apply(&mut t, &DESC, &[entry(3, on_change(1000))], false, None).unwrap();
    assert_eq!(granted(&t, &DESC), [entry(3, on_change(1000))]);
    apply(&mut t, &DESC, &[], false, None).unwrap();
    assert!(granted(&t, &DESC).is_empty());
    assert!(!t.is_active());
    assert_eq!(t.budget(), 0);
    // nothing is sent once off
    t.mark_changed(1);
    assert_eq!(poll(&mut t, &DESC, 1_000_000, |_| 4), (vec![], 0));
}

#[test]
fn periodic_rate_and_initial_values() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC,
        &[entry(1, periodic(10_000)), entry(3, on_change(1000))],
        false,
        Some(10_000),
    )
    .unwrap();
    // both right away, the on change one as its initial value
    assert_eq!(poll(&mut t, &DESC, 0, |_| 4).0, [0, 1]);
    let mut sent_at = vec![];
    let mut now = 0;
    while now <= 1_000_000 {
        now += 300;
        if poll(&mut t, &DESC, now, |_| 4).0.contains(&0) {
            sent_at.push(now);
        }
    }
    // on the grid, never early, held within a poll interval
    assert_eq!(sent_at.len(), 100);
    for (i, at) in sent_at.iter().enumerate() {
        let grid = (i as u64 + 1) * 10_000;
        assert!(*at >= grid && *at < grid + 300, "{i}: {at}");
    }
    // the on change one only when changed, and not more often than every 1 ms
    t.mark_changed(1);
    assert_eq!(poll(&mut t, &DESC, now + 1, |_| 4).0, [1]);
    t.mark_changed(1);
    assert_eq!(poll(&mut t, &DESC, now + 500, |_| 4).0, Vec::<usize>::new());
    assert_eq!(poll(&mut t, &DESC, now + 1001, |_| 4).0, [1]);
}

#[test]
fn budget_never_exceeded() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC,
        &[
            entry(1, periodic(100)),
            entry(3, periodic(100)),
            entry(4, periodic(100)),
        ],
        false,
        Some(2_000),
    )
    .unwrap();
    let mut bytes = 0;
    let mut now = 0;
    let mut window = std::collections::VecDeque::new();
    while now < 2_000_000 {
        now += 100;
        let (_, len) = poll(&mut t, &DESC, now, |k| 3 + k * 5);
        bytes += len;
        window.push_back((now, len));
        while window.front().is_some_and(|(at, _)| *at + 100_000 <= now) {
            window.pop_front();
        }
        // any 100 ms: the bucket's capacity plus what flowed in
        let in_window: usize = window.iter().map(|(_, l)| l).sum();
        assert!(in_window <= 200 + 200, "{now}: {in_window}");
    }
    // 2 s at 2 kB/s, plus the initial bucket
    assert!(bytes <= 4_000 + 200, "{bytes}");
    assert!(bytes >= 3_800, "{bytes}");
}

#[test]
fn earliest_deadline_first_and_missed() {
    let mut due = [Due::EMPTY; 8];
    let desc = LevelDesc {
        item_count: 2,
        observable: &[0, 1],
        batched: &[false; 2],
    };
    let mut t = ObserveTable::<[Slot; 2]>::new();
    // 64 B bucket, refilled at 100 B/s: one 40 byte value per poll at most
    apply(
        &mut t,
        &desc,
        &[entry(0, periodic(100_000)), entry(1, periodic(300_000))],
        false,
        Some(100),
    )
    .unwrap();
    // both due at 0: slot 0 first (tie, slot order) from the full bucket, slot 1 slips; only late, not missed yet
    assert_eq!(poll(&mut t, &desc, 0, |_| 36), (vec![0], 40));
    let plan = t.plan(0, &LIMITS, &mut due);
    assert_eq!(plan.missed(), None);
    // 400 ms later the bucket is full again; slot 0 fell 3 whole periods behind (100, 200, 300 ms: 3 missed) and
    // is due at 400 ms, slot 1's occurrence at 0 is lost (1 missed) and the one at 300 ms is due first
    // counted when the update is planned, they go with it
    assert_eq!(t.plan(400_000, &LIMITS, &mut due).missed(), Some(UNib32(4)));
    let (chosen, _) = poll(&mut t, &desc, 400_000, |_| 36);
    assert_eq!(chosen, [1]);
    // slot 0, late by less than a period, isn't missed
    let plan = t.plan(400_000, &LIMITS, &mut due);
    assert_eq!(plan.missed(), None);
    assert_eq!(plan.edf().collect::<Vec<_>>(), [0]);
}

/// A level short of budget, polled often: it holds until everything due fits, so each update carries all of it,
/// and nothing counts as missed while it keeps up on average.
#[test]
fn short_budget_sends_full_updates() {
    let mut due = [Due::EMPTY; 8];
    let desc = LevelDesc {
        item_count: 4,
        observable: &[0, 1, 2, 3],
        batched: &[false; 4],
    };
    let mut t = ObserveTable::<[Slot; 4]>::new();
    // 4 values of 4 B every ms: 20 B per update, 20 kB/s; 10 kB/s granted
    let entries: Vec<_> = (0..4).map(|i| entry(i, periodic(1_000))).collect();
    apply(&mut t, &desc, &entries, false, Some(10_000)).unwrap();
    let mut updates = vec![];
    let mut missed = 0;
    let mut now = 0;
    while now < 1_000_000 {
        now += 40;
        // what the update of this poll carries
        let carried = t.plan(now, &LIMITS, &mut due).missed().map_or(0, |m| m.0);
        let (chosen, len) = poll(&mut t, &desc, now, |_| 4);
        if len > 0 {
            updates.push(chosen.len());
            missed += carried;
        }
    }
    // after the initial burst every update has all four
    assert!(updates[10..].iter().all(|n| *n == 4), "{updates:?}");
    // 10 kB/s / 20 B: 500 updates (and the bucket's capacity at the start)
    assert!((495..=560).contains(&updates.len()), "{}", updates.len());
    // every other period can't be sent: one missed per property and whole period behind
    assert!((1_800..=2_100).contains(&missed), "{missed}");
}

/// On change: held back for less than `min` is late, not missed; for `min` or longer one missed send.
#[test]
fn hold_is_not_a_miss() {
    let mut due = [Due::EMPTY; 8];
    let desc = LevelDesc {
        item_count: 2,
        observable: &[0, 1],
        batched: &[false; 2],
    };
    let mut t = ObserveTable::<[Slot; 2]>::new();
    // 64 B bucket at 100 B/s, 40 B values: one per 400 ms
    apply(
        &mut t,
        &desc,
        &[entry(0, on_change(100_000)), entry(1, on_change(100_000))],
        false,
        Some(100),
    )
    .unwrap();
    // initial values: slot 0 from the full bucket, slot 1 left out
    assert_eq!(poll(&mut t, &desc, 0, |_| 36).0, [0]);
    assert_eq!(poll(&mut t, &desc, 50_000, |_| 36).0, Vec::<usize>::new());
    assert_eq!(t.plan(50_000, &LIMITS, &mut due).missed(), None);
    // held 100 ms: one missed, only once
    assert_eq!(poll(&mut t, &desc, 100_000, |_| 36).0, Vec::<usize>::new());
    assert_eq!(poll(&mut t, &desc, 200_000, |_| 36).0, Vec::<usize>::new());
    assert_eq!(t.plan(200_000, &LIMITS, &mut due).missed(), Some(UNib32(1)));
}

#[test]
fn t_is_a_wrapping_u32() {
    let mut due = [Due::EMPTY; 8];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(&mut t, &DESC, &[entry(1, periodic(1_000))], false, None).unwrap();
    let limits = ObserveLimits {
        timestamps: true,
        ..LIMITS
    };
    let plan = t.plan((1 << 32) + 5, &limits, &mut due);
    assert_eq!(
        plan.t(),
        Some(ww_si::Second::from_parts(
            ww_si::Prefix::Micro,
            ww_si::ww_numeric::NumericValue::U32(5)
        ))
    );
    // prefix and number type in one byte, then the u32: 4 bytes less than a u64
    let mut buf = [0u8; 32];
    let t32 = event_overhead(&mut buf, &[UNib32(1)], plan.t(), None).unwrap();
    let t64 = ww_si::Second::from_parts(
        ww_si::Prefix::Micro,
        ww_si::ww_numeric::NumericValue::U64(5),
    );
    let t64 = event_overhead(&mut buf, &[UNib32(1)], Some(t64), None).unwrap();
    let without = event_overhead(&mut buf, &[UNib32(1)], None, None).unwrap();
    assert_eq!(t64 - t32, 4);
    assert!(t32 - without <= 6, "{t32} {without}");
}

#[test]
fn too_big_goes_alone_when_full_or_never() {
    let mut due = [Due::EMPTY; 8];
    let mut marks = [Mark::NONE; 8];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC,
        &[entry(1, periodic(1000)), entry(3, periodic(1000))],
        false,
        Some(100),
    )
    .unwrap();
    // slot 0 is bigger than the 64 B bucket: alone, from a full bucket
    assert_eq!(poll(&mut t, &DESC, 0, |k| [100, 2][k]).0, [0]);
    // slot 1 is bigger than any message: dropped, as missed, every time it is due
    let plan = t.plan(2_000_000, &LIMITS, &mut due);
    let mut sel = plan.selection(&DESC, 4, 64, &mut marks);
    assert!(sel.offer(1, Some(1000)));
    assert!(!sel.is_chosen(1));
}

#[test]
fn measure_bounds() {
    let mut buf = [0u8; 32];
    assert_eq!(max_write_len::<u32>(), Some(4));
    assert_eq!(measure(&7u8, &mut buf), Some(1));
    let s: &str = "hello";
    // the string's own length nibble, and room for its size in the update
    assert_eq!(measure(&s, &mut buf), Some(6 + LEN_SLOT));
    assert!(event_overhead(&mut buf, &[UNib32(1)], None, None).unwrap() <= 4);
}

#[test]
fn overload_rotates() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    // the same period for all, room for about one value per period: earliest deadline first takes turns
    apply(
        &mut t,
        &DESC,
        &[
            entry(1, periodic(10_000)),
            entry(3, periodic(10_000)),
            entry(4, periodic(10_000)),
        ],
        false,
        Some(1_500),
    )
    .unwrap();
    let mut sent = vec![];
    let mut now = 0;
    while now < 2_000_000 {
        now += 1_000;
        let (chosen, _) = poll(&mut t, &DESC, now, |_| 10);
        sent.extend(chosen);
    }
    let counts: Vec<usize> = (0..3)
        .map(|k| sent.iter().filter(|s| **s == k).count())
        .collect();
    let (min, max) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
    assert!(max - min <= 2, "{counts:?}");
    assert!(*min > 50, "{counts:?}");
}

#[test]
fn malformed_entries_change_nothing() {
    use wire_weaver::shrink_wrap::BufReader;
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(&mut t, &DESC, &[entry(4, on_change(500))], false, None).unwrap();
    // says 1000 entries, has none
    let req = ObserveRequest {
        entries: RefVec::Buf {
            buf: BufReader::new(&[]),
            elements_count: 1000,
        },
        patch: false,
        budget: None,
    };
    assert!(matches!(
        t.apply(&req, &DESC, &LIMITS, 0, &mut []),
        Err(ObserveError::Malformed(_))
    ));
    assert_eq!(granted(&t, &DESC), [entry(4, on_change(500))]);
}

fn samples(every_us: u32, max_age_us: u32) -> ObserveMode {
    ObserveMode::Samples {
        every_us: UNib32(every_us),
        max_age_us: UNib32(max_age_us),
    }
}

/// `Samples` only for `+batched` properties (untrusted input: refused, nothing applied for it), its max age
/// clamped to the limits both ways, the grid raised to the device's floor (`samples_min_every_us`).
#[test]
fn samples_mode_validated_and_clamped() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    let refused = apply(
        &mut t,
        &DESC_B,
        &[
            entry(1, samples(0, 20_000)),
            entry(3, samples(0, 10)),
            entry(4, samples(u32::MAX, u32::MAX)),
        ],
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        refused,
        [
            ObserveRefused {
                index: UNib32(1),
                reason: ObserveRefusal::ModeNotSupported
            },
            ObserveRefused {
                index: UNib32(4),
                reason: ObserveRefusal::ModeNotSupported
            }
        ]
    );
    // every sample asked for, one per 10 µs at most granted
    assert_eq!(granted(&t, &DESC_B), [entry(3, samples(10, 1_000))]);
    assert_eq!(t.samples(1), Some((10, 1_000)));
    apply(
        &mut t,
        &DESC_B,
        &[entry(3, samples(25, 1_000))],
        false,
        None,
    )
    .unwrap();
    assert_eq!(t.samples(1), Some((25, 1_000)));
    apply(
        &mut t,
        &DESC_B,
        &[entry(3, samples(u32::MAX, u32::MAX))],
        false,
        None,
    )
    .unwrap();
    assert_eq!(t.samples(1), Some((u32::MAX, 1_000_000)));
    // the same mode on a level without batched properties
    let mut t = ObserveTable::<[Slot; 3]>::new();
    let refused = apply(&mut t, &DESC, &[entry(3, samples(0, 20_000))], false, None).unwrap();
    assert_eq!(refused[0].reason, ObserveRefusal::ModeNotSupported);
    assert!(!t.is_active());
}

/// A slot in `Samples` mode is due while a batch of it waits, and keeps its place when left out; what the outbox
/// dropped goes with the next update as missed, slipping alone counts nothing.
#[test]
fn samples_due_while_a_batch_waits() {
    let mut due = [Due::EMPTY; 8];
    let mut marks = [Mark::NONE; 8];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC_B,
        &[entry(3, samples(0, 5_000)), entry(4, periodic(1_000))],
        false,
        Some(10_000),
    )
    .unwrap();
    // nothing waiting: only the periodic one (sent at start)
    let plan = t.plan_with(0, &LIMITS, &|_| false, &mut due);
    assert_eq!(plan.edf().collect::<Vec<_>>(), [2]);
    let mut sel = plan.selection(&DESC_B, 4, 1024, &mut marks);
    assert!(sel.offer(2, Some(4)));
    t.commit(&plan, &sel, 0, Some(12));
    t.add_missed(5);
    // a batch waits: due now; the outbox's drops go with this update
    let plan = t.plan_with(100, &LIMITS, &|k| k == 1, &mut due);
    assert_eq!(plan.edf().collect::<Vec<_>>(), [1]);
    assert_eq!(plan.missed(), Some(UNib32(5)));
    // left out (nothing sent): it keeps waiting from 100 µs and goes before the periodic one due at 1 ms
    let sel = plan.selection(&DESC_B, 4, 1024, &mut marks);
    t.commit(&plan, &sel, 100, None);
    let plan = t.plan_with(1_500, &LIMITS, &|k| k == 1, &mut due);
    assert_eq!(plan.edf().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(plan.missed(), Some(UNib32(5)), "not counted again");
    let mut sel = plan.selection(&DESC_B, 4, 1024, &mut marks);
    assert!(sel.offer(1, Some(40)) && sel.offer(2, Some(4)));
    t.commit(&plan, &sel, 1_500, Some(52));
    assert_eq!(
        t.plan_with(1_600, &LIMITS, &|_| false, &mut due).missed(),
        None
    );
}

/// Samples lost with nothing left to send (the last of a burst): the count goes in an update of its own, once the
/// budget has room, and only once.
#[test]
fn missed_goes_alone_when_nothing_is_due() {
    let mut due = [Due::EMPTY; 8];
    let mut marks = [Mark::NONE; 8];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC_B,
        &[entry(3, samples(0, 5_000))],
        false,
        Some(10_000),
    )
    .unwrap();
    let plan = t.plan_with(0, &LIMITS, &|_| false, &mut due);
    assert!(plan.is_empty());
    assert!(!plan.report_missed(&DESC_B, 4), "nothing missed");
    t.add_missed(32);
    let plan = t.plan_with(100, &LIMITS, &|_| false, &mut due);
    assert!(plan.is_empty());
    assert!(plan.report_missed(&DESC_B, 4));
    assert_eq!(plan.missed(), Some(UNib32(32)));
    // more than the bucket holds: waits
    assert!(!plan.report_missed(&DESC_B, 100_000));
    let sel = plan.selection(&DESC_B, 4, 1024, &mut marks);
    t.commit(&plan, &sel, 100, Some(8));
    let plan = t.plan_with(200, &LIMITS, &|_| false, &mut due);
    assert_eq!(plan.missed(), None);
    assert!(!plan.report_missed(&DESC_B, 4));
}

#[test]
fn gate_takes_all_none_or_a_grid() {
    let gate = SampleGate::new();
    let mut cursor = GateCursor::new();
    let t = |us: u32| ww_si::Second::micro(us);
    assert!(!cursor.take(&gate, &t(0)));
    gate.set(Some(0));
    assert!((0..10).all(|i| cursor.take(&gate, &t(i * 40))));
    // one per 100 µs from 40 µs samples: 0, 120, 200, 320, 400, ..: the first one at or after each grid point
    gate.set(Some(100));
    let taken: Vec<u32> = (0..12)
        .map(|i| i * 40)
        .filter(|us| cursor.take(&gate, &t(*us)))
        .collect();
    assert_eq!(taken, [0, 120, 200, 320, 400]);
    // across the wrap of the u32 µs clock
    let mut cursor = GateCursor::new();
    let start = u32::MAX - 150;
    let taken: Vec<u32> = (0..8)
        .map(|i| start.wrapping_add(i * 40))
        .filter(|us| cursor.take(&gate, &t(*us)))
        .collect();
    assert_eq!(
        taken,
        [start, start.wrapping_add(120), start.wrapping_add(200)]
    );
    // a sampler that fell far behind starts its grid over at the next sample
    assert!(cursor.take(&gate, &t(1_000_000)));
    assert!(!cursor.take(&gate, &t(1_000_050)));
    gate.set(None);
    assert!(!gate.is_open());
    assert!(!cursor.take(&gate, &t(2_000_000)));
}

#[test]
fn batch_budget_leaves_room_for_the_slots() {
    let budget = batch_budget(&DESC_B, 10, 256, 4096, 1);
    assert!(budget < 256 && budget > 200, "{budget}");
    assert_eq!(batch_budget(&DESC_B, 10, 256, 62, 1), 50);
    // a batch of the budget, in its slots (the time column and one value column), fits the update's estimate of
    // the buffer, however its bytes are split between them
    assert!(
        10 + EVENT_SLACK + DESC_B.header_bound() + batch_slot_len(budget) + batch_slot_len(0)
            <= 256 + 10
    );
    // more columns, more slots
    assert!(batch_budget(&DESC_BB, 10, 256, 4096, 2) < batch_budget(&DESC_BB, 10, 256, 4096, 1));
}

/// One row per sample of a level and one time column per update (API-39, shared clock): the level's `Samples`
/// slots share the finest `every_us` and the shortest `max_age_us` asked, and the grant says so.
#[test]
fn samples_settings_are_shared_by_the_level() {
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC_BB,
        &[
            entry(1, samples(100, 5_000)),
            entry(3, samples(0, 2_000)),
            entry(
                4,
                ObserveMode::Periodic {
                    every_us: UNib32(1_000),
                },
            ),
        ],
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        granted(&t, &DESC_BB),
        [
            entry(1, samples(10, 2_000)),
            entry(3, samples(10, 2_000)),
            entry(
                4,
                ObserveMode::Periodic {
                    every_us: UNib32(1_000)
                }
            ),
        ]
    );
    // `every_us` 0 raised to the limits' floor of 10
    assert_eq!(t.level_samples(), Some((10, 2_000)));
    assert_eq!(t.samples(2), None);
    // one property alone keeps what it asked for
    apply(
        &mut t,
        &DESC_BB,
        &[entry(1, samples(100, 5_000))],
        false,
        None,
    )
    .unwrap();
    assert_eq!(granted(&t, &DESC_BB), [entry(1, samples(100, 5_000))]);
    apply(&mut t, &DESC_BB, &[], false, None).unwrap();
    assert_eq!(t.level_samples(), None);
}

/// A batch of samples goes into an update whole, with the level's `sample_times` slot, or not at all.
#[test]
fn batch_of_samples_is_offered_whole() {
    let mut due = [Due::EMPTY; 8];
    let mut marks = [Mark::NONE; 8];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC_BB,
        &[entry(1, samples(0, 2_000)), entry(3, samples(0, 2_000))],
        false,
        None,
    )
    .unwrap();
    let plan = t.plan_with(0, &LIMITS, &|k| k < 2, &mut due);
    assert_eq!(plan.edf().count(), 2);
    let mut sel = plan.selection(&DESC_BB, 4, 1024, &mut marks);
    assert!(sel.offer_batch(&[0, 1], 20));
    assert!(sel.is_chosen(0) && sel.is_chosen(1) && sel.has_time());
    assert_eq!(sel.present(&DESC_BB).collect::<Vec<_>>(), [1, 3]);
    // bigger than a message: neither column, no time slot
    let mut sel = plan.selection(&DESC_BB, 4, 64, &mut marks);
    sel.offer_batch(&[0, 1], 100);
    assert!(!sel.is_chosen(0) && !sel.is_chosen(1) && !sel.has_time());
    assert_eq!(sel.present(&DESC_BB).count(), 0);
}

/// The shared loop of a level (SRV-24): `select` chooses within the budget, `write` asks for the chosen values only
/// and in slot order, `ser_update` makes the event a host reads back.
#[test]
fn shared_loop_writes_the_chosen_values() {
    use wire_weaver::shrink_wrap::DeserializeShrinkWrap;
    use ww_client_server::{Event, EventKind};
    let mut due = [Due::EMPTY; 3];
    let mut marks = [Mark::NONE; 3];
    let mut t = ObserveTable::<[Slot; 3]>::new();
    apply(
        &mut t,
        &DESC,
        &[entry(1, periodic(1_000)), entry(4, periodic(1_000))],
        false,
        Some(10_000),
    )
    .unwrap();
    let path = [UNib32(2)];
    let mut buf = [0u8; 128];
    let plan = t.plan(0, &LIMITS, &mut due);
    assert!(plan.is_due(0) && !plan.is_due(1) && plan.is_due(2));
    let mut measured = vec![];
    let round = select(
        &plan,
        &DESC,
        &path,
        &mut buf,
        &mut marks,
        None,
        &mut |k, buf| {
            measured.push(k);
            measure(&(k as u32 * 100), buf)
        },
    )
    .unwrap()
    .expect("both due");
    assert_eq!(measured, [0, 2]);
    assert!(round.needs_value(0) && !round.needs_value(1) && round.needs_value(2));
    let mut written = vec![];
    let update = write(&round, &plan, &DESC, &mut buf, &mut |k, column, wr| {
        assert!(!column);
        written.push(k);
        wr.write(DESC.observable[k], &(k as u32 * 100)).is_ok()
    })
    .expect("values written");
    assert_eq!(written, [0, 2]);
    let data = buf[..update.data_len()].to_vec();
    let mut scratch = [0u8; 128];
    let len = ser_update(&mut scratch, &path, &update, &data, None).unwrap();
    let Ok(EventKind::PropertyUpdate {
        t: None,
        missed: None,
        data,
        ..
    }) = Event::from_ww_bytes(&scratch[..len]).unwrap().result
    else {
        panic!("not an update");
    };
    let values = wire_weaver::shrink_wrap::option_any_vec::OptionAnyVec::new(data.0).unwrap();
    let mut it = values.iter();
    assert_eq!(it.next_present(), Some(1));
    assert_eq!(it.next::<u32>().unwrap(), Some(0));
    assert_eq!(it.next_present(), Some(4));
    assert_eq!(it.next::<u32>().unwrap(), Some(200));
    t.commit(&plan, round.selection(), 0, Some(len));
    // sent: nothing due before the next period
    assert!(t.plan(500, &LIMITS, &mut due).is_empty());

    // a value that can't be written: nothing goes out, the chosen ones count as missed
    let plan = t.plan(1_000, &LIMITS, &mut due);
    let round = select(
        &plan,
        &DESC,
        &path,
        &mut buf,
        &mut marks,
        None,
        &mut |_, _| Some(8),
    )
    .unwrap()
    .unwrap();
    assert!(write(&round, &plan, &DESC, &mut buf, &mut |_, _, _| false).is_none());
    t.commit(&plan, round.selection(), 1_000, None);
    assert_eq!(t.plan(1_100, &LIMITS, &mut due).missed(), Some(UNib32(2)));
}
