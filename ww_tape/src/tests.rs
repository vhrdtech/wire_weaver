use super::*;
use shrink_wrap::prelude::*;

/// The run of blocks made by the tests.
pub(crate) const TEST_RUN: RunTag = RunTag::writable(0x7465_7374);

/// Block `seq` of [TEST_RUN].
pub(crate) fn id(seq: u64) -> BlockId {
    BlockId::new(TEST_RUN, seq)
}

const C1: u32 = 1;
const C3: u32 = 3;

fn at(clock: u32, ticks: i64) -> ClockTime {
    ClockTime::new(clock, ticks)
}

#[test]
fn write_and_read_back() {
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(7), &[C1]);
    w.push(1, at(C1, 1_000), &[b"ab"]).unwrap();
    w.push(300, at(C1, 900), &[b"c", b"de"]).unwrap();
    w.push_with(2, at(C1, 5_000), |b| {
        b[..3].copy_from_slice(b"xyz");
        Some(3)
    })
    .unwrap();
    assert_eq!(w.records(), 3);
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    assert_eq!(
        (
            block.kind,
            block.seq,
            block.record_count,
            block.clocks().collect::<Vec<_>>()
        ),
        (BlockKind::Records, 7, 3, vec![C1])
    );
    let records: Vec<_> = block.records().map(|r| r.unwrap()).collect();
    assert_eq!(
        records,
        [
            Record {
                track: 1,
                at: Some(at(C1, 1_000)),
                item: b"ab"
            },
            Record {
                track: 300,
                at: Some(at(C1, 900)),
                item: b"cde"
            },
            Record {
                track: 2,
                at: Some(at(C1, 5_000)),
                item: b"xyz"
            },
        ]
    );
    // header + clock table (2) + (1+2+1+2: 1000 is 2 bytes) + (2+2+1+3: -100 is 2 bytes) + (1+2+1+3) + crc
    assert_eq!(len, 32 + 2 + 6 + 8 + 7 + 4);
}

/// Deltas are per clock: records of two clocks interleave, each coded against the previous one on its clock, and
/// the slot costs one bit of the head.
#[test]
fn deltas_per_clock() {
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(0), &[C1, C3]);
    let big = 1_700_000_000_000_000_000i64;
    w.push(1, at(C1, 10), &[b"a"]).unwrap();
    w.push(2, at(C3, big), &[b"b"]).unwrap();
    w.push(1, at(C1, 11), &[b"c"]).unwrap();
    w.push(2, at(C3, big + 1), &[b"d"]).unwrap();
    w.push(5, at(C1, -4), &[b"e"]).unwrap();
    w.push(3, at(C1, i64::MIN), &[]).unwrap();
    w.push(3, at(C1, i64::MAX), &[]).unwrap();
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    assert_eq!(block.clocks().collect::<Vec<_>>(), [C1, C3]);
    let got: Vec<_> = block
        .records()
        .map(|r| r.unwrap())
        .map(|r| (r.track, r.at.unwrap(), r.item))
        .collect();
    assert_eq!(
        got,
        [
            (1, at(C1, 10), &b"a"[..]),
            (2, at(C3, big), b"b"),
            (1, at(C1, 11), b"c"),
            (2, at(C3, big + 1), b"d"),
            (5, at(C1, -4), b"e"),
            (3, at(C1, i64::MIN), b""),
            (3, at(C1, i64::MAX), b""),
        ]
    );
    // the second record of each clock is a one-byte delta
    let r = &buf[HEADER_LEN + 3..];
    let first_two = 1 + 1 + 2 + 1 + 9 + 2;
    assert_eq!(&r[first_two..first_two + 3], &[1 << 1, 2, 1]);
}

/// Describe records carry no time; a record on a slot the block has no clock for is damage.
#[test]
fn describe_records_have_no_time() {
    let mut buf = [0u8; 64];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, id(0), &[]);
    w.push_describe(&[b"xy"]).unwrap();
    let len = w.finish();
    assert_eq!(len, HEADER_LEN + 1 + 1 + 1 + 2 + CRC_LEN);
    let block = Block::read(&buf[..len]).unwrap();
    let r = block.records().next().unwrap().unwrap();
    assert_eq!((r.track, r.at, r.item), (DESCRIBE_TRACK, None, &b"xy"[..]));
    // track 1 in a block without clocks
    let mut bad = buf;
    bad[HEADER_LEN + 1] = 1 << 0;
    let crc = CRC.checksum(&bad[..len - CRC_LEN]);
    bad[len - CRC_LEN..len].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(
        Block::read(&bad[..len]).unwrap_err(),
        BlockError::BadRecords
    );
}

#[test]
#[should_panic(expected = "clock is one the block was started with")]
fn a_record_on_a_clock_not_in_the_block_panics() {
    let mut buf = [0u8; 64];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(0), &[C1]);
    _ = w.push(1, at(C3, 0), &[]);
}

#[test]
fn full_leaves_the_block_as_it_was() {
    let mut buf = [0u8; HEADER_LEN + 2 + CRC_LEN + 6];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(0), &[C1]);
    w.push(1, at(C1, 0), &[b"abc"]).unwrap();
    assert_eq!(w.push(1, at(C1, 0), &[b"ab"]), Err(Full));
    assert_eq!(w.push_with(1, at(C1, 0), |_| None), Err(Full));
    let len = w.finish();
    assert_eq!(Block::read(&buf[..len]).unwrap().record_count, 1);
}

#[test]
fn push_with_big_item_moves_down() {
    let mut buf = vec![0u8; 4096];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(0), &[C1]);
    let item: Vec<u8> = (0..200).map(|i| i as u8).collect();
    w.push_with(1, at(C1, 10), |b| {
        b[..200].copy_from_slice(&item);
        Some(200)
    })
    .unwrap();
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    let r = block.records().next().unwrap().unwrap();
    assert_eq!(r.item, &item[..]);
    assert_eq!(len, HEADER_LEN + 2 + 1 + 1 + 2 + 200 + CRC_LEN);
}

#[test]
fn corrupt_blocks_are_refused() {
    let mut buf = [0u8; 64];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, id(0), &[]);
    w.push_describe(&[b"x"]).unwrap();
    let len = w.finish();
    let mut bad = buf;
    bad[HEADER_LEN + 3] ^= 1;
    assert_eq!(Block::read(&bad[..len]).unwrap_err(), BlockError::BadCrc);
    assert_eq!(
        Block::read(&buf[..len - 1]).unwrap_err(),
        BlockError::Short { needed: len }
    );
    let mut bad = buf;
    bad[0] = b'X';
    assert_eq!(Block::read(&bad[..len]).unwrap_err(), BlockError::BadMagic);
}

const HOST: u32 = 1;
const WALL: u32 = 2;
/// The host counter's next epoch.
const EPOCH: u32 = 3;

/// The epoch link: tick 0 of [EPOCH] is tick 4e18 of [HOST].
const EPOCH_START: [MapSegment; 1] = [MapSegment::offset(0, 4_000_000_000_000_000_000)];

const SEGMENTS: [MapSegment; 2] = [
    MapSegment {
        a0: 0,
        a1: Some(1_000_000),
        b0: 1_763_164_800_000_000_000,
        slope: Ratio64::ONE,
        error: 500,
    },
    MapSegment {
        a0: 1_000_000,
        a1: None,
        b0: 1_763_164_800_001_000_020,
        slope: Ratio64::new(1_000_000_007, 999_999_931).unwrap(),
        error: 800,
    },
];

/// One record of every kind: a producer, a bundle, a host clock and the wall clock with a map between them, a track
/// with dimensions and an item type, and a typed item.
fn sample_describe() -> Vec<Describe<'static>> {
    const PATH: [UNib32; 3] = [UNib32(1), UNib32(3), UNib32(0)];
    const DIMS: [Dimension<'static>; 2] = [
        Dimension {
            name: "input",
            value: "cam1",
        },
        Dimension {
            name: "producer",
            value: "vad",
        },
    ];
    let item_type = TypeRef {
        bundle: RefVec::Slice {
            slice: &[0xaa, 0xbb],
        },
        krate: FullVersion::new("eventfull", Version::new(0, 4, 0)),
        path: "run::RunRecord",
    };
    vec![
        Describe::Item(TypedItem {
            ty: item_type.clone(),
            value: RefVec::Slice { slice: &[1, 2, 3] },
        }),
        Describe::Track(TrackDef {
            number: UNib32(4),
            key: track_key(api_root_id("ww_tap", 0), &[1, 3, 0]),
            clock: UNib32(HOST),
            dimensions: RefVec::Slice { slice: &DIMS },
            api: FullVersion::new("ww_tap", Version::new(0, 1, 0)),
            bundle: RefVec::Slice { slice: &[1, 2] },
            path: RefVec::Slice { slice: &PATH },
            item: Some(item_type),
            label: "link[3].tx.frames",
            sticky: true,
            kind: TrackKind::Events(EventsDef::INSTANTS),
        }),
        Describe::Map(ClockMap {
            from: UNib32(HOST),
            to: UNib32(WALL),
            segments: RefVec::Slice { slice: &SEGMENTS },
        }),
        Describe::Clock(ClockDef {
            number: UNib32(WALL),
            key: UNIX_CLOCK_KEY,
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::pow10(-6).into(),
            origin: ClockOrigin::UnixEpoch,
            label: "wall",
        }),
        Describe::Clock(ClockDef {
            number: UNib32(HOST),
            key: clock_key(api_root_id("host", 0), &[], 17),
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::ratio(1, 32768).into(),
            origin: ClockOrigin::Boot,
            label: "host",
        }),
        Describe::Bundle {
            hash: RefVec::Slice { slice: &[1, 2] },
            bytes: RefVec::Slice { slice: &[9; 40] },
        },
        Describe::Producer(Producer { name: "test" }),
        Describe::Map(ClockMap {
            from: UNib32(EPOCH),
            to: UNib32(HOST),
            segments: RefVec::Slice {
                slice: &EPOCH_START,
            },
        }),
        Describe::Clock(ClockDef {
            number: UNib32(EPOCH),
            key: clock_key(api_root_id("host", 0), &[], 17) ^ 1,
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::ratio(1, 32768).into(),
            origin: ClockOrigin::Epoch,
            label: "host epoch 1",
        }),
    ]
}

#[test]
fn describe_records_round_trip() {
    for d in sample_describe() {
        let mut scratch = [0u8; 256];
        let bytes = d.to_ww_bytes(&mut scratch).unwrap();
        let back = Describe::from_ww_bytes(bytes).unwrap();
        assert_eq!(back, d);
        let owned = DescribeOwned::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    }
}

#[test]
fn map_segments() {
    use Rounding::{Ceil, Floor, NearestEven};
    let [exact, fitted] = SEGMENTS;
    assert_eq!(exact.map(-1, Floor), None);
    assert_eq!(exact.map(10, Floor), Some(1_763_164_800_000_000_010));
    assert_eq!(exact.map(1_000_000, Floor), None);
    assert_eq!(fitted.map(1_000_000, Floor), Some(fitted.b0));
    // 999_999_931 ticks of a 1_000_000_007 / 999_999_931 slope are 1_000_000_007
    let whole = 1_000_000 + 999_999_931;
    for r in [Floor, Ceil, NearestEven] {
        assert_eq!(fitted.map(whole, r), Some(fitted.b0 + 1_000_000_007));
    }
    // 10^9 ticks are 1000000076.000005776..: the rounding is the caller's
    let a = 1_000_000 + 1_000_000_000;
    assert_eq!(fitted.map(a, Floor), Some(fitted.b0 + 1_000_000_076));
    assert_eq!(fitted.map(a, Ceil), Some(fitted.b0 + 1_000_000_077));
    assert_eq!(fitted.map(a, NearestEven), Some(fitted.b0 + 1_000_000_076));
    // a result past i64 is refused
    assert_eq!(MapSegment::offset(0, i64::MAX).map(1, Floor), None);
    assert_eq!(MapSegment::new(5, Some(5), 0, Ratio64::ONE, 0), None);

    // the same bytes with plain parts: a slope that isn't reduced, a zero part or an empty range is refused
    #[derive_shrink_wrap(borrowed, derive(Debug))]
    struct Raw {
        a0: i64,
        a1: Option<i64>,
        b0: i64,
        num: UVlq64,
        den: UVlq64,
        error: u64,
    }
    let raw = |a1, num, den| {
        let mut scratch = [0u8; 64];
        let r = Raw {
            a0: 5,
            a1,
            b0: 7,
            num: UVlq64(num),
            den: UVlq64(den),
            error: 3,
        };
        r.to_ww_bytes(&mut scratch).unwrap().to_vec()
    };
    let good = MapSegment::from_ww_bytes(&raw(Some(9), 3, 2)).unwrap();
    assert_eq!(
        good,
        MapSegment::new(5, Some(9), 7, Ratio64::new(3, 2).unwrap(), 3).unwrap()
    );
    for (a1, num, den) in [
        (Some(9), 2, 2),
        (Some(9), 0, 1),
        (Some(9), 1, 0),
        (Some(5), 1, 1),
    ] {
        assert!(
            MapSegment::from_ww_bytes(&raw(a1, num, den)).is_err(),
            "{a1:?} {num}/{den}"
        );
    }
}

#[test]
fn track_identity() {
    let key = track_key(api_root_id("ww_tap", 0), &[1, 3, 0]);
    assert_eq!(track_id(key, []), key);
    let d = |name, value| Dimension { name, value };
    let a = track_id(key, [d("input", "cam1")]);
    assert_ne!(a, key);
    assert_ne!(a, track_id(key, [d("input", "cam2")]));
    // the separator keeps `ab=c` and `a=bc` apart
    assert_ne!(track_id(key, [d("ab", "c")]), track_id(key, [d("a", "bc")]));
    let Describe::Track(t) = &sample_describe()[1] else {
        unreachable!("the second sample is a track")
    };
    assert!(t.dimensions_canonical());
    assert_eq!(
        t.id(),
        track_id(key, [d("input", "cam1"), d("producer", "vad")])
    );
    let unsorted = [d("producer", "vad"), d("input", "cam1")];
    let t = TrackDef {
        dimensions: RefVec::Slice { slice: &unsorted },
        ..t.clone()
    };
    assert!(!t.dimensions_canonical());
    let mut set = DescribeSet::new();
    assert_eq!(
        set.insert(&Describe::Track(t)),
        Err(DescribeError::Dimensions { track: 4 })
    );
}

fn describe_set(records: impl IntoIterator<Item = Describe<'static>>) -> DescribeSet {
    let mut set = DescribeSet::new();
    for d in records {
        set.insert(&d).unwrap();
    }
    set
}

/// Records go in canonical order whatever order they were added in; a record replaces the one with its key.
#[test]
fn describe_set_is_canonical() {
    let forward = describe_set(sample_describe());
    let backward = describe_set(sample_describe().into_iter().rev());
    let ranks: Vec<Rank> = forward
        .records()
        .map(|r| Describe::from_ww_bytes(r).unwrap().rank())
        .collect();
    assert_eq!(
        ranks,
        [
            Rank::Producer,
            Rank::Bundle,
            Rank::Clock(HOST),
            Rank::Clock(WALL),
            Rank::Clock(EPOCH),
            Rank::Map {
                from: HOST,
                to: WALL
            },
            Rank::Map {
                from: EPOCH,
                to: HOST
            },
            Rank::Track(4),
            Rank::Other,
        ]
    );
    assert!(forward.records().eq(backward.records()));
    let mut replaced = forward.clone();
    replaced
        .insert(&Describe::Producer(Producer { name: "other" }))
        .unwrap();
    assert_eq!(replaced.len(), forward.len());
    assert!(!replaced.records().eq(forward.records()));
}

#[test]
fn describe_set_needs_its_clocks() {
    let no_wall: Vec<_> = sample_describe()
        .into_iter()
        .filter(|d| d.rank() != Rank::Clock(WALL))
        .collect();
    let set = describe_set(no_wall);
    assert_eq!(
        set.check(),
        Err(DescribeError::UnknownClock { clock: WALL })
    );
}

/// A record's clock is its track's (T11): a sticky record on another clock, or on a track not described, is refused.
#[test]
fn sticky_records_are_on_their_tracks_clock() {
    let set = describe_set(sample_describe());
    let write = |track, clock| {
        let sticky = [Sticky {
            track,
            at: at(clock, 0),
            item: b"s",
        }];
        set.write_blocks(&mut id(0), 1024, &sticky, |_| {})
    };
    assert_eq!(write(4, HOST), Ok(()));
    assert_eq!(
        write(4, WALL),
        Err(DescribeError::WrongClock {
            track: 4,
            clock: WALL,
            track_clock: HOST
        })
    );
    assert_eq!(
        write(4, 9),
        Err(DescribeError::WrongClock {
            track: 4,
            clock: 9,
            track_clock: HOST
        })
    );
    assert_eq!(
        write(5, HOST),
        Err(DescribeError::UnknownTrack { track: 5 })
    );
}

fn map(from: u32, to: u32, segments: &'static [MapSegment]) -> Describe<'static> {
    Describe::Map(ClockMap {
        from: UNib32(from),
        to: UNib32(to),
        segments: RefVec::Slice { slice: segments },
    })
}

/// A map is strictly increasing over all its segments: refused on insert and on decode when a segment overlaps the
/// one before, follows one without an end, or starts at or below its last value.
#[test]
fn maps_increase_across_segments() {
    let fitted = SEGMENTS[1];
    assert!(segments_increasing(SEGMENTS));
    let last = SEGMENTS[0].last().unwrap();
    assert_eq!(last, SEGMENTS[0].b0 + 999_999);
    static AT_LAST: [MapSegment; 2] = [
        SEGMENTS[0],
        MapSegment {
            b0: 1_763_164_800_000_999_999,
            ..SEGMENTS[1]
        },
    ];
    static OVERLAP: [MapSegment; 2] = [
        SEGMENTS[0],
        MapSegment {
            a0: 999_999,
            ..SEGMENTS[1]
        },
    ];
    static AFTER_OPEN: [MapSegment; 2] = [SEGMENTS[1], SEGMENTS[1]];
    let mut set = DescribeSet::new();
    assert_eq!(set.insert(&map(HOST, WALL, &SEGMENTS)), Ok(()));
    for bad in [&AT_LAST[..], &OVERLAP, &AFTER_OPEN] {
        let d = map(HOST, WALL, bad);
        assert_eq!(
            set.insert(&d),
            Err(DescribeError::MapNotIncreasing {
                from: HOST,
                to: WALL
            })
        );
        let mut scratch = [0u8; 256];
        let bytes = d.to_ww_bytes(&mut scratch).unwrap();
        assert!(Describe::from_ww_bytes(bytes).is_err());
        assert!(DescribeOwned::from_ww_bytes_owned(bytes).is_err());
    }
    // one above the last value is fine
    static ABOVE: [MapSegment; 2] = [
        SEGMENTS[0],
        MapSegment {
            b0: 1_763_164_800_001_000_000,
            ..SEGMENTS[1]
        },
    ];
    assert_eq!(set.insert(&map(HOST, WALL, &ABOVE)), Ok(()));
    assert_eq!(fitted.last(), None);
}

/// An epoch clock needs its epoch link: a map from it to the previous epoch's clock, one exact offset segment from
/// tick 0 to the end with `b0 > 0`. The reverse direction, a fitted segment, a bounded one or a second one don't do.
#[test]
fn epoch_clocks_need_their_link() {
    let without: Vec<_> = sample_describe()
        .into_iter()
        .filter(|d| {
            d.rank()
                != (Rank::Map {
                    from: EPOCH,
                    to: HOST,
                })
        })
        .collect();
    let set = describe_set(without);
    let unmapped = Err(DescribeError::EpochUnmapped { clock: EPOCH });
    assert_eq!(set.check(), unmapped);
    let seg = MapSegment::offset(0, 4_000);
    static REVERSED: [MapSegment; 1] = [MapSegment::offset(4_000, 0)];
    static FITTED: [MapSegment; 1] = [MapSegment {
        error: 5,
        ..MapSegment::offset(0, 4_000)
    }];
    static SLOPED: [MapSegment; 1] = [MapSegment {
        slope: Ratio64::new(1001, 1000).unwrap(),
        ..MapSegment::offset(0, 4_000)
    }];
    static LATE: [MapSegment; 1] = [MapSegment::offset(1, 4_000)];
    static AT_ZERO: [MapSegment; 1] = [MapSegment::offset(0, 0)];
    static BOUNDED: [MapSegment; 1] = [MapSegment {
        a1: Some(10),
        ..MapSegment::offset(0, 4_000)
    }];
    static TWO: [MapSegment; 2] = [
        MapSegment {
            a1: Some(10),
            ..MapSegment::offset(0, 4_000)
        },
        MapSegment::offset(10, 5_000),
    ];
    for (from, to, segments) in [
        (HOST, EPOCH, &REVERSED[..]),
        (EPOCH, HOST, &FITTED),
        (EPOCH, HOST, &SLOPED),
        (EPOCH, HOST, &LATE),
        (EPOCH, HOST, &AT_ZERO),
        (EPOCH, HOST, &BOUNDED),
        (EPOCH, HOST, &TWO),
        (EPOCH, HOST, &[]),
    ] {
        let mut s = set.clone();
        s.insert(&map(from, to, segments)).unwrap();
        assert_eq!(s.check(), unmapped, "{from} -> {to}: {segments:?}");
    }
    let mut s = set.clone();
    s.insert(&map(EPOCH, HOST, &EPOCH_START)).unwrap();
    assert_eq!(s.check(), Ok(()));
    assert!(seg.is_exact_offset());
}

/// A clock's resolution is at least one of its ticks: `resolution · rate >= 1`.
#[test]
fn clock_resolution_at_least_a_tick() {
    let clock = |rate: Magnitude, resolution: Magnitude| {
        Describe::Clock(ClockDef {
            number: UNib32(1),
            key: 1,
            rate: rate.into(),
            resolution: resolution.into(),
            origin: ClockOrigin::Free,
            label: "c",
        })
    };
    let mut set = DescribeSet::new();
    let ns = Magnitude::pow10(9);
    // exactly one tick, more than one, a 32 kHz counter read in ns, an NTSC frame clock
    for (rate, resolution) in [
        (ns, Magnitude::pow10(-9)),
        (ns, Magnitude::pow10(-6)),
        (ns, Magnitude::ratio(1, 32768)),
        (
            Magnitude::ratio(60_000, 1001),
            Magnitude::ratio(1001, 60_000),
        ),
    ] {
        assert_eq!(
            set.insert(&clock(rate, resolution)),
            Ok(()),
            "{rate} {resolution}"
        );
    }
    for (rate, resolution) in [
        (ns, Magnitude::pow10(-10)),
        (
            Magnitude::ratio(60_000, 1001),
            Magnitude::ratio(1000, 60_000),
        ),
        (Magnitude::pow10(3), Magnitude::new(1, 4000, 0, 0, 1)),
    ] {
        assert_eq!(
            set.insert(&clock(rate, resolution)),
            Err(DescribeError::Resolution { clock: 1 }),
            "{rate} {resolution}"
        );
    }
}

/// A describe set written as blocks: definitions on track 0, then the sticky records on their tracks and clocks;
/// split over several blocks when they are small.
#[test]
fn describe_set_as_blocks() {
    let mut set = describe_set(sample_describe());
    let Describe::Track(t) = &sample_describe()[1] else {
        unreachable!("the second sample is a track")
    };
    let on_wall = TrackDef {
        number: UNib32(9),
        clock: UNib32(WALL),
        ..t.clone()
    };
    set.insert(&Describe::Track(on_wall)).unwrap();
    let sticky = [
        Sticky {
            track: 9,
            at: at(WALL, 5),
            item: b"late",
        },
        Sticky {
            track: 4,
            at: at(HOST, -3),
            item: b"session",
        },
    ];
    for block_len_max in [64, 4096] {
        let mut blocks = vec![];
        let mut seq = id(10);
        set.write_blocks(&mut seq, block_len_max, &sticky, |b| {
            blocks.push(b.to_vec())
        })
        .unwrap();
        assert_eq!(seq, id(10 + blocks.len() as u64));
        assert_eq!(blocks.len() > 1, block_len_max == 64);
        let mut records = vec![];
        for b in &blocks {
            let b = Block::read(b).unwrap();
            assert_eq!(b.kind, BlockKind::Describe);
            for r in b.records() {
                let r = r.unwrap();
                records.push((r.track, r.at, r.item.to_vec()));
            }
        }
        let mut want: Vec<_> = set.records().map(|r| (0, None, r.to_vec())).collect();
        want.push((4, Some(at(HOST, -3)), b"session".to_vec()));
        want.push((9, Some(at(WALL, 5)), b"late".to_vec()));
        assert_eq!(records, want);
    }
}

#[test]
fn resync_skips_bad_and_reports_cut() {
    let mut file = Vec::new();
    for seq in 0..3u64 {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(seq), &[C1]);
        w.push(1, at(C1, seq as i64), &[&[seq as u8; 5]]).unwrap();
        let len = w.finish();
        file.extend_from_slice(&buf[..len]);
    }
    let one = file.len() / 3;
    // damage the middle block, cut the last one
    file[one + HEADER_LEN + 2] ^= 0xff;
    file.truncate(file.len() - 3);
    let mut rd = FileBlocks::with_recovery(&file[..], Recovery::Resync);
    assert_eq!(rd.next_block().unwrap().unwrap().seq, 0);
    match rd.next_block().unwrap() {
        Err(FileError::Bad { offset, error, .. }) => {
            assert_eq!((offset, error), (one as u64, BlockError::BadCrc))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        rd.next_block().unwrap(),
        Err(FileError::Truncated { .. })
    ));
    assert!(rd.next_block().is_none());
}

/// A later version's block and a kind this reader doesn't know are checked, skipped by their length and given out
/// without records, not taken for damage; a describe variant it doesn't know reads as `Unknown`.
#[test]
fn unknown_versions_kinds_and_describe_variants_are_skipped() {
    let mut file = Vec::new();
    for (seq, version, kind) in [
        (0u64, VERSION, 1u8),
        (1, VERSION + 1, 1),
        (2, VERSION, 200),
        (3, VERSION, 1),
        // parity: reserved, not read
        (4, VERSION, 5),
    ] {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(seq), &[C1]);
        w.push(1, at(C1, seq as i64), &[&[seq as u8; 5]]).unwrap();
        let len = w.finish();
        buf[4] = version;
        buf[5] = kind;
        // a later version's body: not records
        if version != VERSION {
            buf[HEADER_LEN] = 0xff;
        }
        let crc = CRC.checksum(&buf[..len - CRC_LEN]);
        buf[len - CRC_LEN..len].copy_from_slice(&crc.to_le_bytes());
        file.extend_from_slice(&buf[..len]);
    }
    let mut rd = FileBlocks::new(&file[..]);
    let mut seen = vec![];
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        seen.push((b.seq, b.body_known(), b.records().count()));
    }
    assert_eq!(
        seen,
        [
            (0, true, 1),
            (1, false, 0),
            (2, false, 0),
            (3, true, 1),
            (4, false, 0)
        ]
    );
    assert_eq!(BlockKind::from_u8(200), BlockKind::Other(200));

    // a variant appended later, as a newer producer writes it
    #[derive_shrink_wrap(derive(Debug), ww_repr = u4)]
    enum NewerDescribe<'i> {
        Producer(Producer<'i>),
        Track(TrackDef<'i>),
        Bundle {
            hash: RefVec<'i, u8>,
            bytes: RefVec<'i, u8>,
        },
        Clock(ClockDef<'i>),
        Map(ClockMap<'i>),
        Item(TypedItem<'i>),
        Layout(Layout),
        Index {
            first_seq: u64,
        },
    }
    let mut scratch = [0u8; 32];
    let bytes = NewerDescribe::Index { first_seq: 7 }
        .to_ww_bytes(&mut scratch)
        .unwrap();
    assert!(matches!(
        Describe::from_ww_bytes(bytes),
        Ok(Describe::Unknown(7))
    ));
}

/// `n` data blocks of different sizes back to back, and where each ends.
fn tape(n: u64) -> (Vec<u8>, Vec<usize>) {
    let mut file = Vec::new();
    let mut ends = vec![];
    for seq in 0..n {
        let mut buf = [0u8; 128];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(seq), &[C1]);
        for i in 0..=seq % 3 {
            w.push(
                1 + i as u32,
                at(C1, (seq * 10 + i) as i64),
                &[&[seq as u8; 7][..(seq as usize % 7) + 1]],
            )
            .unwrap();
        }
        let len = w.finish();
        file.extend_from_slice(&buf[..len]);
        ends.push(file.len());
    }
    (file, ends)
}

/// Seqs of the blocks a reader gives, and whether it reported a cut or damage.
fn read_all(bytes: &[u8], recovery: Recovery) -> (Vec<u64>, usize) {
    let mut rd = FileBlocks::with_recovery(bytes, recovery);
    let (mut seqs, mut errors) = (vec![], 0);
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) => seqs.push(b.seq),
            Err(_) => errors += 1,
        }
        assert!(errors < 10_000, "the walk ends");
    }
    (seqs, errors)
}

/// A capture cut at every byte reads as the blocks before the cut, the cut said when it is inside a block.
#[test]
fn prefix_of_a_capture_cut_anywhere() {
    let (file, ends) = tape(8);
    for at in 0..=file.len() {
        let whole = ends.iter().filter(|e| **e <= at).count() as u64;
        let at_boundary = at == 0 || ends.contains(&at);
        let (seqs, errors) = read_all(&file[..at], Recovery::Prefix);
        assert_eq!(seqs, (0..whole).collect::<Vec<_>>(), "cut at {at}");
        assert_eq!(errors, usize::from(!at_boundary), "cut at {at}");
        // the salvage reader doesn't panic either, and loses nothing before the cut
        let (seqs, _) = read_all(&file[..at], Recovery::Resync);
        assert_eq!(seqs, (0..whole).collect::<Vec<_>>(), "resync, cut at {at}");
    }
}

/// A flipped byte anywhere: the blocks before its block, then one cut; the salvage reader gets every other block.
#[test]
fn prefix_of_a_capture_with_a_flipped_byte() {
    let (file, ends) = tape(8);
    for at in 0..file.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut bad = file.clone();
            bad[at] ^= flip;
            let block = ends.iter().position(|e| at < *e).unwrap() as u64;
            let (seqs, errors) = read_all(&bad, Recovery::Prefix);
            assert_eq!(
                seqs,
                (0..block).collect::<Vec<_>>(),
                "flip {flip:#x} at {at}"
            );
            assert_eq!(errors, 1, "flip {flip:#x} at {at}");
            let (seqs, _) = read_all(&bad, Recovery::Resync);
            let others: Vec<u64> = (0..8).filter(|s| *s != block).collect();
            assert_eq!(seqs, others, "resync, flip {flip:#x} at {at}");
        }
    }
}

/// Zeros after the last block (preallocated space) are the end; anything else there is a cut.
#[test]
fn zeros_at_the_end_are_the_end() {
    let (mut file, _) = tape(3);
    file.extend_from_slice(&[0u8; 100_000]);
    assert_eq!(read_all(&file, Recovery::Prefix), (vec![0, 1, 2], 0));
    file.push(1);
    assert_eq!(read_all(&file, Recovery::Prefix), (vec![0, 1, 2], 1));
}

/// A data block whose seq goes back is a cut; a describe block may go back (another producer run appended).
#[test]
fn seq_going_back() {
    let block = |kind, seq| {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, kind, seq, &[C1]);
        w.push(1, at(C1, 0), &[b"x"]).unwrap();
        let len = w.finish();
        buf[..len].to_vec()
    };
    let file: Vec<u8> = [
        block(BlockKind::Records, id(5)),
        block(BlockKind::Records, id(6)),
        block(BlockKind::Describe, id(0)),
        block(BlockKind::Records, id(1)),
        block(BlockKind::Records, id(1)),
        block(BlockKind::Records, id(2)),
    ]
    .concat();
    let mut rd = FileBlocks::new(&file[..]);
    let mut seqs = vec![];
    let mut cut = None;
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) => seqs.push(b.seq),
            Err(FileError::Cut { why, .. }) => cut = Some(why),
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(seqs, [5, 6, 0, 1]);
    assert_eq!(cut, Some(Cut::SeqBackwards { seq: 1, after: 1 }));
}

#[test]
fn writer_partial_then_renamed() {
    let dir = std::env::temp_dir().join(format!("ww_tape_writer_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let (file, ends) = tape(4);
    let mut w = TapeWriter::create(&path, SyncPolicy::EveryBlocks(2)).unwrap();
    assert_eq!(w.current_path(), writer::partial_name(&path));
    let mut start = 0;
    for end in &ends {
        w.write_block(&file[start..*end]).unwrap();
        start = *end;
    }
    assert!(!path.exists(), "named only when finished");
    assert_eq!(std::fs::read(w.current_path()).unwrap(), file);
    assert_eq!((w.blocks(), w.bytes()), (4, file.len() as u64));
    assert_eq!(w.finish().unwrap().path, path);
    // the final index, finish and tail after the blocks
    assert!(std::fs::read(&path).unwrap().starts_with(&file));
    assert!(!writer::partial_name(&path).exists());
    assert!(matches!(
        TapeWriter::create(&path, SyncPolicy::None),
        Err(WriteError::Exists(_))
    ));
    // appended: the second run's blocks start with a describe block
    let mut w = TapeWriter::append(&path, SyncPolicy::default()).unwrap();
    let mut buf = [0u8; 64];
    let len = BlockWriter::new(&mut buf, BlockKind::Describe, id(0), &[]).finish();
    w.write_block(&buf[..len]).unwrap();
    w.tick().unwrap();
    w.finish().unwrap();
    let (seqs, errors) = read_all(&std::fs::read(&path).unwrap(), Recovery::Prefix);
    // each run ends with its final index, finish and tail, the second also with its describe backup: they carry
    // the seq of the last counted block
    assert_eq!(
        (seqs, errors),
        (vec![0, 1, 2, 3, 3, 3, 3, 0, 0, 0, 0, 0], 0)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A capture passes the producer's blocks through with their own seqs: the checkpoints and describe repeats the
/// writer adds between them take no seq of their own, so the prefix walk reads the whole file.
#[test]
fn pass_through_capture_reads_to_the_end() {
    let dir = std::env::temp_dir().join(format!("ww_tape_pass_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    _ = std::fs::remove_file(&path);
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    w.set_index_policy(IndexPolicy {
        bytes: 1024,
        interval: None,
    });
    w.set_describe_repeat(2048);
    let mut buf = [0u8; 64];
    let len = BlockWriter::new(&mut buf, BlockKind::Describe, id(0), &[]).finish();
    w.write_block(&buf[..len]).unwrap();
    let (file, ends) = tape(60);
    let mut start = 0;
    for end in &ends {
        // the producer numbers its blocks 1 and up after its describe block
        let mut b = file[start..*end].to_vec();
        let seq = Block::read(&b).unwrap().seq;
        set_id(&mut b, id(seq + 1)).unwrap();
        w.write_block(&b).unwrap();
        start = *end;
    }
    w.finish().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let mut rd = FileBlocks::new(&bytes[..]);
    let (mut kinds, mut counted) = (vec![], vec![]);
    while let Some(b) = rd.next_block() {
        let b = b.expect("no cut");
        kinds.push(b.kind);
        if b.kind == BlockKind::Records {
            counted.push(b.seq);
        }
    }
    assert_eq!(counted, (1..=60).collect::<Vec<_>>());
    assert!(
        kinds.iter().filter(|k| **k == BlockKind::Index).count() >= 3,
        "checkpoints and the final index"
    );
    assert!(
        kinds.iter().filter(|k| **k == BlockKind::Describe).count() >= 3,
        "repeats and the backup"
    );
    assert_eq!(kinds.last(), Some(&BlockKind::Tail));
}

/// A failed write is the end of the file: later writes and syncs are refused, never retried.
#[cfg(target_os = "linux")]
#[test]
fn writer_stops_after_a_failure() {
    let mut w = TapeWriter::append(std::path::Path::new("/dev/full"), SyncPolicy::None).unwrap();
    let (file, ends) = tape(1);
    // /dev/full takes nothing: the write fails with ENOSPC
    let first = w.write_block(&file[..ends[0]]);
    assert!(matches!(first, Err(WriteError::Io { .. })), "{first:?}");
    assert!(matches!(
        w.write_block(&file[..ends[0]]),
        Err(WriteError::Failed)
    ));
    assert!(matches!(w.sync(), Err(WriteError::Failed)));
    assert!(matches!(w.finish(), Err(WriteError::Failed)));
}

#[test]
fn varints() {
    use varint::*;
    for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
        let mut b = [0u8; 10];
        let n = put_uvar(&mut b, v);
        assert_eq!(n, uvar_len(v));
        assert_eq!(Reader(&b[..n]).uvar(), Some(v));
    }
    for v in [0i64, -1, 1, -64, 64, i64::MIN, i64::MAX] {
        let mut b = [0u8; 10];
        let n = put_ivar(&mut b, v);
        assert_eq!(n, ivar_len(v));
        assert_eq!(Reader(&b[..n]).ivar(), Some(v));
    }
}

/// A tape of the sample describe set, a sticky record and a records block on two clocks, written through
/// [TapeWriter] to `path`, the describe records added in the order given.
fn write_sample_tape(path: &std::path::Path, describe: Vec<Describe<'static>>) -> Vec<u8> {
    _ = std::fs::remove_file(path);
    let mut w = TapeWriter::create(path, SyncPolicy::None).unwrap();
    let set = describe_set(describe);
    let sticky = [Sticky {
        track: 4,
        at: at(HOST, 7),
        item: b"session",
    }];
    let mut seq = id(0);
    let mut blocks = vec![];
    set.write_blocks(&mut seq, 4096, &sticky, |b| blocks.push(b.to_vec()))
        .unwrap();
    let mut buf = [0u8; 256];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, seq, &[HOST, WALL]);
    b.push(4, at(HOST, 1_000), &[b"one"]).unwrap();
    b.push(4, at(HOST, 1_250), &[b"two"]).unwrap();
    b.push(9, at(WALL, 1_763_164_800_000_000_000), &[b"wall"])
        .unwrap();
    let len = b.finish();
    blocks.push(buf[..len].to_vec());
    for b in &blocks {
        w.write_block(b).unwrap();
    }
    w.finish().unwrap();
    let bytes = std::fs::read(path).unwrap();
    // the writer's final index, describe backup, finish and tail follow; their golden file is golden/v2-index.hex
    let written = blocks.concat();
    assert!(bytes.starts_with(&written));
    written
}

fn to_hex_lines(tape: &[u8]) -> String {
    let mut out = String::new();
    let mut rd = FileBlocks::new(tape);
    while let Some(b) = rd.next_block() {
        for byte in b.unwrap().bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push('\n');
    }
    out
}

/// The same input gives the same bytes, whatever order the describe records came in, and those bytes are the
/// golden file's (`WW_TAPE_GOLDEN=update` rewrites it): one block per line, in hex.
#[test]
fn deterministic_and_golden() {
    let dir = std::env::temp_dir().join(format!("ww_tape_golden_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = write_sample_tape(&dir.join("a.wwtape"), sample_describe());
    let b = write_sample_tape(
        &dir.join("b.wwtape"),
        sample_describe().into_iter().rev().collect(),
    );
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(a, b, "two writes of the same input");

    let hex = to_hex_lines(&a);
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &hex).unwrap();
    }
    let want =
        std::fs::read_to_string(&golden).expect("golden/v2.hex: run with WW_TAPE_GOLDEN=update");
    assert_eq!(
        hex, want,
        "the format changed: bytes differ from golden/v2.hex"
    );

    // and the golden file reads back to the same records
    let mut describe = vec![];
    let mut times = vec![];
    let mut rd = FileBlocks::new(&a[..]);
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        for r in b.records() {
            let r = r.unwrap();
            match r.at {
                None => describe.push(DescribeOwned::from_ww_bytes_owned(r.item).unwrap()),
                Some(t) => times.push((r.track, t)),
            }
        }
    }
    let mut want: Vec<_> = sample_describe();
    want.sort_by_key(Describe::rank);
    let want: Vec<DescribeOwned> = want
        .iter()
        .map(|d| {
            let mut scratch = [0u8; 256];
            DescribeOwned::from_ww_bytes_owned(d.to_ww_bytes(&mut scratch).unwrap()).unwrap()
        })
        .collect();
    assert_eq!(describe, want);
    assert_eq!(
        times,
        [
            (4, at(HOST, 7)),
            (4, at(HOST, 1_000)),
            (4, at(HOST, 1_250)),
            (9, at(WALL, 1_763_164_800_000_000_000)),
        ]
    );
}

/// Typed items hash as they compare, borrowed and owned (eventfull keys its items by them).
#[test]
fn typed_items_hash() {
    use std::collections::HashSet;
    use std::hash::BuildHasher;
    let ty = TypeRef {
        bundle: RefVec::Slice { slice: &[1, 2] },
        krate: FullVersion::new("eventfull", Version::new(0, 4, 0)),
        path: "run::RunRecord",
    };
    let item = |v: &'static [u8]| TypedItem {
        ty: ty.clone(),
        value: RefVec::Slice { slice: v },
    };
    let state = std::hash::RandomState::new();
    assert_eq!(state.hash_one(item(&[7])), state.hash_one(item(&[7])));
    assert_ne!(state.hash_one(item(&[7])), state.hash_one(item(&[8])));
    let mut scratch = [0u8; 128];
    let mut owned = |v| {
        TypedItemOwned::from_ww_bytes_owned(item(v).to_ww_bytes(&mut scratch).unwrap()).unwrap()
    };
    let set: HashSet<TypedItemOwned> = [owned(&[7]), owned(&[8]), owned(&[7])]
        .into_iter()
        .collect();
    assert_eq!(set.len(), 2);
}
