//! Index, finish, tail, piece chains and the positional reader, through the public API.

use std::path::{Path, PathBuf};

use ww_tape::chunk::ChunkHead;
use ww_tape::index::IndexPolicy;
use ww_tape::piece::{ChainEnd, ChainLink, PieceId, check_chain};
use ww_tape::reader::{Found, OpenOptions};
use ww_tape::{
    BlockId, BlockKind, BlockWriter, ClockTime, Codec, ColumnKind, ColumnSpec, Describe,
    DescribeSet, EndMode, FileBlocks, FinishState, Layout, PieceChain, Producer, Recovery,
    ResultId, RunTag, SyncPolicy, TapeReader, TapeWriter,
};

/// A fresh scratch directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww_tape_pieces_{name}_{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Describe blocks numbered from `first` on.
fn describe_blocks(layout: Layout, first: BlockId) -> Vec<Vec<u8>> {
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "pieces" }))
        .unwrap();
    set.insert(&Describe::Layout(layout)).unwrap();
    let mut seq = first;
    let mut blocks = vec![];
    set.write_blocks(&mut seq, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    blocks
}

fn records(id: BlockId, n: i64) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]);
    b.push(9, ClockTime::new(1, n), &[&[n as u8; 30]]).unwrap();
    let len = b.finish();
    buf[..len].to_vec()
}

/// Track `t`'s chunk `k`: 50 items with raw starts.
fn chunk(t: u64, k: u64) -> (ChunkHead, Vec<u8>) {
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

fn column(bytes: &[u8]) -> [(ColumnSpec, &[u8]); 1] {
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
        3,
        "first, the copy after the first checkpoint, and backup"
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
    w.set_alignment(ww_tape::ALIGNMENT);
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

/// Write chain `n` of a result, pieces cut every 4 chunks; the piece files in order.
fn write_chain(dir: &Path, n: u64) -> Vec<PathBuf> {
    let mut chain = PieceChain::create(
        dir,
        ResultId([0x42; 32]),
        n,
        &[],
        ww_tape::ChainOptions {
            sync: SyncPolicy::None,
            index: IndexPolicy {
                bytes: 2048,
                interval: None,
            },
            cut: ww_tape::CutPolicy {
                chunks: 4,
                ..Default::default()
            },
            alignment: 0,
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

/// Two chains of one result (an events chain and a series chain) have their own piece ids and run tags, so their
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
    assert_eq!(h0.ident.id, PieceId::first_of(&ResultId([0x42; 32]), 0));
    assert_eq!(h1.ident.id, PieceId::first_of(&ResultId([0x42; 32]), 1));
    assert_ne!(
        RunTag::of_piece(&h0.ident.id),
        RunTag::of_piece(&h1.ident.id)
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

/// The golden file: a finished two-piece chain, one block per line in hex (`WW_TAPE_GOLDEN=update` rewrites it).
#[test]
fn index_golden() {
    let dir = scratch("golden");
    let mut chain = PieceChain::create(
        &dir,
        ResultId([7; 32]),
        0,
        &[],
        ww_tape::ChainOptions {
            sync: SyncPolicy::None,
            index: IndexPolicy {
                bytes: 1000,
                interval: None,
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
fn every_block_of_a_chain_carries_its_run() {
    let dir = scratch("runs");
    let paths = write_chain(&dir, 0);
    assert!(paths.len() > 1, "the chain was cut");
    let run = RunTag::of_piece(&PieceId::first(&ResultId([0x42; 32])));
    assert!(run.is_run());
    let mut seqs = vec![];
    for (i, p) in paths.iter().enumerate() {
        let bytes = std::fs::read(p).unwrap();
        let r = TapeReader::open(&bytes[..]).unwrap();
        let (head, _) = r.pieces()[0].head.clone().unwrap();
        // a cut names the run going on; the first piece follows nothing
        let before = if i == 0 { RunTag::NONE } else { run };
        assert_eq!(head.prev_run, before);
        let mut rd = FileBlocks::new(&bytes[..]);
        while let Some(b) = rd.next_block() {
            let b = b.unwrap();
            assert_eq!(
                b.run,
                run,
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
    // the first piece: the producer's describe block, the copy after the first checkpoint, the backup; each later
    // one: the copy at its start, the copy after its first checkpoint, the backup
    assert_eq!(marks[0], [false, true, true]);
    assert!(
        marks[1..].iter().all(|m| m == &[true, true, true]),
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

/// A piece with a damaged head describe block still has its describe set: the copy the writer placed after the first
/// data block, found past the scan's stop in an unfinished 1 MiB piece (no checkpoint yet), through the checkpoints
/// of an unfinished 10 MiB piece and through the final index of a finished one.
#[test]
fn a_damaged_head_describe_set_reads_from_the_next_copy() {
    for (chunks, finished) in [(16, false), (160, false), (160, true)] {
        let dir = scratch(&format!("damaged_describe_{chunks}_{finished}"));
        let path = dir.join("p.wwt");
        let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
        let result = ResultId([7; 32]);
        let head = ww_tape::PieceHead {
            ident: ww_tape::PieceIdent {
                id: PieceId::first(&result),
                result,
                seq: 0,
            },
            prev: None,
            prev_run: RunTag::NONE,
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
        // the head's set, the copy after the first chunk, and the backup of a finished piece
        let want = if finished { 3 } else { 2 };
        assert_eq!(describe.len(), want, "{describe:?}");
        assert!(!describe[0].1 && describe[1..].iter().all(|d| d.1));
        assert!(describe[1].0 < 128 << 10, "{describe:?}");

        let mut damaged = bytes.clone();
        damaged[describe[0].0 as usize + 40] ^= 1;
        for (src, copy) in [(&bytes, false), (&damaged, true)] {
            let rd = TapeReader::open(&src[..]).unwrap();
            assert_eq!(rd.pieces().len(), 1);
            if chunks == 16 && copy {
                // no checkpoint yet: the scan from the start stops at the damaged head set
                assert_eq!(rd.pieces()[0].cut, Some(describe[0].0));
            }
            let set = rd.describe_set(0).unwrap();
            assert_eq!(set.len(), originals.len());
            let at = if copy { describe[1].0 } else { describe[0].0 };
            assert_eq!(
                set[0].0, at,
                "{chunks} chunks, finished {finished}, damaged {copy}"
            );
            for ((_, got), want) in set.iter().zip(&originals) {
                let got = ww_tape::Block::read(got).unwrap();
                let want = ww_tape::Block::read(want).unwrap();
                assert_eq!(got.is_copy(), copy);
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
    let result = ResultId([8; 32]);
    let head = ww_tape::PieceHead {
        ident: ww_tape::PieceIdent {
            id: PieceId::first(&result),
            result,
            seq: 0,
        },
        prev: None,
        prev_run: RunTag::NONE,
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
