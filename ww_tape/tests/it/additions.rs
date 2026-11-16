//! Describe additions: records that join the current describe set mid-piece, the whole set in every copy after
//! them, and [TapeReader::describe_set] with an addition damaged.

use std::path::Path;

use crate::pieces::records;
use shrink_wrap::prelude::*;
use ww_tape::index::IndexPolicy;
use ww_tape::piece::{PieceHead, PieceId, PieceIdent, ResultId};
use ww_tape::{
    BlockId, BlockKind, ClockDef, ClockOrigin, Describe, DescribeSet, EventsDef, FLAG_SET_ADD,
    FileBlocks, FullVersion, Layout, Magnitude, MemSink, Producer, Recovery, RunTag, Segment,
    SyncPolicy, TapeReader, TapeSink, TapeWriter, TrackDef, TrackKind, Version, WriteError,
    WriterOptions, clock_key, track_key,
};

/// The clock of the tests' tracks.
const CLOCK: u32 = 1;

pub(crate) fn clock(number: u32) -> Describe<'static> {
    Describe::Clock(ClockDef {
        number: UNib32(number),
        key: clock_key(0xad, &[], 1),
        rate: Magnitude::pow10(9).into(),
        resolution: Magnitude::pow10(-9).into(),
        origin: ClockOrigin::Boot,
        label: "host",
    })
}

/// Events track `number` on `clock`.
pub(crate) fn track(number: u32, clock: u32) -> Describe<'static> {
    Describe::Track(TrackDef {
        number: UNib32(number),
        key: track_key(0xad, &[Segment::Index(number)]),
        clock: UNib32(clock),
        dimensions: RefVec::Slice { slice: &[] },
        api: FullVersion::new("additions_api", Version::new(0, 1, 0)),
        bundle: RefVec::Slice { slice: &[] },
        path: RefVec::Slice {
            slice: &[UNib32(7)],
        },
        keys: RefVec::Slice { slice: &[] },
        item: None,
        label: "added",
        sticky: false,
        kind: TrackKind::Events(EventsDef::INSTANTS),
    })
}

/// `records` as an addition in blocks of at most `max` bytes, numbered from `first` on.
pub(crate) fn addition(records: &[Describe<'_>], first: BlockId, max: usize) -> Vec<Vec<u8>> {
    let mut set = DescribeSet::new();
    for r in records {
        set.insert(r).unwrap();
    }
    let mut id = first;
    let mut out = vec![];
    set.write_addition(&mut id, max, &[], |b| out.push(b.to_vec()))
        .unwrap();
    out
}

/// The track numbers of the set describe blocks `blocks` make.
pub(crate) fn tracks<'b>(blocks: impl IntoIterator<Item = &'b [u8]>) -> Vec<u32> {
    let (set, _) = DescribeSet::read_blocks(blocks).unwrap();
    set.records()
        .filter_map(|r| match Describe::from_ww_bytes(r).unwrap() {
            Describe::Track(t) => Some(t.number.0),
            _ => None,
        })
        .collect()
}

/// A good describe block in a piece: its offset, flags and bytes.
struct DescribeAt {
    offset: u64,
    flags: u16,
    copy: bool,
    bytes: Vec<u8>,
}

fn describe_blocks(bytes: &[u8]) -> Vec<DescribeAt> {
    let mut rd = FileBlocks::with_recovery(bytes, Recovery::Prefix);
    let mut out = vec![];
    loop {
        let Some(Ok(b)) = rd.next_block() else {
            return out;
        };
        if b.kind != BlockKind::Describe {
            continue;
        }
        let (flags, copy, bytes) = (b.flags, b.is_copy(), b.bytes().to_vec());
        out.push(DescribeAt {
            offset: rd.offset(),
            flags,
            copy,
            bytes,
        });
    }
}

/// The whole copies in `blocks`: each one's first offset and blocks.
fn copies(blocks: &[DescribeAt]) -> Vec<(u64, Vec<&[u8]>)> {
    let mut out: Vec<(u64, Vec<&[u8]>)> = vec![];
    for b in blocks.iter().filter(|b| b.copy) {
        if b.flags & ww_tape::FLAG_SET_FIRST != 0 {
            out.push((b.offset, vec![]));
        }
        out.last_mut().unwrap().1.push(&b.bytes);
    }
    out
}

fn options(describe_repeat: u64) -> WriterOptions {
    WriterOptions {
        sync: SyncPolicy::None,
        index: IndexPolicy {
            bytes: 1024,
            interval: None,
        },
        alignment: 0,
        describe_repeat,
    }
}

/// A first piece on a memory sink, its describe set written: a producer, a layout, a clock and track 1.
fn begin(o: WriterOptions) -> TapeWriter<MemSink> {
    let mut w = TapeWriter::create_on(MemSink::new(Path::new("p")), o.sync);
    w.set_index_policy(o.index);
    w.set_describe_repeat(o.describe_repeat);
    let result = ResultId([5; 32]);
    w.begin_piece(
        &PieceHead {
            ident: PieceIdent {
                id: PieceId::first(&result),
                result,
                seq: 0,
            },
            prev: None,
            prev_run: RunTag::NONE,
        },
        &[],
    )
    .unwrap();
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "additions" }))
        .unwrap();
    set.insert(&Describe::Layout(Layout::UNALIGNED)).unwrap();
    set.insert(&clock(CLOCK)).unwrap();
    set.insert(&track(1, CLOCK)).unwrap();
    let mut id = w.next_id();
    let mut blocks = vec![];
    set.write_blocks(&mut id, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    w.write_describe(&blocks).unwrap();
    w
}

fn write_records<S: TapeSink>(w: &mut TapeWriter<S>, from: i64, n: i64) {
    for k in from..from + n {
        w.write_block(&records(w.next_id(), k)).unwrap();
    }
}

/// Tracks 2 and 3 added mid-piece, in an addition of two blocks: every copy after it carries them (the repeats and the backup
/// before the finish), none before it does, and [TapeReader::describe_set] has them, also once the addition's own
/// blocks are damaged.
#[test]
fn a_track_added_mid_piece_is_in_every_copy_after_it() {
    let mut w = begin(options(2 << 10));
    write_records(&mut w, 0, 30);
    let add = addition(&[track(2, CLOCK), track(3, CLOCK)], w.next_id(), 40);
    assert_eq!(add.len(), 2, "an addition of two blocks");
    w.write_describe(&add).unwrap();
    write_records(&mut w, 100, 30);
    let tape = w.sink().tape();
    w.finish().unwrap();
    let bytes = tape.to_vec().unwrap();

    let blocks = describe_blocks(&bytes);
    let added: Vec<u64> = (blocks.iter())
        .filter(|b| b.flags & FLAG_SET_ADD != 0)
        .map(|b| b.offset)
        .collect();
    assert_eq!(added.len(), 2);
    let copies = copies(&blocks);
    let (before, after): (Vec<_>, Vec<_>) = copies.iter().partition(|c| c.0 < added[0]);
    assert!(
        before.len() >= 2,
        "early and repeat copies: {}",
        before.len()
    );
    assert!(after.len() >= 2, "a repeat and the backup: {}", after.len());
    for (at, set) in &before {
        assert_eq!(tracks(set.iter().copied()), [1], "copy at {at}");
    }
    for (at, set) in &after {
        assert_eq!(tracks(set.iter().copied()), [1, 2, 3], "copy at {at}");
    }

    let r = TapeReader::open(&bytes[..]).unwrap();
    let set = r.describe_set(0).unwrap();
    assert_eq!(set.base, after.last().map(|c| c.0), "the backup");
    assert!(set.lost.is_empty());
    assert_eq!(tracks(set.bytes()), [1, 2, 3]);

    for at in &added {
        let mut damaged = bytes.clone();
        damaged[*at as usize + 40] ^= 1;
        let r = TapeReader::open(&damaged[..]).unwrap();
        let set = r.describe_set(0).unwrap();
        assert_eq!(
            tracks(set.bytes()),
            [1, 2, 3],
            "addition block at {at} damaged"
        );
        assert!(set.lost.is_empty(), "{:?}", set.lost);
    }
}

/// An unfinished piece with an addition after its last copy: [TapeReader::describe_set] gives the copy and the
/// addition after it, and with the addition damaged the copy alone, the addition's blocks listed as lost.
#[test]
fn an_addition_with_no_copy_after_it_is_read_or_listed_lost() {
    let mut w = begin(options(u64::MAX));
    write_records(&mut w, 0, 30);
    let add = addition(&[track(2, CLOCK), track(3, CLOCK)], w.next_id(), 40);
    w.write_describe(&add).unwrap();
    // checkpoints after it, so a reader knows the addition from the index
    write_records(&mut w, 100, 30);
    let bytes = w.sink().tape().to_vec().unwrap();
    drop(w);

    let blocks = describe_blocks(&bytes);
    let added: Vec<u64> = (blocks.iter())
        .filter(|b| b.flags & FLAG_SET_ADD != 0)
        .map(|b| b.offset)
        .collect();
    let last_copy = copies(&blocks).last().unwrap().0;
    assert!(last_copy < added[0], "no copy after the addition");

    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(r.pieces()[0].cut.is_none_or(|c| c > added[1]));
    let set = r.describe_set(0).unwrap();
    assert_eq!(set.base, Some(last_copy));
    assert_eq!(tracks(set.bytes()), [1, 2, 3]);
    assert!(set.lost.is_empty());

    for k in 0..2 {
        let mut damaged = bytes.clone();
        damaged[added[k] as usize + 40] ^= 1;
        let r = TapeReader::open(&damaged[..]).unwrap();
        let set = r.describe_set(0).unwrap();
        assert_eq!(set.base, Some(last_copy));
        assert_eq!(tracks(set.bytes()), [1], "block {k} damaged");
        assert_eq!(set.lost, added, "block {k} damaged");
    }
}

/// A resumed piece keeps the set with the addition merged: the backup it writes before the finish carries it.
#[test]
fn a_resumed_piece_keeps_its_additions() {
    let o = options(u64::MAX);
    let mut w = begin(o);
    write_records(&mut w, 0, 30);
    let add = addition(&[track(2, CLOCK), track(3, CLOCK)], w.next_id(), 40);
    w.write_describe(&add).unwrap();
    write_records(&mut w, 100, 5);
    let bytes = w.sink().tape().to_vec().unwrap();
    drop(w);

    let mut sink = MemSink::new(Path::new("p"));
    sink.write_all(&bytes).unwrap();
    let tape = sink.tape();
    let (mut w, _) = TapeWriter::resume_on(sink, o).unwrap();
    write_records(&mut w, 200, 5);
    w.finish().unwrap();
    let bytes = tape.to_vec().unwrap();
    let blocks = describe_blocks(&bytes);
    let backup = copies(&blocks).pop().unwrap();
    assert_eq!(tracks(backup.1), [1, 2, 3]);
    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(r.verify(0).unwrap());
    assert_eq!(r.describe_set(0).unwrap().base, Some(backup.0));
}

/// An addition that doesn't join the set (its track on a clock the set lacks) is refused before any of its blocks is
/// written; the piece goes on and ends without it.
#[test]
fn an_addition_that_does_not_join_the_set_is_refused() {
    let mut w = begin(options(u64::MAX));
    write_records(&mut w, 0, 3);
    let tape = w.sink().tape();
    let len = tape.to_vec().unwrap().len();
    let next = w.next_seq();
    for max in [40, 4096] {
        let add = addition(&[track(2, 9), track(3, 9)], w.next_id(), max);
        assert_eq!(add.len(), if max == 40 { 2 } else { 1 });
        let e = w.write_describe(&add).unwrap_err();
        assert!(matches!(e, WriteError::Describe(_)), "{e:?}");
        assert_eq!(tape.to_vec().unwrap().len(), len, "nothing written");
        assert_eq!(w.next_seq(), next);
    }
    // one block alone, through write_block
    let add = addition(&[track(2, 9)], w.next_id(), 4096);
    assert!(matches!(
        w.write_block(&add[0]),
        Err(WriteError::Describe(_))
    ));
    assert_eq!(tape.to_vec().unwrap().len(), len, "nothing written");
    write_records(&mut w, 10, 3);
    w.finish().unwrap();
    let bytes = tape.to_vec().unwrap();
    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(r.verify(0).unwrap());
    let set = r.describe_set(0).unwrap();
    assert_eq!(tracks(set.bytes()), [1]);
    assert!(set.lost.is_empty());
}

/// The next piece of a cut starts with the whole set, the addition merged.
#[test]
fn the_next_piece_of_a_cut_starts_with_the_additions() {
    let dir = crate::pieces::scratch("additions_cut");
    let o = options(u64::MAX);
    let opts = ww_tape::ChainOptions {
        writer: o,
        cut: ww_tape::CutPolicy::default(),
    };
    let mut c = ww_tape::PieceChain::create(&dir, ResultId([6; 32]), 0, &[], opts).unwrap();
    let w = c.writer().unwrap();
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "additions" }))
        .unwrap();
    set.insert(&clock(CLOCK)).unwrap();
    set.insert(&track(1, CLOCK)).unwrap();
    let mut id = w.next_id();
    let mut blocks = vec![];
    set.write_blocks(&mut id, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    c.write_describe(&blocks).unwrap();
    let w = c.writer().unwrap();
    write_records(w, 0, 3);
    let add = addition(&[track(2, CLOCK), track(3, CLOCK)], w.next_id(), 40);
    c.write_describe(&add).unwrap();
    c.cut_now().unwrap();
    write_records(c.writer().unwrap(), 10, 3);
    let pieces = c.finish().unwrap();
    assert_eq!(pieces.len(), 2);
    let bytes = std::fs::read(&pieces[1].path).unwrap();
    let r = TapeReader::open(&bytes[..]).unwrap();
    let set = r.describe_set(0).unwrap();
    assert_eq!(tracks(set.bytes()), [1, 2, 3]);
    // the set the piece starts with, before its data
    let blocks = describe_blocks(&bytes);
    let first = &copies(&blocks)[0];
    assert_eq!(first.0, blocks[0].offset);
    assert_eq!(tracks(first.1.iter().copied()), [1, 2, 3]);
    std::fs::remove_dir_all(&dir).unwrap();
}
