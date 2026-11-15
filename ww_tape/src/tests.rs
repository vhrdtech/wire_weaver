use super::*;
use shrink_wrap::prelude::*;

const C1: u32 = 1;
const C3: u32 = 3;

fn at(clock: u32, ticks: i64) -> ClockTime {
    ClockTime::new(clock, ticks)
}

#[test]
fn write_and_read_back() {
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 7, &[C1]);
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
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 0, &[C1, C3]);
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
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, 0, &[]);
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
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 0, &[C1]);
    _ = w.push(1, at(C3, 0), &[]);
}

#[test]
fn full_leaves_the_block_as_it_was() {
    let mut buf = [0u8; HEADER_LEN + 2 + CRC_LEN + 6];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 0, &[C1]);
    w.push(1, at(C1, 0), &[b"abc"]).unwrap();
    assert_eq!(w.push(1, at(C1, 0), &[b"ab"]), Err(Full));
    assert_eq!(w.push_with(1, at(C1, 0), |_| None), Err(Full));
    let len = w.finish();
    assert_eq!(Block::read(&buf[..len]).unwrap().record_count, 1);
}

#[test]
fn push_with_big_item_moves_down() {
    let mut buf = vec![0u8; 4096];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 0, &[C1]);
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
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, 0, &[]);
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

const SEGMENTS: [MapSegment; 2] = [
    MapSegment {
        a0: 0,
        a1: Some(1_000_000),
        b0: 1_763_164_800_000_000_000,
        num: 1,
        den: 1,
        error: 500,
    },
    MapSegment {
        a0: 1_000_000,
        a1: None,
        b0: 1_763_164_800_001_000_020,
        num: 1_000_000_007,
        den: 999_999_931,
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
        }),
        Describe::Map(ClockMap {
            from: UNib32(HOST),
            to: UNib32(WALL),
            segments: RefVec::Slice { slice: &SEGMENTS },
        }),
        Describe::Clock(ClockDef {
            number: UNib32(WALL),
            key: UNIX_CLOCK_KEY,
            rate: Magnitude::pow10(9),
            resolution: Magnitude::pow10(-6),
            origin: ClockOrigin::UnixEpoch,
            label: "wall",
        }),
        Describe::Clock(ClockDef {
            number: UNib32(HOST),
            key: clock_key(api_root_id("host", 0), &[], 17),
            rate: Magnitude::ratio(60_000, 1001),
            resolution: Magnitude::ratio(1, 32768),
            origin: ClockOrigin::Boot,
            label: "host",
        }),
        Describe::Bundle {
            hash: RefVec::Slice { slice: &[1, 2] },
            bytes: RefVec::Slice { slice: &[9; 40] },
        },
        Describe::Producer(Producer { name: "test" }),
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
    let [exact, fitted] = SEGMENTS;
    assert_eq!(exact.map(-1), None);
    assert_eq!(exact.map(10), Some(1_763_164_800_000_000_010));
    assert_eq!(exact.map(1_000_000), None);
    assert_eq!(fitted.map(1_000_000), Some(fitted.b0));
    // 999_999_931 ticks of a 1_000_000_007 / 999_999_931 slope are 1_000_000_007
    assert_eq!(
        fitted.map(1_000_000 + 999_999_931),
        Some(fitted.b0 + 1_000_000_007)
    );
    assert_eq!(
        MapSegment::new(0, None, 0, 6, 4, 0).map(|s| (s.num, s.den)),
        Some((3, 2))
    );
    assert_eq!(MapSegment::new(0, None, 0, 0, 1, 0), None);
    assert_eq!(MapSegment::new(5, Some(5), 0, 1, 1, 0), None);
    // a slope that isn't reduced, or a zero one, is refused on reading
    for (num, den) in [(2, 2), (0, 1), (1, 0)] {
        let s = MapSegment { num, den, ..exact };
        let mut scratch = [0u8; 64];
        let bytes = s.to_ww_bytes(&mut scratch).unwrap();
        assert!(MapSegment::from_ww_bytes(bytes).is_err(), "{num}/{den}");
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
            Rank::Map {
                from: HOST,
                to: WALL
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
    let set = describe_set(sample_describe());
    let sticky = [Sticky {
        track: 4,
        at: at(9, 0),
        item: b"s",
    }];
    let r = set.write_blocks(&mut 0, 1024, &sticky, |_| {});
    assert_eq!(r, Err(DescribeError::UnknownClock { clock: 9 }));
}

/// A describe set written as blocks: definitions on track 0, then the sticky records on their tracks and clocks;
/// split over several blocks when they are small.
#[test]
fn describe_set_as_blocks() {
    let set = describe_set(sample_describe());
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
        let mut seq = 10;
        set.write_blocks(&mut seq, block_len_max, &sticky, |b| {
            blocks.push(b.to_vec())
        })
        .unwrap();
        assert_eq!(seq, 10 + blocks.len() as u64);
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
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, seq, &[C1]);
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
        (2, VERSION, 9),
        (3, VERSION, 1),
    ] {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, seq, &[C1]);
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
        [(0, true, 1), (1, false, 0), (2, false, 0), (3, true, 1)]
    );
    assert_eq!(BlockKind::from_u8(9), BlockKind::Other(9));

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
        Ok(Describe::Unknown(6))
    ));
}

/// `n` data blocks of different sizes back to back, and where each ends.
fn tape(n: u64) -> (Vec<u8>, Vec<usize>) {
    let mut file = Vec::new();
    let mut ends = vec![];
    for seq in 0..n {
        let mut buf = [0u8; 128];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, seq, &[C1]);
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
        block(BlockKind::Records, 5),
        block(BlockKind::Records, 6),
        block(BlockKind::Describe, 0),
        block(BlockKind::Records, 1),
        block(BlockKind::Records, 1),
        block(BlockKind::Records, 2),
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
    assert_eq!(w.finish().unwrap(), path);
    assert_eq!(std::fs::read(&path).unwrap(), file);
    assert!(!writer::partial_name(&path).exists());
    assert!(matches!(
        TapeWriter::create(&path, SyncPolicy::None),
        Err(WriteError::Exists(_))
    ));
    // appended: the second run's blocks start with a describe block
    let mut w = TapeWriter::append(&path, SyncPolicy::default()).unwrap();
    let mut buf = [0u8; 64];
    let len = BlockWriter::new(&mut buf, BlockKind::Describe, 0, &[]).finish();
    w.write_block(&buf[..len]).unwrap();
    w.tick().unwrap();
    w.finish().unwrap();
    let (seqs, errors) = read_all(&std::fs::read(&path).unwrap(), Recovery::Prefix);
    assert_eq!((seqs, errors), (vec![0, 1, 2, 3, 0], 0));
    std::fs::remove_dir_all(&dir).unwrap();
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
    let mut seq = 0;
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
    assert_eq!(bytes, blocks.concat());
    bytes
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
