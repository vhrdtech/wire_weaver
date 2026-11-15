use std::collections::BTreeMap;

use super::*;
use crate::{Block, BlockError, Ratio64, SampleShape, SeriesSegment};

/// xorshift64*: workloads without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// An event workload: starts, ends, flags.
type Items = Vec<(i64, i64, u32)>;

/// A named event workload and the fields its tree keeps.
type Workload = (&'static str, fn(usize, &mut Rng) -> Items, EventFields);

fn bursty(n: usize, rng: &mut Rng) -> Items {
    let mut t = 0i64;
    (0..n)
        .map(|_| {
            t += if rng.below(16) == 0 {
                rng.below(1_000_000) as i64
            } else {
                rng.below(200) as i64
            };
            (t, t, 0)
        })
        .collect()
}

fn periodic_jitter(n: usize, rng: &mut Rng) -> Items {
    (0..n as i64)
        .map(|i| {
            let t = i * 1_000_000 + rng.below(600) as i64;
            (
                t,
                t,
                if rng.below(1000) == 0 {
                    1 << rng.below(32)
                } else {
                    0
                },
            )
        })
        .collect()
}

fn exact_gaps(n: usize, rng: &mut Rng) -> Items {
    let mut t = 0i64;
    (0..n)
        .map(|_| {
            t += if rng.below(5000) == 0 {
                7_000_000_000
            } else {
                1_000_000
            };
            (t, t, 0)
        })
        .collect()
}

fn sparse(n: usize, rng: &mut Rng) -> Items {
    let mut t = -50_000_000_000i64;
    (0..n)
        .map(|_| {
            t += rng.below(40_000_000_000) as i64;
            (t, t, rng.below(4) as u32)
        })
        .collect()
}

/// Spans of mixed lengths: some long ones reach over many later items.
fn spans(n: usize, rng: &mut Rng) -> Items {
    let mut t = 0i64;
    (0..n)
        .map(|_| {
            t += rng.below(1000) as i64;
            let d = if rng.below(500) == 0 {
                rng.below(100_000_000) as i64
            } else {
                rng.below(800) as i64
            };
            (t, t + d, rng.below(2) as u32 * 4)
        })
        .collect()
}

/// Every node of every level read back from `blocks`: (level, node) to (first item, items, node).
fn event_levels(blocks: &[Vec<u8>]) -> BTreeMap<(u8, u64), (u64, u64, EventNode)> {
    let mut m = BTreeMap::new();
    for b in blocks {
        let block = Block::read(b).expect("a good block");
        assert_eq!(block.kind, crate::BlockKind::Summary);
        let s = block.summary().expect("a summary");
        let h = s.head();
        for i in 0..s.node_count() {
            let (first, items) = h.node_items(i as u64);
            let old = m.insert(
                (h.level, h.first_node() + i as u64),
                (first, items, s.event_node(i).unwrap()),
            );
            assert!(old.is_none(), "node written twice");
        }
    }
    m
}

/// The node of `items[from..to]` computed directly.
fn naive_event(items: &[(i64, i64, u32)], fields: EventFields) -> EventNode {
    EventNode {
        first_tick: items[0].0,
        last_tick: items[items.len() - 1].0,
        flags: if fields.flags {
            items.iter().fold(0, |f, x| f | x.2)
        } else {
            0
        },
        latest_end: items.iter().map(|x| x.1).max().unwrap(),
    }
}

/// Build the tree of `items` (item numbers from `origin`), check every level against the naive model and that each
/// parent is its children merged; returns the blocks.
fn check_events(items: &Items, origin: u64, fields: EventFields, runs: bool) -> Vec<Vec<u8>> {
    let mut tree = EventTree::new(0x1234, 3, fields, origin);
    if runs {
        // the same items as runs that stop at level-0 node edges and every 128 items
        let mut i = 0usize;
        while i < items.len() {
            let item = origin + i as u64;
            let to_node = 2048 - (item % 2048);
            let n = (to_node.min(128 - item % 128) as usize).min(items.len() - i);
            let run = &items[i..i + n];
            tree.push_run(ItemRun {
                count: n as u64,
                first_tick: run[0].0,
                last_tick: run[n - 1].0,
                flags: run.iter().fold(0, |f, x| f | x.2),
                latest_end: run.iter().map(|x| x.1).max().unwrap(),
            })
            .unwrap();
            i += n;
        }
    } else {
        for &(s, e, f) in items {
            tree.push(s, e, f).unwrap();
        }
    }
    let mut blocks = tree.take_blocks();
    tree.finish().unwrap();
    blocks.extend(tree.take_blocks());
    let got = event_levels(&blocks);
    let end = origin + items.len() as u64;
    let top = got.keys().map(|k| k.0).max().unwrap();
    for level in 0..=top {
        let shift = EVENT_BASE_SHIFT + FANOUT_SHIFT * level;
        let first = origin >> shift;
        let last = (end - 1) >> shift;
        let nodes = last - first + 1;
        assert_eq!(
            nodes == 1,
            level == top,
            "the top level, only it, has a single node"
        );
        for n in first..=last {
            let (fi, count, node) = got[&(level, n)];
            let from = (n << shift).max(origin);
            let to = ((n + 1) << shift).min(end);
            assert_eq!(
                (fi, count),
                (from, to - from),
                "level {level} node {n} items"
            );
            let slice = &items[(from - origin) as usize..(to - origin) as usize];
            assert_eq!(node, naive_event(slice, fields), "level {level} node {n}");
            if level > 0 {
                // a reader merging the children in order gets the parent
                let mut kids = (n << FANOUT_SHIFT..(n + 1) << FANOUT_SHIFT)
                    .filter_map(|c| got.get(&(level - 1, c)));
                let mut m = kids.next().unwrap().2;
                for k in kids {
                    m.merge(&k.2);
                }
                assert_eq!(m, node);
            }
        }
    }
    assert_eq!(got.keys().filter(|k| k.0 > top).count(), 0);
    blocks
}

#[test]
fn event_tree_matches_naive_model() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let instants = EventFields {
        flags: true,
        latest_end: false,
    };
    let span_fields = EventFields {
        flags: true,
        latest_end: true,
    };
    let workloads: [Workload; 5] = [
        ("bursty", bursty, EventFields::default()),
        ("periodic-jitter", periodic_jitter, instants),
        ("exact-gaps", exact_gaps, instants),
        ("sparse", sparse, instants),
        ("spans", spans, span_fields),
    ];
    for (name, make, fields) in workloads {
        for (n, origin) in [
            (1, 0),
            (2047, 0),
            (2048, 0),
            (2049, 0),
            (40_000, 0),
            (40_000, 2048 * 15 + 100),
            (600_000, 0),
            (600_000, 524_288 - 3),
        ] {
            let items = make(n, &mut rng);
            let a = check_events(&items, origin, fields, false);
            let b = check_events(&items, origin, fields, true);
            assert_eq!(a, b, "{name}: runs and single items give the same bytes");
        }
    }
}

#[test]
fn event_tree_refuses_bad_input() {
    let f = EventFields {
        flags: false,
        latest_end: false,
    };
    let mut t = EventTree::new(1, 1, f, 0);
    t.push(10, 10, 0).unwrap();
    assert!(t.push(9, 9, 0).is_err(), "out of order");
    assert!(t.push(11, 12, 0).is_err(), "a span in a tree of instants");
    let run = |count| ItemRun {
        count,
        first_tick: 20,
        last_tick: 30,
        flags: 0,
        latest_end: 30,
    };
    assert!(t.push_run(run(0)).is_err());
    assert!(t.push_run(run(2048)).is_err(), "across a level-0 node");
    t.push_run(run(2047)).unwrap();
    assert!(
        t.take_blocks().is_empty(),
        "node 0 closed, its batch not full yet"
    );
}

/// Overview of 1 G items: the level of 32768 items is about 30 000 nodes, about 600 KB, enough for 2000 pixels.
#[test]
fn overview_of_a_billion_items_reads_a_level() {
    let n: u64 = 1_000_000_000;
    let fields = EventFields {
        flags: true,
        latest_end: false,
    };
    let mut tree = EventTree::new(9, 1, fields, 0);
    let mut item = 0u64;
    let mut bytes = BTreeMap::<u8, (usize, u64)>::new();
    let mut tally = |blocks: Vec<Vec<u8>>| {
        for b in blocks {
            let h = Summary::read(&b).unwrap().head();
            let e = bytes.entry(h.level).or_default();
            e.0 += b.len();
            e.1 += h.node_count();
        }
    };
    while item < n {
        let count = (n - item).min(2048);
        let t = (item * 1000) as i64;
        tree.push_run(ItemRun {
            count,
            first_tick: t,
            last_tick: t + (count as i64 - 1) * 1000,
            flags: 0,
            latest_end: t + (count as i64 - 1) * 1000,
        })
        .unwrap();
        item += count;
        if item.is_multiple_of(1 << 24) {
            tally(tree.take_blocks());
        }
    }
    tree.finish().unwrap();
    tally(tree.take_blocks());
    let (level1, nodes1) = bytes[&1];
    assert_eq!(nodes1, n.div_ceil(32_768));
    assert!(level1 < 650_000, "level 1 is {level1} bytes");
    assert!(nodes1 / 2000 >= 15, "about 15 nodes per pixel");
    let (level2, _) = bytes[&2];
    assert!(level2 < 40_000, "level 2 is {level2} bytes");
    assert_eq!(
        bytes.keys().max(),
        Some(&5),
        "levels 0 to 5, one node on top"
    );
    assert_eq!(bytes[&5].1, 1);
}

/// A series of interleaved frames: `channels × components` scalars per sample.
fn series_blocks<T: Scalar>(
    data: &[T],
    shape: SampleShape,
    spec: SeriesSummarySpec,
    first_item: u64,
    flags: &dyn Fn(usize) -> u32,
    pushes: usize,
) -> Vec<Vec<u8>> {
    let start = SeriesStart {
        track_id: 77,
        track_number: 2,
        shape,
        first_item,
        segment: SeriesSegment::start(first_item, 1000, Magnitude::ONE),
        period: Ratio64::new(62_500, 3).unwrap(),
    };
    let mut s = SeriesSummaries::<T>::new(start, spec).unwrap();
    let stride = shape.lanes().unwrap() as usize;
    let frames = data.len() / stride;
    // pushes of uneven sizes, flags per push
    let mut at = 0usize;
    let mut k = 0usize;
    let mut blocks = vec![];
    while at < frames {
        let n = ((frames / pushes.max(1)) + k % 7).max(1).min(frames - at);
        s.push(&data[at * stride..(at + n) * stride], flags(k))
            .unwrap();
        at += n;
        k += 1;
        blocks.extend(s.take_blocks());
    }
    s.finish().unwrap();
    blocks.extend(s.take_blocks());
    blocks
}

/// Every node of every channel read back: (level, node, channel) to (first item, items, stats).
fn series_levels(blocks: &[Vec<u8>]) -> BTreeMap<(u8, u64, u16), (u64, u64, ChannelStats)> {
    let mut m = BTreeMap::new();
    for b in blocks {
        let s = Block::read(b).unwrap().summary().unwrap();
        let h = s.head();
        let Layout::Series { channels, .. } = h.layout else {
            panic!("a series block")
        };
        for i in 0..s.node_count() {
            let (first, items) = h.node_items(i as u64);
            for ch in 0..channels {
                let old = m.insert(
                    (h.level, h.first_node() + i as u64, ch),
                    (first, items, s.channel_stats(ch, i).unwrap()),
                );
                assert!(old.is_none(), "node written twice");
            }
        }
    }
    m
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a == b || (a - b).abs() <= rel * a.abs().max(b.abs())
}

fn same_bits(a: &ChannelStats, b: &ChannelStats) -> bool {
    a.min.map(Value::to_bits) == b.min.map(Value::to_bits)
        && a.max.map(Value::to_bits) == b.max.map(Value::to_bits)
        && a.sum.map(f64::to_bits) == b.sum.map(f64::to_bits)
        && a.sum_sq.map(f64::to_bits) == b.sum_sq.map(f64::to_bits)
        && a.clipped == b.clipped
        && a.flags == b.flags
        && a.power_mean.to_bits() == b.power_mean.to_bits()
        && a.power_max.to_bits() == b.power_max.to_bits()
}

/// Check every level of a series against statistics computed straight from the samples, and every parent against
/// its children merged in order.
#[expect(
    clippy::too_many_arguments,
    reason = "a test helper over every series parameter"
)]
fn check_series<T: Scalar>(
    data: &[T],
    shape: SampleShape,
    spec: SeriesSummarySpec,
    first_item: u64,
    flags: &dyn Fn(usize) -> u32,
    pushes: usize,
    clip: (T, T),
    exact_sums: bool,
) {
    let blocks = series_blocks(data, shape, spec, first_item, flags, pushes);
    let got = series_levels(&blocks);
    let element = shape.element;
    let comps = usize::from(series::components(element));
    let channels = shape.channels.0 as usize;
    let stride = channels * comps;
    let frames = data.len() / stride;
    let end = first_item + frames as u64;
    let top = got.keys().map(|k| k.0).max().unwrap();
    // flags as the builder saw them: a frame's flags are those of the push it came in
    let mut frame_flags = vec![0u32; frames];
    {
        let mut at = 0usize;
        let mut k = 0usize;
        while at < frames {
            let n = ((frames / pushes.max(1)) + k % 7).max(1).min(frames - at);
            frame_flags[at..at + n].fill(flags(k));
            at += n;
            k += 1;
        }
    }
    for level in 0..=top {
        let shift = spec.base_shift + FANOUT_SHIFT * level;
        let (first, last) = (first_item >> shift, (end - 1) >> shift);
        assert_eq!(
            first == last,
            level == top,
            "the top level, only it, has a single node"
        );
        for n in first..=last {
            let from = (n << shift).max(first_item);
            let to = ((n + 1) << shift).min(end);
            let fr = (from - first_item) as usize..(to - first_item) as usize;
            for ch in 0..channels {
                let (fi, count, s) = got[&(level, n, ch as u16)];
                assert_eq!((fi, count), (from, to - from));
                let val = |f: usize, c: usize| data[f * stride + ch * comps + c];
                for c in 0..comps {
                    let xs: Vec<T> = fr
                        .clone()
                        .map(|f| val(f, c))
                        .filter(|x| x.partial_cmp(x).is_some())
                        .collect();
                    let min = xs.iter().copied().reduce(|a, b| if b < a { b } else { a });
                    let max = xs.iter().copied().reduce(|a, b| if b > a { b } else { a });
                    if spec.stats.min {
                        match min {
                            Some(m) => assert_eq!(
                                s.min[c].to_bits(),
                                m.value().to_bits(),
                                "min L{level} n{n}"
                            ),
                            None => assert!(s.min[c].as_f64().is_nan()),
                        }
                    }
                    if spec.stats.max {
                        match max {
                            Some(m) => assert_eq!(
                                s.max[c].to_bits(),
                                m.value().to_bits(),
                                "max L{level} n{n}"
                            ),
                            None => assert!(s.max[c].as_f64().is_nan()),
                        }
                    }
                    let sum: f64 = fr.clone().map(|f| val(f, c).to_f64()).sum();
                    let sq: f64 = fr.clone().map(|f| val(f, c).to_f64().powi(2)).sum();
                    if spec.stats.sum {
                        assert!(
                            close(s.sum[c], sum, if exact_sums { 0.0 } else { 1e-9 }),
                            "sum L{level} n{n}: {} {sum}",
                            s.sum[c]
                        );
                    }
                    if spec.stats.sum_sq {
                        assert!(
                            close(s.sum_sq[c], sq, if exact_sums { 0.0 } else { 1e-9 }),
                            "sum_sq L{level} n{n}"
                        );
                    }
                }
                if spec.stats.clipped {
                    let want = fr
                        .clone()
                        .filter(|&f| {
                            (0..comps).any(|c| !(clip.0 < val(f, c) && val(f, c) < clip.1))
                        })
                        .count() as u64;
                    assert_eq!(s.clipped, want, "clipped L{level} n{n}");
                }
                if spec.stats.flags {
                    assert_eq!(
                        s.flags,
                        frame_flags[fr.clone()].iter().fold(0, |a, b| a | b)
                    );
                }
                if spec.stats.power_mean || spec.stats.power_max {
                    let p: Vec<f64> = fr
                        .clone()
                        .map(|f| val(f, 0).to_f64().powi(2) + val(f, 1).to_f64().powi(2))
                        .collect();
                    let mean = p.iter().sum::<f64>() / p.len() as f64;
                    let pmax = p.iter().copied().fold(f64::NAN, f64::max);
                    if spec.stats.power_mean {
                        assert!(
                            close(f64::from(s.power_mean), mean, 1e-6),
                            "power mean L{level} n{n}"
                        );
                    }
                    if spec.stats.power_max {
                        assert_eq!(s.power_max, pmax as f32, "power max L{level} n{n}");
                    }
                }
                if level > 0 {
                    let mut m = ChannelStats::empty(element);
                    let mut items = 0;
                    for c in n << FANOUT_SHIFT..(n + 1) << FANOUT_SHIFT {
                        if let Some(k) = got.get(&(level - 1, c, ch as u16)) {
                            m.merge(&k.2, items, k.1);
                            items += k.1;
                        }
                    }
                    assert!(
                        same_bits(&m, &s),
                        "L{level} n{n}: children merged {m:?} vs {s:?}"
                    );
                }
            }
        }
    }
}

fn spec(base_shift: u8, stats: SeriesStats, clip: Option<ClipRange>) -> SeriesSummarySpec {
    SeriesSummarySpec {
        base_shift,
        stats,
        clip,
    }
}

#[test]
fn series_summaries_match_naive_model() {
    let mut rng = Rng(42);
    let no_flags = |_k: usize| 0u32;
    let some_flags = |k: usize| {
        if k.is_multiple_of(5) {
            1 << (k % 32)
        } else {
            0
        }
    };

    // complex i16 IQ, 2 channels, full scale clipping, every statistic
    let shape = SampleShape::new(Element::ComplexI16, 2);
    let iq: Vec<i16> = (0..200_000 * 4)
        .map(|i| {
            if i % 9973 == 0 {
                i16::MAX
            } else {
                (rng.next() >> 48) as i16 / 4
            }
        })
        .collect();
    for first in [0, 1000, 65_536 * 3 - 17] {
        check_series(
            &iq,
            shape,
            spec(6, SeriesStats::all(shape.element), None),
            first,
            &some_flags,
            37,
            (i16::MIN, i16::MAX),
            true,
        );
    }

    // f32 with NaN and infinities, one channel
    let shape = SampleShape::scalar(Element::F32);
    let f: Vec<f32> = (0..300_000)
        .map(|i| match i % 10_007 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            _ => (rng.next() >> 40) as f32 / 1000.0 - 8000.0,
        })
        .collect();
    check_series(
        &f,
        shape,
        spec(10, SeriesStats::all(shape.element), None),
        0,
        &no_flags,
        3,
        (f32::NEG_INFINITY, f32::INFINITY),
        false,
    );

    // a node of only NaN
    let nan = vec![f64::NAN; 100];
    check_series(
        &nan,
        SampleShape::scalar(Element::F64),
        spec(4, SeriesStats::all(Element::F64), None),
        0,
        &no_flags,
        1,
        (f64::NEG_INFINITY, f64::INFINITY),
        false,
    );

    // u8 with a declared clip range and only some statistics, 3 channels
    let shape = SampleShape::new(Element::U8, 3);
    let u: Vec<u8> = (0..90_000).map(|_| rng.next() as u8).collect();
    let stats = SeriesStats {
        min: true,
        clipped: true,
        flags: true,
        ..SeriesStats::default()
    };
    let clip = ClipRange {
        low: 10.5,
        high: 250.2,
    };
    check_series(
        &u,
        shape,
        spec(5, stats, Some(clip)),
        7,
        &some_flags,
        11,
        (10, 251),
        true,
    );

    // i64 near its ends: exact min and max, sums in f64
    let shape = SampleShape::scalar(Element::I64);
    let w: Vec<i64> = (0..5000).map(|_| rng.next() as i64).collect();
    check_series(
        &w,
        shape,
        spec(4, SeriesStats::all(shape.element), None),
        0,
        &no_flags,
        9,
        (i64::MIN, i64::MAX),
        false,
    );
}

#[test]
fn series_refuses_partial_frames() {
    let shape = SampleShape::new(Element::I16, 3);
    let start = SeriesStart {
        track_id: 1,
        track_number: 1,
        shape,
        first_item: 0,
        segment: SeriesSegment::start(0, 0, Magnitude::ONE),
        period: Ratio64::ONE,
    };
    let mut s =
        SeriesSummaries::<i16>::new(start, spec(4, SeriesStats::all(Element::I16), None)).unwrap();
    assert_eq!(
        s.push(&[1, 2, 3, 4], 0),
        Err(SummaryError::PartialFrame { stride: 3 })
    );
    s.push(&[1, 2, 3], 0).unwrap();
}

#[test]
fn series_refuses_bad_specs() {
    let start = |shape| SeriesStart {
        track_id: 1,
        track_number: 1,
        shape,
        first_item: 0,
        segment: SeriesSegment::start(0, 0, Magnitude::ONE),
        period: Ratio64::ONE,
    };
    let s = |shape, spec| SeriesSummaries::<i16>::new(start(shape), spec).err();
    let all = SeriesStats::all(Element::ComplexI16);
    assert!(
        s(SampleShape::scalar(Element::F32), spec(6, all, None)).is_some(),
        "element not the scalar"
    );
    assert!(
        s(SampleShape::new(Element::I16, 0), spec(6, all, None)).is_some(),
        "no channels"
    );
    assert!(
        s(SampleShape::scalar(Element::I16), spec(6, all, None)).is_some(),
        "power of a real element"
    );
    assert!(
        s(
            SampleShape::scalar(Element::I16),
            spec(3, SeriesStats::all(Element::I16), None)
        )
        .is_some()
    );
    assert!(
        s(
            SampleShape::scalar(Element::I16),
            spec(6, SeriesStats::default(), None)
        )
        .is_some()
    );
    assert!(s(SampleShape::new(Element::ComplexI16, 1), spec(6, all, None)).is_none());
}

#[test]
fn heads_read_back_and_bad_heads_are_refused() {
    let mut t = EventTree::new(
        5,
        6,
        EventFields {
            flags: true,
            latest_end: true,
        },
        100,
    );
    for i in 0..5000i64 {
        t.push(i * 10, i * 10 + 25, (i % 3) as u32).unwrap();
    }
    t.finish().unwrap();
    let blocks = t.take_blocks();
    let b = &blocks[0];
    let h = Summary::read(b).unwrap().head();
    assert_eq!(
        (h.track_id, h.track_number, h.tree, h.level, h.node_shift),
        (5, 6, Tree::Events, 0, 11)
    );
    assert_eq!(
        (h.first_item, h.items, h.first_node(), h.node_count()),
        (100, 5000, 0, 3)
    );
    assert_eq!(
        (h.first_tick, h.last_tick, h.latest_end),
        (0, 49_990, 50_015)
    );
    assert_eq!(h.node_items(0), (100, 1948));
    assert_eq!(h.node_items(2), (4096, 1004));
    // a flipped bit anywhere is a bad CRC; a head that lies with a good CRC is a bad summary
    for at in [5usize, 33, 50, 70, b.len() - 1] {
        let mut x = b.clone();
        x[at] ^= 1;
        assert!(Block::read(&x).is_err());
    }
    let restamp = |x: &mut Vec<u8>| {
        let n = x.len();
        let crc = crate::CRC.checksum(&x[..n - 4]);
        x[n - 4..].copy_from_slice(&crc.to_le_bytes());
    };
    for (at, v) in [
        (44usize, 9u8),
        (46, 12),
        (47, 1),
        (48, 0xff),
        (52, 1),
        (64, 0),
        (12, 9),
    ] {
        let mut x = b.clone();
        x[at] = v;
        if at == 64 {
            x[64..72].fill(0);
        }
        restamp(&mut x);
        assert!(
            matches!(Block::read(&x), Err(BlockError::Summary(_))),
            "byte {at}"
        );
    }
}

/// Series chunks in an aligned file with their summaries deferred: the summaries fill the gaps in front of the chunk
/// payloads and cost no file space, and a reader finds them all.
#[test]
fn summaries_fill_alignment_gaps() {
    use crate::{
        BlockKind, ChunkHead, Codec, ColumnKind, ColumnSpec, EndMode, FileBlocks, SyncPolicy,
        TapeWriter,
    };
    let dir = std::env::temp_dir().join(format!("ww_tape_summary_gaps_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shape = SampleShape::scalar(Element::I16);
    let chunk = 32_768usize;
    let chunks = 12;
    let samples: Vec<i16> = (0..chunk * chunks)
        .map(|i| (i as i16).wrapping_mul(31))
        .collect();
    let write = |name: &str, with_summaries: bool| {
        let path = dir.join(name);
        let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
        w.set_alignment(crate::ALIGNMENT).unwrap();
        let start = SeriesStart {
            track_id: 7,
            track_number: 1,
            shape,
            first_item: 0,
            segment: SeriesSegment::start(0, 0, Magnitude::pow10(-3)),
            period: Ratio64::int(1000).unwrap(),
        };
        let mut s =
            SeriesSummaries::<i16>::new(start, spec(10, SeriesStats::all(Element::I16), None))
                .unwrap();
        let mut chunk_ends = vec![];
        for c in 0..chunks {
            let xs = &samples[c * chunk..(c + 1) * chunk];
            if with_summaries {
                s.push(xs, 0).unwrap();
                for b in s.take_blocks() {
                    w.defer_block(b).unwrap();
                }
            }
            let bytes: Vec<u8> = xs.iter().flat_map(|x| x.to_le_bytes()).collect();
            let first = (c * chunk) as u64;
            let head = ChunkHead {
                kind: BlockKind::Series,
                track_id: 7,
                track_number: 1,
                first_item: first,
                count: chunk as u32,
                first_tick: first as i64 * 1000,
                last_tick: (first as i64 + chunk as i64 - 1) * 1000,
                latest_end: (first as i64 + chunk as i64 - 1) * 1000,
                time_mode: 0,
                end_mode: EndMode::Absent,
            };
            let col = ColumnSpec {
                kind: ColumnKind::Samples,
                codec: Codec::RAW,
                param: 0,
                len: bytes.len(),
            };
            w.write_chunk(&head, &[(col, &bytes)]).unwrap();
            chunk_ends.push(w.bytes());
        }
        if with_summaries {
            s.finish().unwrap();
            for b in s.take_blocks() {
                w.defer_block(b).unwrap();
            }
        }
        let path = w.finish().unwrap().path;
        (std::fs::read(path).unwrap(), chunk_ends)
    };
    let (plain, plain_ends) = write("plain.wwtape", false);
    let (file, ends) = write("summaries.wwtape", true);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(ends, plain_ends, "every summary block went into a gap");
    assert!(
        file.len() > plain.len(),
        "the last summaries follow the last chunk"
    );
    let mut rd = FileBlocks::new(&file[..]);
    let mut summaries = vec![];
    let mut seq = None;
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        if crate::index::is_structural(b.kind) {
            continue;
        }
        assert!(seq.is_none_or(|s| b.seq == s + 1), "seqs ascend");
        seq = Some(b.seq);
        if b.kind == BlockKind::Summary {
            summaries.push(b.bytes().to_vec());
        }
    }
    // the final index lists every summary block under its track, with its level
    let r = crate::TapeReader::open(&file[..]).unwrap();
    let listed: Vec<_> = r
        .track_entries(0, 7)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == BlockKind::Summary)
        .collect();
    assert_eq!(listed.len(), summaries.len());
    assert!(listed.iter().all(|e| e.tree == 2));
    assert!(listed.iter().any(|e| e.level > 0));
    let levels = series_levels(&summaries);
    assert_eq!(
        levels.keys().filter(|k| k.0 == 0).count(),
        chunk * chunks / 1024
    );
    let top = levels.keys().map(|k| k.0).max().unwrap();
    assert_eq!(levels.keys().filter(|k| k.0 == top).count(), 1);
}

/// Event tree and series blocks of a small fixed workload are the golden file's (`WW_TAPE_GOLDEN=update` rewrites
/// it): one block per line, in hex. Built twice: the same bytes.
#[test]
fn summary_golden() {
    let build = || {
        let mut t = EventTree::new(
            0xabcd,
            2,
            EventFields {
                flags: true,
                latest_end: true,
            },
            2000,
        );
        t.set_max_block_bytes(256);
        for i in 0..2200i64 {
            t.push(i * 7 - 3000, i * 7 - 3000 + (i % 13) * 50, 1 << (i % 5))
                .unwrap();
        }
        t.finish().unwrap();
        let mut blocks = t.take_blocks();
        let shape = SampleShape::new(Element::ComplexI16, 2);
        let start = SeriesStart {
            track_id: 0x5e,
            track_number: 3,
            shape,
            first_item: 30,
            segment: SeriesSegment::start(30, -100, Magnitude::pow10(-3)),
            period: Ratio64::new(62_500, 3).unwrap(),
        };
        let mut s =
            SeriesSummaries::<i16>::new(start, spec(4, SeriesStats::all(shape.element), None))
                .unwrap();
        let xs: Vec<i16> = (0..300 * 4)
            .map(|i| ((i * 7919) % 65_536 - 32_768) as i16)
            .collect();
        // a prefix change from milli to micro at sample 170, mid-node
        let (a, b) = xs.split_at((170 - 30) * 4);
        s.push(a, 0x10).unwrap();
        let micro = start.segment.rescaled(170, Magnitude::pow10(-6));
        s.segment(micro).unwrap();
        s.push(b, 0x10).unwrap();
        s.finish().unwrap();
        blocks.extend(s.take_blocks());
        blocks
    };
    let blocks = build();
    assert_eq!(blocks, build(), "deterministic");
    let mut hex = String::new();
    for b in &blocks {
        Block::read(b).unwrap();
        for byte in b {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex.push('\n');
    }
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-summary.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-summary.hex: run with WW_TAPE_GOLDEN=update");
    assert!(
        hex == want,
        "the format changed: bytes differ from golden/v2-summary.hex"
    );
}

#[test]
fn common_scales_and_factors() {
    let (milli, micro) = (Magnitude::pow10(-3), Magnitude::pow10(-6));
    assert_eq!(common_scale(milli, micro), Ok(micro));
    assert_eq!(common_scale(micro, milli), Ok(micro));
    assert_eq!(scale_factor(milli, micro), Ok(1000));
    // 1/1000 and 1/1024: 1/128000, factors 128 and 125
    let k = Magnitude::pow2(-10);
    let c = common_scale(milli, k).unwrap();
    assert_eq!(c, Magnitude::ratio(1, 128_000));
    assert_eq!(
        (scale_factor(milli, c), scale_factor(k, c)),
        (Ok(128), Ok(125))
    );
    assert_eq!(
        common_scale(Magnitude::ONE, Magnitude::ratio(1024, 1)),
        Ok(Magnitude::ONE)
    );
    assert_eq!(
        common_scale(Magnitude::ONE, Magnitude::PI),
        Err(ScaleError::NoCommonScale)
    );
    assert_eq!(
        scale_factor(micro, milli),
        Err(ScaleError::NoCommonScale),
        "not whole"
    );
    assert_eq!(
        scale_factor(Magnitude::pow10(20), Magnitude::ONE),
        Err(ScaleError::Overflow),
        "past 2^53"
    );
    // statistics of no samples rescale to themselves; integer ends that leave their type are refused
    let empty = ChannelStats::empty(Element::I32);
    assert!(same_bits(&empty.rescaled(1000).unwrap(), &empty));
    let mut big = empty;
    big.min[0] = Value::I64(i64::MAX / 10);
    big.max[0] = Value::I64(i64::MAX / 10);
    assert_eq!(big.rescaled(100), Err(ScaleError::Overflow));
    assert_eq!(
        big.rescaled(10).unwrap().max[0],
        Value::I64(i64::MAX / 10 * 10)
    );
}

/// Every node read back with the scale of its block: (level, node) to (first item, items, block scale, node).
fn scaled_nodes(blocks: &[Vec<u8>]) -> BTreeMap<(u8, u64), (u64, u64, Magnitude, SeriesNode)> {
    let mut m = BTreeMap::new();
    for b in blocks {
        let s = Block::read(b).unwrap().summary().unwrap();
        let h = s.head();
        for i in 0..s.node_count() {
            let (first, items) = h.node_items(i as u64);
            let node = s.series_node(i).unwrap();
            assert_eq!(Some(node.scale), s.scale());
            let old = m.insert(
                (h.level, h.first_node() + i as u64),
                (first, items, node.scale, node),
            );
            assert!(old.is_none(), "node written twice");
        }
    }
    m
}

fn int(v: Value) -> i128 {
    match v {
        Value::I64(x) => i128::from(x),
        Value::U64(x) => i128::from(x),
        Value::F64(x) => panic!("a float {x}"),
    }
}

/// A series whose scale changes at random samples (prefix changes, a binary prefix too): at every level each node's
/// statistics, read with its block's scale, equal a naive model of the samples in the sample type's unit; minimum
/// and maximum exactly. Every parent equals its children merged across their scales.
#[test]
fn series_prefix_changes_match_naive_model_in_the_unit() {
    let mut rng = Rng(7);
    let scales = [
        Magnitude::pow10(-3),
        Magnitude::pow10(-6),
        Magnitude::pow10(-9),
        Magnitude::pow2(-10),
    ];
    // the model's unit: a scale every sample's number is a whole multiple of
    let g = scales
        .into_iter()
        .reduce(|a, b| common_scale(a, b).unwrap())
        .unwrap();
    let k = |s: Magnitude| i128::from(scale_factor(s, g).unwrap());
    for (shape, base_shift, first_item, changes, frames) in [
        (SampleShape::scalar(Element::I16), 4, 0u64, 40, 9000usize),
        (SampleShape::new(Element::ComplexI16, 2), 5, 1000, 12, 6000),
        (SampleShape::new(Element::I16, 3), 4, 77, 1, 3000),
        (SampleShape::scalar(Element::I16), 4, 5, 2000, 4000),
    ] {
        let element = shape.element;
        let comps = usize::from(series::components(element));
        let channels = shape.channels.0 as usize;
        let stride = channels * comps;
        let data: Vec<i16> = (0..frames * stride).map(|_| rng.next() as i16).collect();
        let end = first_item + frames as u64;
        // change points, ascending and distinct, past the first sample
        let mut at: Vec<u64> = (0..changes)
            .map(|_| first_item + 1 + rng.below(frames as u64 - 1))
            .collect();
        at.sort_unstable();
        at.dedup();
        let first = SeriesSegment::start(first_item, 0, scales[0]);
        let start = SeriesStart {
            track_id: 9,
            track_number: 1,
            shape,
            first_item,
            segment: first,
            period: Ratio64::new(62_500, 3).unwrap(),
        };
        let mut s =
            SeriesSummaries::<i16>::new(start, spec(base_shift, SeriesStats::all(element), None))
                .unwrap();
        s.set_max_block_bytes(600);
        let mut scale_of = vec![scales[0]; frames];
        let mut flags_of = vec![0u32; frames];
        let mut blocks = vec![];
        let mut seg = first;
        let mut from = first_item;
        let mut k_push = 0usize;
        for &to in at.iter().chain([&end]) {
            // pushes of uneven sizes up to the next change
            while from < to {
                let n = (1 + rng.below(70)).min(to - from);
                let (a, b) = (
                    (from - first_item) as usize,
                    (from - first_item + n) as usize,
                );
                let f = 1u32 << (k_push % 9);
                flags_of[a..b].fill(f);
                scale_of[a..b].fill(seg.scale);
                s.push(&data[a * stride..b * stride], f).unwrap();
                blocks.extend(s.take_blocks());
                from += n;
                k_push += 1;
            }
            if to < end {
                seg = seg.rescaled(to, scales[rng.below(4) as usize]);
                s.segment(seg).unwrap();
            }
        }
        for (i, scale) in scale_of.iter().enumerate() {
            assert_eq!(s.timeline().scale(first_item + i as u64), Some(*scale));
        }
        s.finish().unwrap();
        blocks.extend(s.take_blocks());
        let got = scaled_nodes(&blocks);
        let mixed = got.values().filter(|v| !scales.contains(&v.2)).count();
        assert!(changes < 10 || mixed > 0, "some blocks hold two scales");
        // a block's scale is the common scale of its samples
        for b in &blocks {
            let sm = Block::read(b).unwrap().summary().unwrap();
            let h = sm.head();
            let r = (h.first_item - first_item) as usize
                ..(h.first_item + h.items - first_item) as usize;
            let want = scale_of[r]
                .iter()
                .copied()
                .reduce(|a, b| common_scale(a, b).unwrap())
                .unwrap();
            assert_eq!(
                sm.scale(),
                Some(want),
                "block L{} at {}",
                h.level,
                h.first_item
            );
        }
        for (&(level, n), (fi, items, bscale, node)) in &got {
            let r = (fi - first_item) as usize..(fi + items - first_item) as usize;
            let kb = k(*bscale);
            let kbf = kb as f64;
            for (ch, st) in node.channels.iter().enumerate() {
                // the samples of the node in the model's unit
                let val = |f: usize, c: usize| {
                    i128::from(data[f * stride + ch * comps + c]) * k(scale_of[f])
                };
                for c in 0..comps {
                    let xs: Vec<i128> = r.clone().map(|f| val(f, c)).collect();
                    let tag = format!("L{level} n{n} ch{ch} c{c}");
                    assert_eq!(int(st.min[c]) * kb, *xs.iter().min().unwrap(), "min {tag}");
                    assert_eq!(int(st.max[c]) * kb, *xs.iter().max().unwrap(), "max {tag}");
                    let sum: i128 = xs.iter().sum();
                    assert_eq!(st.sum[c] * kbf, sum as f64, "sum {tag}");
                    let sq: f64 = xs.iter().map(|x| (*x as f64).powi(2)).sum();
                    assert!(close(st.sum_sq[c] * kbf * kbf, sq, 1e-12), "sum_sq {tag}");
                }
                let clipped = r
                    .clone()
                    .filter(|&f| {
                        (0..comps).any(|c| {
                            let x = data[f * stride + ch * comps + c];
                            x == i16::MIN || x == i16::MAX
                        })
                    })
                    .count() as u64;
                assert_eq!(st.clipped, clipped, "clipped L{level} n{n}");
                assert_eq!(st.flags, flags_of[r.clone()].iter().fold(0, |a, b| a | b));
                if comps == 2 {
                    let p: Vec<f64> = r
                        .clone()
                        .map(|f| (val(f, 0) as f64).powi(2) + (val(f, 1) as f64).powi(2))
                        .collect();
                    let mean = p.iter().sum::<f64>() / p.len() as f64;
                    let pmax = p.iter().copied().fold(f64::NAN, f64::max);
                    let kk = kbf * kbf;
                    assert!(
                        close(f64::from(st.power_mean) * kk, mean, 1e-6),
                        "power mean L{level} n{n}"
                    );
                    assert!(
                        close(f64::from(st.power_max) * kk, pmax, 1e-6),
                        "power max L{level} n{n}"
                    );
                }
            }
            if level > 0 {
                // the children merged across their scales
                let mut m: Option<(SeriesNode, u64)> = None;
                for c in n << FANOUT_SHIFT..(n + 1) << FANOUT_SHIFT {
                    let Some((_, ci, _, cn)) = got.get(&(level - 1, c)) else {
                        continue;
                    };
                    m = Some(match m {
                        None => (cn.clone(), *ci),
                        Some((mut a, ai)) => {
                            a.merge(cn, ai, *ci).unwrap();
                            (a, ai + ci)
                        }
                    });
                }
                let (mut m, _) = m.unwrap();
                let mut p = node.clone();
                let c = common_scale(m.scale, p.scale).unwrap();
                m.rescale(c).unwrap();
                p.rescale(c).unwrap();
                for (a, b) in m.channels.iter().zip(&p.channels) {
                    assert_eq!((a.min, a.max), (b.min, b.max), "L{level} n{n}");
                    assert_eq!((a.sum, a.clipped, a.flags), (b.sum, b.clipped, b.flags));
                    for c in 0..comps {
                        assert!(close(a.sum_sq[c], b.sum_sq[c], 1e-12));
                    }
                    assert!(close(
                        f64::from(a.power_mean),
                        f64::from(b.power_mean),
                        1e-6
                    ));
                    assert!(close(f64::from(a.power_max), f64::from(b.power_max), 1e-6));
                }
            }
        }
        // the open tail a live reader takes from memory is at its own scale
        let (_, pending) = s.pending(0).unwrap();
        assert!(pending.is_empty(), "nothing pending after finish");
    }
}

/// A change of scale that can't be kept exactly is an error, never a silent mix.
#[test]
fn series_scale_changes_refused() {
    let start = |first: SeriesSegment| SeriesStart {
        track_id: 1,
        track_number: 1,
        shape: SampleShape::scalar(Element::I64),
        first_item: 0,
        segment: first,
        period: Ratio64::ONE,
    };
    let sp = spec(4, SeriesStats::all(Element::I64), None);
    let first = SeriesSegment::start(0, 0, Magnitude::ONE);
    let mut s = SeriesSummaries::<i64>::new(start(first), sp).unwrap();
    s.push(&[1, 2, 3], 0).unwrap();
    assert_eq!(
        s.segment(first.rescaled(3, Magnitude::PI)),
        Err(SummaryError::Scale(ScaleError::NoCommonScale))
    );
    assert!(
        matches!(
            s.segment(first.rescaled(2, Magnitude::pow10(-3))),
            Err(SummaryError::Invalid(_))
        ),
        "a segment not at the next sample"
    );
    // a value that doesn't fit i64 at the finer scale
    s.push(&[i64::MAX / 2], 0).unwrap();
    s.segment(first.rescaled(4, Magnitude::pow10(-3))).unwrap();
    s.push(&[5], 0).unwrap();
    assert_eq!(s.finish(), Err(SummaryError::Scale(ScaleError::Overflow)));
    // a pending open node across the change reports it too
    let mut s = SeriesSummaries::<i64>::new(start(first), sp).unwrap();
    s.push(&[i64::MAX / 2], 0).unwrap();
    s.segment(first.rescaled(1, Magnitude::pow10(-3))).unwrap();
    s.push(&[5], 0).unwrap();
    assert_eq!(
        s.pending(0).err(),
        Some(SummaryError::Scale(ScaleError::Overflow))
    );
    // a first sample before its segment
    let late = SeriesSegment::start(10, 0, Magnitude::ONE);
    assert!(SeriesSummaries::<i64>::new(start(late), sp).is_err());
}

/// A series block's scale: refused when missing, cut, or not as a scale writes it.
#[test]
fn series_block_scale_is_checked() {
    let shape = SampleShape::scalar(Element::I16);
    let first = SeriesSegment::start(0, 0, Magnitude::pow10(-3));
    let mut s = SeriesSummaries::<i16>::new(
        SeriesStart {
            track_id: 1,
            track_number: 1,
            shape,
            first_item: 0,
            segment: first,
            period: Ratio64::ONE,
        },
        spec(4, SeriesStats::all(shape.element), None),
    )
    .unwrap();
    s.push(&[1i16; 40], 0).unwrap();
    s.finish().unwrap();
    let b = s.take_blocks().remove(0);
    let sm = Block::read(&b).unwrap().summary().unwrap();
    assert_eq!(sm.scale(), Some(Magnitude::pow10(-3)));
    assert_eq!(&b[52..54], &[2, 0], "a power of ten takes 2 bytes");
    assert_eq!(&b[96..98], &[0xfa, 0x00]);
    let restamp = |x: &mut Vec<u8>| {
        let n = x.len();
        let crc = crate::CRC.checksum(&x[..n - 4]);
        x[n - 4..].copy_from_slice(&crc.to_le_bytes());
    };
    for (at, v) in [
        (52usize, 0u8),
        (52, 1),
        (52, 3),
        (52, 0xff),
        (53, 0xff),
        // the first channel or leaf of the block, reserved
        (54, 1),
        (55, 1),
        // the reserved leaves element
        (47, Element::LEAVES.code()),
        (97, 0xff),
    ] {
        let mut x = b.clone();
        x[at] = v;
        restamp(&mut x);
        assert!(
            matches!(Block::read(&x), Err(BlockError::Summary(_))),
            "byte {at} = {v}"
        );
    }
}

/// A series with a period of 62500/3 ticks cut into pieces at arbitrary samples, each piece summarised on its own from
/// the cut: every node lies on the uncut series' grid with its items, statistics and ticks, and only the nodes a cut
/// splits differ.
#[test]
fn series_cut_into_pieces_keeps_node_ticks() {
    let shape = SampleShape::scalar(Element::I16);
    let period = Ratio64::new(62_500, 3).unwrap();
    let segment = SeriesSegment::start(0, 1000, Magnitude::ONE);
    let data: Vec<i16> = (0..9000).map(|i| ((i * 31) % 2000 - 1000) as i16).collect();
    let summarise = |from: usize, to: usize| -> Vec<Vec<u8>> {
        let start = SeriesStart {
            track_id: 77,
            track_number: 2,
            shape,
            first_item: from as u64,
            segment,
            period,
        };
        let mut s =
            SeriesSummaries::<i16>::new(start, spec(4, SeriesStats::all(Element::I16), None))
                .unwrap();
        s.push(&data[from..to], 0).unwrap();
        s.finish().unwrap();
        s.take_blocks()
    };
    let tick = |item: u64| segment.tick(period, item).unwrap();
    let ticks_on_grid = |blocks: &[Vec<u8>]| {
        for b in blocks {
            let h = Block::read(b).unwrap().summary().unwrap().head();
            assert_eq!(h.first_tick, tick(h.first_item), "{h:?}");
            assert_eq!(h.last_tick, tick(h.first_item + h.items - 1), "{h:?}");
        }
    };
    let whole = summarise(0, data.len());
    ticks_on_grid(&whole);
    let uncut = series_levels(&whole);
    let cuts = [0, 1, 1234, 1235, 4096, 5003, 8999, data.len()];
    let mut pieces = BTreeMap::new();
    for w in cuts.windows(2) {
        let blocks = summarise(w[0], w[1]);
        ticks_on_grid(&blocks);
        for (k, v) in series_levels(&blocks) {
            pieces.entry(k).or_insert_with(Vec::new).push(v);
        }
    }
    let mut split = 0;
    for (k, got) in &pieces {
        let (first, items, stats) = &uncut[k];
        if let [(f, n, s)] = got.as_slice()
            && (f, n) == (first, items)
        {
            assert!(same_bits(s, stats), "{k:?}");
        } else {
            // a node a cut splits: its parts lie within the uncut node, apart (a piece's top level is the one where a
            // node covers it, so a short piece adds no part to the levels above)
            let mut parts: Vec<(u64, u64)> = got.iter().map(|g| (g.0, g.1)).collect();
            parts.sort_unstable();
            for p in &parts {
                assert!(
                    p.0 >= *first && p.0 + p.1 <= first + items,
                    "{k:?} {parts:?}"
                );
            }
            assert!(
                parts.windows(2).all(|w| w[0].0 + w[0].1 <= w[1].0),
                "{k:?} {parts:?}"
            );
            split += 1;
        }
    }
    assert!(
        pieces.keys().all(|k| uncut.contains_key(k)),
        "no node off the grid"
    );
    assert!(split > 0, "some nodes are split");
}
