//! Index, finish, tail, piece chains and the positional reader, through the public API.

use std::path::{Path, PathBuf};

use ww_tape::chunk::ChunkHead;
use ww_tape::index::IndexPolicy;
use ww_tape::piece::{ChainEnd, ChainLink, PieceId, check_chain};
use ww_tape::reader::{Found, OpenOptions};
use ww_tape::{
    BlockId, BlockKind, BlockWriter, ClockTime, Codec, ColumnKind, ColumnSpec, Describe,
    DescribeSet, EndMode, FileBlocks, FinishState, Layout, OwnerId, PieceChain, Producer, Recovery,
    SessionTag, SyncPolicy, TapeReader, TapeWriter,
};

/// A fresh scratch directory.
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww_tape_pieces_{name}_{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Describe blocks numbered from `first` on.
fn describe_blocks(layout: Layout, first: BlockId) -> Vec<Vec<u8>> {
    describe_blocks_max(layout, first, 4096)
}

/// The describe set of [describe_blocks] in blocks of at most `max` bytes.
pub(crate) fn describe_blocks_max(layout: Layout, first: BlockId, max: usize) -> Vec<Vec<u8>> {
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "pieces" }))
        .unwrap();
    set.insert(&Describe::Layout(layout)).unwrap();
    let mut seq = first;
    let mut blocks = vec![];
    set.write_blocks(&mut seq, max, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    blocks
}

pub(crate) fn records(id: BlockId, n: i64) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]).unwrap();
    b.push(9, ClockTime::new(1, n), &[&[n as u8; 30]]).unwrap();
    let len = b.finish();
    buf[..len].to_vec()
}

/// Track `t`'s chunk `k`: 50 items with raw starts.
pub(crate) fn chunk(t: u64, k: u64) -> (ChunkHead, Vec<u8>) {
    let first = k * 50;
    let starts: Vec<u8> = (0..50)
        .flat_map(|i| ((first + i) as i64 * 10 + t as i64).to_le_bytes())
        .collect();
    let head = ChunkHead {
        kind: BlockKind::Events,
        track_id: 0x1000 + t,
        track_number: t as u32 + 1,
        first_item: first,
        count: 50,
        first_tick: first as i64 * 10 + t as i64,
        last_tick: (first + 49) as i64 * 10 + t as i64,
        latest_end: (first + 49) as i64 * 10 + t as i64,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    (head, starts)
}

pub(crate) fn column(bytes: &[u8]) -> [(ColumnSpec, &[u8]); 1] {
    [(
        ColumnSpec {
            kind: ColumnKind::Starts,
            codec: Codec::RAW,
            param: 0,
            len: bytes.len(),
        },
        bytes,
    )]
}

/// Describe blocks, then `rounds` × (a chunk of each of 3 tracks, a records block), checkpoints every 2 KiB.
fn write_tape(w: &mut TapeWriter, rounds: u64) {
    w.set_index_policy(IndexPolicy {
        bytes: 2048,
        interval: None,
    });
    for b in describe_blocks(Layout::UNALIGNED, w.next_id()) {
        w.write_block(&b).unwrap();
    }
    for k in 0..rounds {
        for t in 0..3 {
            let (h, c) = chunk(t, k);
            w.write_chunk(&h, &column(&c)).unwrap();
        }
        w.write_block(&records(w.next_id(), k as i64)).unwrap();
    }
}

/// Offset, length and kind of every block a FileBlocks walk finds.
fn walk(bytes: &[u8]) -> Vec<(u64, u32, BlockKind)> {
    let mut rd = FileBlocks::with_recovery(bytes, Recovery::Prefix);
    let mut out = vec![];
    while let Some(Ok(b)) = rd.next_block() {
        let (len, kind) = (b.len() as u32, b.kind);
        out.push((rd.offset(), len, kind));
    }
    out
}

fn listed(kind: BlockKind) -> bool {
    !matches!(
        kind,
        BlockKind::Index | BlockKind::Finish | BlockKind::Tail | BlockKind::PieceHead
    )
}

#[test]
fn finished_file_is_found_by_its_tail() {
    let dir = scratch("tail");
    let path = dir.join("t.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    write_tape(&mut w, 20);
    let done = w.finish().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(done.len, bytes.len() as u64);

    let r = TapeReader::open(&bytes[..]).unwrap();
    assert_eq!(r.pieces().len(), 1);
    let p = &r.pieces()[0];
    assert_eq!(
        (p.start, p.end, p.found),
        (0, bytes.len() as u64, Found::Tail)
    );
    let (finish, ranges) = p.finish.as_ref().unwrap();
    assert_eq!(finish.state, FinishState::Complete);
    assert_eq!(finish.hash, done.hash);
    assert!(finish.checkpoint.is_some());
    assert!(r.verify(0).unwrap());
    assert_eq!(ranges.len(), 3);
    assert_eq!(
        (
            ranges[1].track_id,
            ranges[1].chunks,
            ranges[1].first_item,
            ranges[1].items
        ),
        (0x1001, 20, 0, 1000)
    );

    // the index lists every block a walk finds, but the index, finish and tail blocks
    let want: Vec<_> = walk(&bytes).into_iter().filter(|b| listed(b.2)).collect();
    let got: Vec<_> = r
        .entries(0)
        .unwrap()
        .iter()
        .map(|e| (e.offset, e.len, e.kind))
        .collect();
    assert_eq!(got, want);

    // one track from the final index's head and its own entries
    let t1 = r.track_entries(0, 0x1001).unwrap();
    assert_eq!(t1.len(), 20);
    assert!(t1.windows(2).all(|w| w[0].end_item() == w[1].first_item));
    let mut buf = vec![];
    let b = r.read_block(0, &t1[3], &mut buf).unwrap();
    assert_eq!(b.chunk().unwrap().head().first_item, 150);
    let rec = r.track_entries(0, 0).unwrap();
    assert_eq!(
        rec.iter().filter(|e| e.kind == BlockKind::Records).count(),
        20
    );
    assert_eq!(
        rec.iter().filter(|e| e.kind == BlockKind::Describe).count(),
        4,
        "first, the copies after the first data block and the first checkpoint, and the backup"
    );

    // a flipped byte before the finish fails the content hash
    let mut bad = bytes.clone();
    bad[100] ^= 1;
    assert!(!TapeReader::open(&bad[..]).unwrap().verify(0).unwrap());

    // the same through a file
    let f = std::fs::File::open(&path).unwrap();
    let r = TapeReader::open(f).unwrap();
    assert_eq!(r.track_entries(0, 0x1001).unwrap(), t1);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A writer that stopped anywhere: every block up to the cut is found, from the newest checkpoint and a scan of
/// at most the checkpoint interval (plus the blocks that straddle it).
#[test]
fn crash_anywhere_loses_only_the_cut_block() {
    let dir = scratch("crash");
    let path = dir.join("t.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    write_tape(&mut w, 12);
    // the writer dies here: no finish
    drop(w);
    let bytes = std::fs::read(dir.join("t.wwt.partial")).unwrap();
    let blocks = walk(&bytes);
    assert!(blocks.iter().filter(|b| b.2 == BlockKind::Index).count() >= 3);
    for at in (0..=bytes.len()).step_by(7).chain([bytes.len()]) {
        let cut = &bytes[..at];
        let r = TapeReader::open(cut).unwrap();
        let want: Vec<_> = blocks
            .iter()
            .filter(|b| b.0 + u64::from(b.1) <= at as u64 && listed(b.2))
            .map(|b| (b.0, b.1, b.2))
            .collect();
        let got: Vec<_> = match r.pieces() {
            [] => vec![],
            [p] => r
                .entries(0)
                .unwrap()
                .iter()
                .map(|e| (e.offset + p.start, e.len, e.kind))
                .collect(),
            more => panic!("one piece, got {}", more.len()),
        };
        assert_eq!(got, want, "cut at {at}");
        if let Some(p) = r.pieces().first() {
            assert!(p.finish.is_none());
            let whole = blocks.iter().any(|b| b.0 + u64::from(b.1) == at as u64);
            assert_eq!(p.cut.is_some(), !whole && at > 0, "cut at {at}");
            if let Found::Checkpoint { scanned } = p.found {
                assert!(scanned <= 2048 + 4 * 600, "scanned {scanned} at {at}");
            }
        }
    }
    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(matches!(r.pieces()[0].found, Found::Checkpoint { .. }));
    assert!(r.verify(0).is_err(), "no finish, no hash");
    // a short scan back finds no checkpoint: the whole file is scanned instead
    let r = TapeReader::open_with(
        &bytes[..],
        OpenOptions {
            scan_back_max: 16,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(r.pieces()[0].found, Found::Scan { .. }));
    assert_eq!(
        r.entries(0).unwrap().len(),
        blocks.iter().filter(|b| listed(b.2)).count()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In an aligned file the checkpoints go into the gaps before chunk payloads, which stay on 4 KiB boundaries.
#[test]
fn aligned_file_places_checkpoints_in_gaps() {
    let dir = scratch("aligned");
    let path = dir.join("t.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    w.set_alignment(ww_tape::ALIGNMENT).unwrap();
    write_tape(&mut w, 10);
    w.finish().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut rd = FileBlocks::new(&bytes[..]);
    let mut checkpoints = 0;
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        let payload = b.chunk().map(|c| c.payload_offset() as u64);
        checkpoints += usize::from(b.kind == BlockKind::Index);
        if let Some(p) = payload {
            assert_eq!((rd.offset() + p) % 4096, 0);
        }
    }
    assert!(checkpoints >= 3);
    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(r.verify(0).unwrap());
    assert_eq!(r.track_entries(0, 0x1002).unwrap().len(), 10);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Write chain `n` of a owner, pieces cut every 4 chunks; the piece files in order.
fn write_chain(dir: &Path, n: u64) -> Vec<PathBuf> {
    let mut chain = PieceChain::create(
        dir,
        OwnerId([0x42; 32]),
        n,
        &[],
        ww_tape::ChainOptions {
            writer: ww_tape::WriterOptions {
                sync: SyncPolicy::None,
                index: IndexPolicy {
                    bytes: 2048,
                    interval: None,
                },
                ..Default::default()
            },
            cut: ww_tape::CutPolicy {
                chunks: 4,
                ..Default::default()
            },
        },
    )
    .unwrap();
    for b in describe_blocks(Layout::UNALIGNED, chain.writer().unwrap().next_id()) {
        chain.write_block(&b).unwrap();
    }
    for k in 0..6 {
        for t in 0..2 {
            let (h, c) = chunk(t, k);
            chain.write_chunk(&h, &column(&c)).unwrap();
        }
    }
    chain
        .finish()
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect()
}

/// Two chains of one owner (an events chain and a series chain) have their own piece ids and session tags, so their
/// pieces read one after the other.
#[test]
fn chains_of_one_result_are_apart() {
    let (d0, d1) = (scratch("chain0"), scratch("chain1"));
    let (c0, c1) = (write_chain(&d0, 0), write_chain(&d1, 1));
    let (a, b) = (
        std::fs::read(&c0[0]).unwrap(),
        std::fs::read(&c1[0]).unwrap(),
    );
    let r = TapeReader::open(&a[..]).unwrap();
    let head = |bytes: &[u8]| {
        TapeReader::open(bytes).unwrap().pieces()[0]
            .head
            .clone()
            .unwrap()
            .0
    };
    let (h0, h1) = (head(&a), head(&b));
    assert_eq!(h0.ident.id, PieceId::first_of(&OwnerId([0x42; 32]), 0));
    assert_eq!(h1.ident.id, PieceId::first_of(&OwnerId([0x42; 32]), 1));
    assert_ne!(
        SessionTag::of_piece(&h0.ident.id),
        SessionTag::of_piece(&h1.ident.id)
    );
    assert!(r.verify(0).unwrap());
    let both = [a.clone(), b.clone()].concat();
    let mut rd = FileBlocks::new(&both[..]);
    let mut n = 0;
    while let Some(blk) = rd.next_block() {
        blk.unwrap();
        n += 1;
    }
    assert_eq!(n, walk(&a).len() + walk(&b).len());
    std::fs::remove_dir_all(&d0).unwrap();
    std::fs::remove_dir_all(&d1).unwrap();
}

#[test]
fn chain_of_pieces_links_both_ways() {
    let dir = scratch("chain");
    let paths = write_chain(&dir, 0);
    assert_eq!(paths.len(), 3);
    let again = scratch("chain_again");
    let paths2 = write_chain(&again, 0);
    for (a, b) in paths.iter().zip(&paths2) {
        assert_eq!(
            std::fs::read(a).unwrap(),
            std::fs::read(b).unwrap(),
            "same input, same bytes"
        );
    }
    std::fs::remove_dir_all(&again).unwrap();

    let files: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let mut summaries = vec![];
    for (i, f) in files.iter().enumerate() {
        let r = TapeReader::open(&f[..]).unwrap();
        assert_eq!(r.pieces().len(), 1, "piece {i} reads alone");
        assert!(r.verify(0).unwrap());
        let p = &r.pieces()[0];
        let (head, starts) = p.head.as_ref().unwrap();
        assert_eq!(head.ident.seq, i as u64);
        // every piece carries the describe blocks
        assert!(
            r.track_entries(0, 0)
                .unwrap()
                .iter()
                .any(|e| e.kind == BlockKind::Describe)
        );
        // items continue: the head's starts are where the previous piece's ranges ended
        let (_, ranges) = p.finish.as_ref().unwrap();
        for s in starts {
            let rg = ranges.iter().find(|r| r.track_id == s.track_id).unwrap();
            assert_eq!(rg.first_item, s.next_item);
            assert!(rg.first_tick >= s.from_tick);
        }
        // the next id comes from this id and the content hash in the finish, exactly
        let (finish, _) = p.finish.as_ref().unwrap();
        if i + 1 < files.len() {
            let next = PieceId::after(head.ident.id, &finish.hash);
            assert_eq!(finish.next, Some(next));
            assert_eq!(p.tail.as_ref().unwrap().next, Some(next));
        }
        summaries.push(p.summary().unwrap());
    }
    assert!(check_chain(&summaries).is_whole());

    // concatenated, the pieces are one tape: found by their tails, one small read each
    let all = files.concat();
    let r = TapeReader::open(&all[..]).unwrap();
    assert_eq!(r.pieces().len(), 3);
    assert!(r.pieces().iter().all(|p| p.found == Found::Tail));
    assert!(r.chain().is_whole());
    assert!((0..3).all(|i| r.verify(i).unwrap()));
    let t0: usize = (0..3)
        .map(|i| r.track_entries(i, 0x1000).unwrap().len())
        .sum();
    assert_eq!(t0, 6);
    // and a stream reader walks it without a cut
    let mut rd = FileBlocks::new(&all[..]);
    let mut n = 0;
    while let Some(b) = rd.next_block() {
        b.unwrap();
        n += 1;
    }
    assert!(n > 20);

    // the first piece deleted (retention): head cut, not damage
    let rest = files[1..].concat();
    let r = TapeReader::open(&rest[..]).unwrap();
    let c = r.chain();
    assert!(c.head_cut);
    assert_eq!(c.links, [ChainLink::Linked]);
    assert_eq!(c.end, ChainEnd::Complete);

    // the last piece lost its tail in a crash: its blocks are still found
    let mut cut = files.concat();
    cut.truncate(cut.len() - 300);
    let r = TapeReader::open(&cut[..]).unwrap();
    assert_eq!(r.pieces().len(), 3);
    assert_eq!(r.chain().end, ChainEnd::Open);
    assert!(
        r.entries(2)
            .unwrap()
            .iter()
            .any(|e| e.kind == BlockKind::Events)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// One file of finished pieces with an unfinished one after them (being written, or cut off by a crash): the
/// finished ones are found by their tails, the last one from its checkpoints and a scan, wherever it was cut.
#[test]
fn finished_pieces_then_an_unfinished_one() {
    let dir = scratch("then_open");
    let files: Vec<Vec<u8>> = write_chain(&dir, 0)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    let done = files[..2].concat();
    let last = &files[2];
    let blocks = walk(last);
    let (finish_at, _, _) = *blocks
        .iter()
        .find(|(_, _, k)| *k == BlockKind::Finish)
        .unwrap();
    // at every block boundary before the tail, and inside the padded finish
    let mut cuts: Vec<u64> = blocks.iter().map(|(at, _, _)| *at).filter(|at| *at > 0).collect();
    cuts.extend([finish_at + 200, last.len() as u64 - 100]);
    for cut in cuts {
        let all = [&done[..], &last[..cut as usize]].concat();
        let r = TapeReader::open(&all[..]).unwrap();
        let p = r.pieces();
        assert_eq!(p.len(), 3, "cut at {cut}");
        assert!(
            p[..2].iter().all(|p| p.found == Found::Tail && p.tail.is_some()),
            "cut at {cut}: {:?}",
            p.iter().map(|p| (p.start, p.found, p.end)).collect::<Vec<_>>()
        );
        assert_eq!(p[2].start, done.len() as u64, "cut at {cut}");
        assert!(p[2].tail.is_none(), "cut at {cut}");
        // cut inside its tail, the piece has its finish: it ended, and only the tail is lost
        let ended = cut > finish_at + 200;
        assert_eq!(p[2].finish.is_some(), ended, "cut at {cut}");
        let end = if ended { ChainEnd::Complete } else { ChainEnd::Open };
        assert_eq!(r.chain().end, end, "cut at {cut}");
        assert!((0..2).all(|i| r.verify(i).unwrap()));
        let events = r
            .entries(2)
            .unwrap()
            .iter()
            .filter(|e| e.kind == BlockKind::Events)
            .count();
        let written = blocks
            .iter()
            .filter(|(at, len, k)| *k == BlockKind::Events && at + u64::from(*len) <= cut)
            .count();
        assert_eq!(events, written, "cut at {cut}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The golden file: a finished two-piece chain, one block per line in hex (`WW_TAPE_GOLDEN=update` rewrites it).
#[test]
fn index_golden() {
    let dir = scratch("golden");
    let mut chain = PieceChain::create(
        &dir,
        OwnerId([7; 32]),
        0,
        &[],
        ww_tape::ChainOptions {
            writer: ww_tape::WriterOptions {
                sync: SyncPolicy::None,
                index: IndexPolicy {
                    bytes: 1000,
                    interval: None,
                },
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    for b in describe_blocks(Layout::UNALIGNED, chain.writer().unwrap().next_id()) {
        chain.write_block(&b).unwrap();
    }
    for k in 0..2 {
        let (h, c) = chunk(0, k);
        chain.write_chunk(&h, &column(&c)).unwrap();
        let id = chain.writer().unwrap().next_id();
        chain.write_block(&records(id, k as i64)).unwrap();
    }
    chain.cut_now().unwrap();
    let (h, c) = chunk(0, 2);
    chain.write_chunk(&h, &column(&c)).unwrap();
    let paths: Vec<PathBuf> = chain
        .finish()
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    let mut hex = String::new();
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        let mut rd = FileBlocks::new(&bytes[..]);
        while let Some(b) = rd.next_block() {
            for byte in b.unwrap().bytes() {
                hex.push_str(&format!("{byte:02x}"));
            }
            hex.push('\n');
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-index.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-index.hex: run with WW_TAPE_GOLDEN=update");
    assert!(
        hex == want,
        "the format changed: bytes differ from golden/v2-index.hex"
    );
}

#[test]
fn every_block_of_a_chain_carries_its_session() {
    let dir = scratch("sessions");
    let paths = write_chain(&dir, 0);
    assert!(paths.len() > 1, "the chain was cut");
    let session = SessionTag::of_piece(&PieceId::first(&OwnerId([0x42; 32])));
    assert!(session.is_session());
    let mut seqs = vec![];
    for (i, p) in paths.iter().enumerate() {
        let bytes = std::fs::read(p).unwrap();
        let r = TapeReader::open(&bytes[..]).unwrap();
        let (head, _) = r.pieces()[0].head.clone().unwrap();
        // a cut names the session going on; the first piece follows nothing
        let before = if i == 0 { SessionTag::NONE } else { session };
        assert_eq!(head.prev_session, before);
        let mut rd = FileBlocks::new(&bytes[..]);
        while let Some(b) = rd.next_block() {
            let b = b.unwrap();
            assert_eq!(
                b.session,
                session,
                "{:?} at {} of {}",
                b.kind,
                rd.offset(),
                p.display()
            );
            if listed(b.kind) {
                seqs.push(b.seq);
            }
        }
    }
    // the describe blocks a cut writes again repeat the seq before them; data blocks count on across pieces
    seqs.dedup();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The describe blocks a writer writes again (a cut's next piece, the backup before each finish) are marked copies;
/// the producer's own are not.
#[test]
fn a_describe_copy_is_marked() {
    let dir = scratch("copies");
    let paths = write_chain(&dir, 0);
    let mut marks = vec![];
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        let mut rd = FileBlocks::new(&bytes[..]);
        let mut piece = vec![];
        while let Some(b) = rd.next_block() {
            let b = b.unwrap();
            if b.kind == BlockKind::Describe {
                piece.push(b.is_copy());
            }
        }
        marks.push(piece);
    }
    // the first piece: the producer's describe block, the copy after the first data block, the copy after the first
    // checkpoint, the backup; each later one: the copy at its start, then the same three
    assert_eq!(marks[0], [false, true, true, true], "{marks:?}");
    assert!(
        marks[1..].iter().all(|m| m == &[true, true, true, true]),
        "{marks:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Track 0's chunk `k` of 8192 items: 64 KiB of raw starts.
fn big_chunk(k: u64) -> (ChunkHead, Vec<u8>) {
    let n = 8192u64;
    let first = k * n;
    let starts: Vec<u8> = (first..first + n)
        .flat_map(|i| (i as i64).to_le_bytes())
        .collect();
    let head = ChunkHead {
        kind: BlockKind::Events,
        track_id: 0x1000,
        track_number: 1,
        first_item: first,
        count: n as u32,
        first_tick: first as i64,
        last_tick: (first + n - 1) as i64,
        latest_end: (first + n - 1) as i64,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    (head, starts)
}

/// A piece with a damaged head describe block still has its describe set: the newest copy, the one the writer placed
/// after the first data block when there is no other, found past the scan's stop in an unfinished 1 MiB piece (no checkpoint yet), through the checkpoints
/// of an unfinished 10 MiB piece and through the final index of a finished one.
#[test]
fn a_damaged_head_describe_set_reads_from_the_next_copy() {
    for (chunks, finished) in [(16, false), (160, false), (160, true)] {
        let dir = scratch(&format!("damaged_describe_{chunks}_{finished}"));
        let path = dir.join("p.wwt");
        let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
        let owner = OwnerId([7; 32]);
        let head = ww_tape::PieceHead {
            ident: ww_tape::PieceIdent {
                id: PieceId::first(&owner),
                owner,
                seq: 0,
            },
            prev: None,
            prev_session: SessionTag::NONE,
        };
        w.begin_piece(&head, &[]).unwrap();
        let originals = describe_blocks(Layout::UNALIGNED, w.next_id());
        for b in &originals {
            w.write_block(b).unwrap();
        }
        for k in 0..chunks {
            let (h, c) = big_chunk(k);
            w.write_chunk(&h, &column(&c)).unwrap();
        }
        let bytes = if finished {
            let f = w.finish().unwrap();
            std::fs::read(f.path).unwrap()
        } else {
            drop(w);
            std::fs::read(dir.join("p.wwt.partial")).unwrap()
        };
        assert!(bytes.len() as u64 > chunks << 16, "{} bytes", bytes.len());
        let describe: Vec<(u64, bool)> = walk(&bytes)
            .into_iter()
            .filter(|b| b.2 == BlockKind::Describe)
            .map(|b| {
                (
                    b.0,
                    ww_tape::Block::read(&bytes[b.0 as usize..])
                        .unwrap()
                        .is_copy(),
                )
            })
            .collect();
        // the head's set, the copy after the first chunk, the copy after the first checkpoint (about 4 MiB in) of a
        // piece past it, and the backup of a finished piece
        let want = 2 + usize::from(chunks > 64) + usize::from(finished);
        assert_eq!(describe.len(), want, "{describe:?}");
        assert!(!describe[0].1 && describe[1..].iter().all(|d| d.1));
        assert!(describe[1].0 < 128 << 10, "{describe:?}");
        if chunks > 64 {
            assert!((4 << 20..5 << 20).contains(&describe[2].0), "{describe:?}");
        }

        let mut damaged = bytes.clone();
        damaged[describe[0].0 as usize + 40] ^= 1;
        for (src, copy) in [(&bytes, false), (&damaged, true)] {
            let rd = TapeReader::open(&src[..]).unwrap();
            assert_eq!(rd.pieces().len(), 1);
            if chunks == 16 && copy {
                // no checkpoint yet: the scan from the start stops at the damaged head set
                assert_eq!(rd.pieces()[0].cut, Some(describe[0].0));
            }
            // the newest whole set: the last copy, also when the head's set reads
            let set = rd.describe_set(0).unwrap();
            assert_eq!(set.blocks.len(), originals.len());
            assert!(set.lost.is_empty(), "{:?}", set.lost);
            assert_eq!(
                set.base,
                describe.last().map(|d| d.0),
                "{chunks} chunks, finished {finished}, damaged {copy}"
            );
            for (got, want) in set.bytes().zip(&originals) {
                let got = ww_tape::Block::read(got).unwrap();
                let want = ww_tape::Block::read(want).unwrap();
                assert!(got.is_copy());
                assert_eq!(
                    got.records().collect::<Vec<_>>(),
                    want.records().collect::<Vec<_>>()
                );
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// The describe repeat set on a writer carries over to the next piece of a cut.
#[test]
fn the_describe_repeat_carries_over_a_cut() {
    let dir = scratch("repeat_cut");
    let mut w = TapeWriter::create(&dir.join("a.wwt"), SyncPolicy::None).unwrap();
    w.set_index_policy(IndexPolicy::NONE);
    w.set_describe_repeat(64 << 10);
    let owner = OwnerId([8; 32]);
    let head = ww_tape::PieceHead {
        ident: ww_tape::PieceIdent {
            id: PieceId::first(&owner),
            owner,
            seq: 0,
        },
        prev: None,
        prev_session: SessionTag::NONE,
    };
    w.begin_piece(&head, &[]).unwrap();
    for b in describe_blocks(Layout::UNALIGNED, w.next_id()) {
        w.write_block(&b).unwrap();
    }
    let (_, mut next) = w.cut(&dir.join("b.wwt")).unwrap();
    for k in 0..4 {
        let (h, c) = big_chunk(k);
        next.write_chunk(&h, &column(&c)).unwrap();
    }
    let f = next.finish().unwrap();
    let bytes = std::fs::read(f.path).unwrap();
    let copies = walk(&bytes)
        .iter()
        .filter(|b| b.2 == BlockKind::Describe)
        .count();
    // the copy at its start, a repeat after each chunk of over 64 KiB, the backup; 2 with the default repeat
    assert_eq!(copies, 6);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A block written again with other bytes (the order a writer makes: records, the describe copy, the records again)
/// takes the place of the first in the index as in a walk, whether the piece is read by its final index or scanned.
#[test]
fn a_replaced_block_leaves_the_index() {
    let dir = scratch("replaced");
    let path = dir.join("r.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    for b in describe_blocks(Layout::UNALIGNED, w.next_id()) {
        w.write_block(&b).unwrap();
    }
    let id = w.next_id();
    w.write_block(&records(id, 1)).unwrap();
    w.write_block(&records(id, 2)).unwrap();
    // the same bytes again: dropped, never listed
    w.write_block(&records(id, 2)).unwrap();
    for n in 3..6 {
        w.write_block(&records(w.next_id(), n)).unwrap();
    }
    let partial = std::fs::read(w.current_path()).unwrap();
    w.finish().unwrap();
    let finished = std::fs::read(&path).unwrap();
    for bytes in [partial, finished] {
        // the blocks a walk ends up with: a replacement takes the place of the last counted block
        let mut rd = FileBlocks::new(&bytes[..]);
        let (mut walked, mut last, mut replaced, mut seqs) = (vec![], None, 0, vec![]);
        while let Some(b) = rd.next_block() {
            match b {
                Ok(b) if listed(b.kind) => {
                    if b.kind == BlockKind::Records {
                        seqs.push(b.seq);
                    }
                    if !b.is_copy() {
                        last = Some(walked.len());
                    }
                    walked.push(rd.offset());
                }
                Ok(_) => {}
                Err(ww_tape::FileError::Replaced { .. }) => {
                    walked.remove(last.take().unwrap());
                    seqs.pop();
                    replaced += 1;
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(replaced, 1);
        let r = TapeReader::open(&bytes[..]).unwrap();
        let listed: Vec<u64> = r.entries(0).unwrap().iter().map(|e| e.offset).collect();
        assert_eq!(listed, walked);
        assert_eq!(seqs, [id.seq, id.seq + 1, id.seq + 2, id.seq + 3]);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every describe set in `bytes` a walk reads, as the offsets of its blocks: a set opens at a block marked first and
/// closes at one marked last; a describe block outside such a run is listed alone.
fn describe_runs(bytes: &[u8]) -> Vec<Vec<(u64, bool)>> {
    let mut runs: Vec<Vec<(u64, bool)>> = vec![];
    let mut open = false;
    for (at, _, kind) in walk(bytes) {
        if kind != BlockKind::Describe {
            open = false;
            continue;
        }
        let b = ww_tape::Block::read(&bytes[at as usize..]).unwrap();
        if b.flags & ww_tape::FLAG_SET_FIRST != 0 || !open {
            runs.push(vec![]);
        }
        runs.last_mut().unwrap().push((at, b.is_copy()));
        open = b.flags & ww_tape::FLAG_SET_LAST == 0;
    }
    runs
}

/// A describe set of two blocks written after the first records: the writer copies it only once its last block is
/// in, so every copy, the backup and the next piece's head carry both blocks.
#[test]
fn copies_carry_the_whole_describe_set() {
    let dir = scratch("whole_set");
    let mut w = TapeWriter::create(&dir.join("a.wwt"), SyncPolicy::None).unwrap();
    w.set_index_policy(IndexPolicy::NONE);
    w.set_describe_repeat(4 << 10);
    let owner = OwnerId([9; 32]);
    let head = ww_tape::PieceHead {
        ident: ww_tape::PieceIdent {
            id: PieceId::first(&owner),
            owner,
            seq: 0,
        },
        prev: None,
        prev_session: SessionTag::NONE,
    };
    w.begin_piece(&head, &[]).unwrap();
    w.write_block(&records(w.next_id(), 0)).unwrap();
    let set = describe_blocks_max(Layout::UNALIGNED, w.next_id(), 64);
    assert_eq!(set.len(), 2);
    for b in &set {
        w.write_block(b).unwrap();
    }
    for n in 1..100 {
        w.write_block(&records(w.next_id(), n)).unwrap();
    }
    let (done, next) = w.cut(&dir.join("b.wwt")).unwrap();
    let finished = next.finish().unwrap();
    for path in [done.path, finished.path] {
        let bytes = std::fs::read(&path).unwrap();
        let runs = describe_runs(&bytes);
        assert!(runs.len() >= 2, "{runs:?}");
        for run in &runs {
            assert_eq!(run.len(), 2, "{path:?}: {runs:?}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A head describe set of two blocks, either damaged, in a scanned piece: the set read is the copy, both blocks,
/// never the head's good block alone.
#[test]
fn part_of_a_describe_set_is_not_the_set() {
    let dir = scratch("part_set");
    let path = dir.join("p.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    w.set_index_policy(IndexPolicy::NONE);
    let originals = describe_blocks_max(Layout::UNALIGNED, w.next_id(), 64);
    assert_eq!(originals.len(), 2);
    for b in &originals {
        w.write_block(b).unwrap();
    }
    for n in 0..20 {
        w.write_block(&records(w.next_id(), n)).unwrap();
    }
    drop(w);
    let bytes = std::fs::read(dir.join("p.wwt.partial")).unwrap();
    let runs = describe_runs(&bytes);
    assert_eq!(runs.len(), 2, "the head's set and the copy: {runs:?}");
    let copy_at = runs[1][0].0;
    for k in 0..2 {
        let mut damaged = bytes.clone();
        damaged[runs[0][k].0 as usize + 40] ^= 1;
        let rd = TapeReader::open(&damaged[..]).unwrap();
        assert_eq!(rd.pieces()[0].cut, Some(runs[0][k].0));
        let set = rd.describe_set(0).unwrap();
        let at: Vec<u64> = set.blocks.iter().map(|(o, _)| *o).collect();
        assert_eq!(
            at,
            runs[1].iter().map(|r| r.0).collect::<Vec<_>>(),
            "block {k} damaged"
        );
        assert_eq!(at[0], copy_at);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
