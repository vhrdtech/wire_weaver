use super::*;
use wire_weaver::shrink_wrap::RefVec;

const DESC: LevelDesc<3> = LevelDesc {
    item_count: 6,
    observable: [1, 3, 4],
};

const LIMITS: ObserveLimits = ObserveLimits {
    min_period_us: 100,
    default_budget: 1_000,
    max_level_budget: 10_000,
    total_budget: 15_000,
    timestamps: false,
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

fn apply<const N: usize>(
    table: &mut ObserveTable<N>,
    desc: &LevelDesc<N>,
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

fn granted<const N: usize>(table: &ObserveTable<N>, desc: &LevelDesc<N>) -> Vec<ObserveEntry> {
    let mut out = [entry(0, ObserveMode::Off); N];
    let len = table.granted(desc, &mut out);
    out[..len].to_vec()
}

/// One poll as the generated code does it, with value sizes `size(k)`, events of 4 bytes plus data: returns the
/// chosen slots and the event length (0 when nothing was sent).
fn poll<const N: usize>(
    table: &mut ObserveTable<N>,
    desc: &LevelDesc<N>,
    now: u64,
    size: impl Fn(usize) -> usize,
) -> (Vec<usize>, usize) {
    let plan = table.plan(now, &LIMITS);
    if plan.is_empty() {
        return (vec![], 0);
    }
    let mut sel = plan.selection(desc, 4, 1024);
    for &k in plan.edf() {
        if !sel.offer(k, Some(size(k))) {
            break;
        }
    }
    let chosen: Vec<usize> = (0..N).filter(|k| sel.is_chosen(*k)).collect();
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
    let desc = LevelDesc::<2> {
        item_count: 2,
        observable: [0, 1],
    };
    let mut t = ObserveTable::<2>::new();
    // 64 B bucket, refilled at 100 B/s: one 40 byte value per poll at most
    apply(
        &mut t,
        &desc,
        &[entry(0, periodic(100_000)), entry(1, periodic(300_000))],
        false,
        Some(100),
    )
    .unwrap();
    // both due at 0: slot 0 first (tie, slot order), slot 1 slips and counts one missed
    assert_eq!(poll(&mut t, &desc, 0, |_| 36), (vec![0], 40));
    let plan = t.plan(0, &LIMITS);
    assert_eq!(plan.missed(), Some(UNib32(1)));
    // 400 ms later the bucket is full again; slot 0 fell 3 whole periods behind (100, 200, 300 ms: 3 missed) and
    // is due at 400 ms, slot 1's occurrence at 0 is lost (counted at its slip) and the one at 300 ms is due first
    let (chosen, _) = poll(&mut t, &desc, 400_000, |_| 36);
    assert_eq!(chosen, [1]);
    let plan = t.plan(400_000, &LIMITS);
    // the update just sent carried the first 1, these are new: slot 0's 3 periods and its slip now
    assert_eq!(plan.missed(), Some(UNib32(4)));
    assert_eq!(plan.edf(), [0]);
}

#[test]
fn too_big_goes_alone_when_full_or_never() {
    let mut t = ObserveTable::<3>::new();
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
    let plan = t.plan(2_000_000, &LIMITS);
    let mut sel = plan.selection(&DESC, 4, 64);
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
    let mut t = ObserveTable::<3>::new();
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
    let mut t = ObserveTable::<3>::new();
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
