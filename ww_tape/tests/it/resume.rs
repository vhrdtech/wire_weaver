//! Resume after a crash, memory sinks and PieceChain on any sink, through the public API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::id;
use crate::pieces::{chunk, column, describe_blocks_max, records, scratch};
use ww_tape::index::IndexPolicy;
use ww_tape::piece::{PieceId, piece_name};
use ww_tape::{
    ALIGNMENT, BlockKind, ChainOptions, CutPolicy, FileBlocks, Files, Layout, MemSink, MemTapes,
    PieceChain, PieceHead, PieceIdent, Recovery, ResultId, RunTag, SinkFactory, SyncPolicy,
    TapeReader, TapeSink, TapeWriter, WriterOptions,
};

/// One call a test makes on a writer; each places one counted block.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// Block `i` of the describe set.
    Describe(usize),
    /// A records block with tick `n`.
    Records(i64),
    /// A records block with tick `n`, deferred.
    Defer(i64),
    /// Track `t`'s chunk `k`.
    Chunk(u64, u64),
}

/// The calls of a recording: a describe set of three blocks, then rounds of two chunks, a records block and a
/// deferred one; `chunk_rounds` rounds.
fn ops(rounds: u64) -> Vec<Op> {
    let mut out: Vec<Op> = (0..describe().len()).map(Op::Describe).collect();
    for k in 0..rounds {
        out.extend([
            Op::Chunk(0, k),
            Op::Records(k as i64),
            Op::Chunk(1, k),
            Op::Defer(1000 + k as i64),
        ]);
    }
    out
}

/// The describe set of the tests, in three blocks.
fn describe() -> Vec<Vec<u8>> {
    let set = describe_blocks_max(Layout::UNALIGNED, id(0), 60);
    assert!(set.len() >= 2, "a set of several blocks");
    set
}

/// Make the call `op` on `w`.
fn apply<S: TapeSink>(w: &mut TapeWriter<S>, op: Op) {
    match op {
        Op::Describe(i) => {
            let mut b = describe()[i].clone();
            ww_tape::set_id(&mut b, w.next_id()).unwrap();
            w.write_block(&b).unwrap();
        }
        Op::Records(n) => w.write_block(&records(w.next_id(), n)).unwrap(),
        Op::Defer(n) => w.defer_block(records(id(0), n)).unwrap(),
        Op::Chunk(t, k) => {
            let (h, c) = chunk(t, k);
            w.write_chunk(&h, &column(&c)).unwrap();
        }
    }
}

/// The options of the tests: checkpoints every 1 KiB, a describe repeat every 6 KiB, so a piece holds every kind of
/// block a writer adds.
fn options(alignment: u32) -> WriterOptions {
    WriterOptions {
        sync: SyncPolicy::None,
        index: IndexPolicy {
            bytes: 1024,
            interval: None,
        },
        alignment,
        describe_repeat: 6 << 10,
    }
}

fn head() -> PieceHead {
    let result = ResultId([3; 32]);
    PieceHead {
        ident: PieceIdent {
            id: PieceId::first(&result),
            result,
            seq: 0,
        },
        prev: None,
        prev_run: RunTag::NONE,
    }
}

/// A first piece of `ops` on a memory sink with `options`, begun but given no call yet.
fn begin(alignment: u32) -> TapeWriter<MemSink> {
    let mut w = TapeWriter::create_on(MemSink::new(Path::new("p")), SyncPolicy::None);
    let o = options(alignment);
    w.set_alignment(o.alignment).unwrap();
    w.set_index_policy(o.index);
    w.set_describe_repeat(o.describe_repeat);
    w.begin_piece(&head(), &[]).unwrap();
    w
}

/// The seq of each record block by its tick in `bytes`, the first records of each.
fn record_seqs(bytes: &[u8]) -> BTreeMap<i64, u64> {
    let mut rd = FileBlocks::with_recovery(bytes, Recovery::Prefix);
    let mut out = BTreeMap::new();
    while let Some(Ok(b)) = rd.next_block() {
        if b.kind == BlockKind::Records && !b.is_copy() {
            let at = b.records().next().unwrap().unwrap().at.unwrap();
            out.insert(at.ticks, b.seq);
        }
    }
    out
}

/// The piece of `ops` written without a crash, and the seq of the block each call placed.
fn reference(alignment: u32, ops: &[Op]) -> (Vec<u8>, Vec<u64>) {
    let mut w = begin(alignment);
    let mut seqs = vec![];
    for op in ops {
        apply(&mut w, *op);
        seqs.push(w.next_seq().wrapping_sub(1));
    }
    let tape = w.sink().tape();
    w.finish().unwrap();
    let bytes = tape.to_vec().unwrap();
    // a deferred block takes its seq where it is placed
    let placed = record_seqs(&bytes);
    for (op, seq) in ops.iter().zip(&mut seqs) {
        if let Op::Defer(n) = op {
            *seq = placed[n];
        }
    }
    (bytes, seqs)
}

/// The ends of the blocks of `bytes` and some places in between, ascending: where the tests cut a piece.
fn cut_points(bytes: &[u8], extra: usize) -> Vec<usize> {
    let mut rd = FileBlocks::with_recovery(bytes, Recovery::Prefix);
    let mut out = vec![0];
    while let Some(Ok(_)) = rd.next_block() {
        out.push(rd.offset() as usize);
    }
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..extra {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push((x % bytes.len() as u64) as usize);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Give a writer resumed after a crash the calls whose blocks it doesn't hold, then finish it; its bytes.
fn finish_resumed(mut w: TapeWriter<MemSink>, ops: &[Op], seqs: &[u64]) -> Vec<u8> {
    let next = w.next_seq();
    for (op, seq) in ops.iter().zip(seqs) {
        if *seq >= next {
            apply(&mut w, *op);
        }
    }
    let tape = w.sink().tape();
    w.finish().unwrap();
    tape.to_vec().unwrap()
}

/// A piece cut at every block boundary and at bytes in between resumes, takes the calls it lost and ends with the
/// bytes, and so the content hash, of the piece written without the crash: with deferred blocks, checkpoints,
/// describe copies and repeats, and cuts inside the end of the piece.
fn cut_anywhere(alignment: u32) {
    let ops = ops(8);
    let (want, seqs) = reference(alignment, &ops);
    let r = TapeReader::open(&want[..]).unwrap();
    let entries = r.entries(0).unwrap();
    let copies = (r.track_entries(0, 0).unwrap().iter())
        .filter(|e| e.kind == BlockKind::Describe)
        .count();
    assert!(
        copies >= 5,
        "head, early, checkpoint, repeat, backup: {copies}"
    );
    assert!(entries.len() >= 40, "{} blocks", entries.len());
    let mut ended = 0;
    for cut in cut_points(&want, 40) {
        let mut sink = MemSink::new(Path::new("p"));
        sink.write_all(&want[..cut]).unwrap();
        let (mut w, resumed) = TapeWriter::resume_on(sink, options(alignment)).unwrap();
        assert!(resumed.kept_bytes <= cut as u64);
        assert_eq!(resumed.kept_bytes + resumed.dropped_bytes, cut as u64);
        ended += usize::from(resumed.ended.is_some());
        if w.bytes() == 0 {
            // cut inside the head: the piece starts again
            w.begin_piece(&head(), &[]).unwrap();
        }
        let got = finish_resumed(w, &ops, &seqs);
        assert!(
            got == want,
            "alignment {alignment}, cut at {cut} of {}",
            want.len()
        );
    }
    assert!(ended > 0, "cuts after the finish");
}

#[test]
fn a_piece_cut_anywhere_resumes_to_the_same_bytes() {
    cut_anywhere(0);
}

#[test]
fn an_aligned_piece_cut_anywhere_resumes_to_the_same_bytes() {
    cut_anywhere(ALIGNMENT);
}

/// [TapeWriter::resume] reopens `NAME.partial`, keeps the blocks up to the last counted one and reports them.
#[test]
fn resume_reopens_the_partial_file() {
    let dir = scratch("resume_file");
    let path = dir.join("p.wwt");
    let ops = ops(6);
    let (want, seqs) = reference(0, &ops);
    let cut = want.len() * 2 / 3;
    std::fs::write(ww_tape::writer::partial_name(&path), &want[..cut]).unwrap();
    let (mut w, resumed) = TapeWriter::resume(&path, options(0)).unwrap();
    assert!(resumed.kept_bytes <= cut as u64);
    assert!(
        resumed
            .entries
            .iter()
            .all(|e| e.offset < resumed.kept_bytes)
    );
    assert!(resumed.entries.iter().any(|e| e.kind == BlockKind::Events));
    let next = w.next_seq();
    for (op, seq) in ops.iter().zip(&seqs) {
        if *seq >= next {
            apply(&mut w, *op);
        }
    }
    let f = w.finish().unwrap();
    assert_eq!(std::fs::read(&f.path).unwrap(), want);
    // finished: nothing to resume
    assert!(matches!(
        TapeWriter::resume(&path, options(0)),
        Err(ww_tape::WriteError::Exists(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A resumed piece whose head describe set is damaged takes its describe set from the next whole copy: the copies
/// it writes after the resume are the ones the undamaged piece has, and it ends whole, its content hash over the
/// damaged bytes.
#[test]
fn a_damaged_head_describe_set_resumes_from_the_next_copy() {
    let ops = ops(12);
    let (want, seqs) = reference(0, &ops);
    let r = TapeReader::open(&want[..]).unwrap();
    let describe: Vec<_> = (r.track_entries(0, 0).unwrap().into_iter())
        .filter(|e| e.kind == BlockKind::Describe)
        .collect();
    let finish_at = r.pieces()[0].finish_offset.unwrap() as usize;
    // past the first checkpoint, with copies after the cut still to come
    let cut = want.len() / 2;
    assert!(describe.iter().any(|e| e.offset as usize > cut));
    let flipped = describe[0].offset as usize + 40;
    let mut sink = MemSink::new(Path::new("p"));
    let mut damaged = want[..cut].to_vec();
    damaged[flipped] ^= 1;
    sink.write_all(&damaged).unwrap();
    let (w, resumed) = TapeWriter::resume_on(sink, options(0)).unwrap();
    assert!(resumed.kept_bytes > describe[2].offset);
    let got = finish_resumed(w, &ops, &seqs);
    assert_eq!(got.len(), want.len());
    let differ: Vec<usize> = (0..want.len()).filter(|&i| got[i] != want[i]).collect();
    assert_eq!(differ[0], flipped);
    assert!(
        differ[1..].iter().all(|&i| i >= finish_at),
        "only the content hash differs past the damage: {differ:?}"
    );
    let r = TapeReader::open(&got[..]).unwrap();
    assert!(r.verify(0).unwrap());
    let set = r.describe_set(0).unwrap();
    assert!(set[0].0 > describe[0].offset, "the set comes from a copy");
}

/// A chain written to memory reads through [TapeReader] while it is written and once finished, and its pieces are the
/// bytes of the same chain written to files.
#[test]
fn a_chain_in_memory_reads_and_matches_files() {
    let dir = scratch("mem_chain");
    let opts = ChainOptions {
        writer: options(0),
        cut: CutPolicy {
            chunks: 5,
            ..Default::default()
        },
    };
    let result = ResultId([5; 32]);
    let mem = MemTapes::new();
    let mut in_mem =
        PieceChain::create_on(mem.clone(), Path::new("mem"), result, 1, &[], opts).unwrap();
    let mut on_disk = PieceChain::create(&dir, result, 1, &[], opts).unwrap();
    for op in ops(10) {
        apply_chain(&mut in_mem, op);
        apply_chain(&mut on_disk, op);
    }
    // the open piece reads while it is written
    let live = mem
        .get(&Path::new("mem").join(piece_name(in_mem.seq())))
        .unwrap()
        .unwrap();
    let r = TapeReader::open(live.clone()).unwrap();
    assert!(!r.pieces().is_empty());
    assert!(
        r.entries(0)
            .unwrap()
            .iter()
            .any(|e| e.kind == BlockKind::Events)
    );
    let a = in_mem.finish().unwrap();
    let b = on_disk.finish().unwrap();
    assert!(a.len() >= 3, "{} pieces", a.len());
    assert_eq!(a.len(), b.len());
    let mut all = vec![];
    for (fa, fb) in a.iter().zip(&b) {
        let tape = mem.get(&fa.path).unwrap().unwrap();
        assert!(tape.is_finished());
        let bytes = tape.to_vec().unwrap();
        assert_eq!(bytes, std::fs::read(&fb.path).unwrap());
        assert_eq!(fa.hash, fb.hash);
        let r = TapeReader::open(tape).unwrap();
        assert!(r.verify(0).unwrap());
        all.extend(bytes);
    }
    let r = TapeReader::open(&all[..]).unwrap();
    assert!(r.chain().is_whole(), "{:?}", r.chain());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The calls of a chain: each chunk through [PieceChain::write_chunk], so cuts come before chunks.
fn apply_chain<F: SinkFactory>(c: &mut PieceChain<F>, op: Op) {
    match op {
        Op::Chunk(t, k) => {
            let (h, b) = chunk(t, k);
            c.write_chunk(&h, &column(&b)).unwrap();
        }
        _ => apply(c.writer().unwrap(), op),
    }
}

fn chain_options() -> ChainOptions {
    ChainOptions {
        writer: options(0),
        cut: CutPolicy {
            chunks: 6,
            ..Default::default()
        },
    }
}

/// The pieces of a chain of `ops` written to `dir` without a crash, and the seq of each call's block.
fn reference_chain(dir: &Path, ops: &[Op]) -> (Vec<PathBuf>, Vec<u64>) {
    let mut c = PieceChain::create(dir, ResultId([9; 32]), 0, &[], chain_options()).unwrap();
    let mut seqs = vec![];
    for op in ops {
        apply_chain(&mut c, *op);
        seqs.push(c.writer().unwrap().next_seq().wrapping_sub(1));
    }
    let paths: Vec<PathBuf> = c.finish().unwrap().into_iter().map(|f| f.path).collect();
    let mut placed = BTreeMap::new();
    for p in &paths {
        placed.extend(record_seqs(&std::fs::read(p).unwrap()));
    }
    for (op, seq) in ops.iter().zip(&mut seqs) {
        if let Op::Defer(n) = op {
            *seq = placed[n];
        }
    }
    (paths, seqs)
}

/// A chain cut off anywhere in any piece, or between a cut's finish and the next piece, resumes and ends with the
/// pieces of the chain written without the crash; the pieces finished before are read back.
#[test]
fn a_chain_cut_anywhere_resumes_to_the_same_pieces() {
    let dir = scratch("chain_resume");
    let want_dir = dir.join("want");
    std::fs::create_dir(&want_dir).unwrap();
    let ops = ops(14);
    let (paths, seqs) = reference_chain(&want_dir, &ops);
    assert!(paths.len() >= 4, "{} pieces", paths.len());
    let want: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let mut runs = 0;
    for (i, piece) in want.iter().enumerate() {
        // a crash between the cut's finish and the next piece: no file of piece i yet
        let mut cuts: Vec<Option<usize>> = vec![None];
        cuts.extend(cut_points(piece, 20).into_iter().map(Some));
        for cut in cuts {
            if cut.is_none() && i == 0 {
                continue;
            }
            let d = dir.join(format!("run{runs}"));
            runs += 1;
            std::fs::create_dir(&d).unwrap();
            for (j, w) in want.iter().enumerate().take(i) {
                std::fs::write(d.join(piece_name(j as u64)), w).unwrap();
            }
            if let Some(cut) = cut {
                let partial = ww_tape::writer::partial_name(&d.join(piece_name(i as u64)));
                std::fs::write(partial, &piece[..cut]).unwrap();
            }
            let (mut c, _) = PieceChain::resume(&d, ResultId([9; 32]), 0, chain_options())
                .unwrap_or_else(|e| panic!("piece {i}, cut {cut:?}: {e}"));
            assert_eq!(c.finished().len(), i);
            let next = c.writer().unwrap().next_seq();
            for (op, seq) in ops.iter().zip(&seqs) {
                if *seq >= next {
                    apply_chain(&mut c, *op);
                }
            }
            let got = c.finish().unwrap();
            assert_eq!(got.len(), want.len(), "piece {i}, cut {cut:?}");
            for (j, w) in want.iter().enumerate() {
                let g = std::fs::read(d.join(piece_name(j as u64))).unwrap();
                let first = g.iter().zip(w).position(|(a, b)| a != b);
                assert!(
                    &g == w,
                    "piece {i}, cut {cut:?}: piece {j} differs at {first:?}, {} vs {} bytes",
                    g.len(),
                    w.len()
                );
            }
            std::fs::remove_dir_all(&d).unwrap();
        }
    }
    // a chain whose last piece is complete takes no more
    assert!(matches!(
        PieceChain::resume(&want_dir, ResultId([9; 32]), 0, chain_options()),
        Err(ww_tape::WriteError::Ended(ww_tape::FinishState::Complete))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A chain in memory left unfinished resumes on the same [MemTapes] and ends as one written in one go.
#[test]
fn a_chain_in_memory_resumes() {
    let tmp = scratch("mem_resume");
    let ops = ops(10);
    let (paths, seqs) = reference_chain(&tmp, &ops);
    let mem = MemTapes::new();
    let dir = Path::new("a");
    let mut c = PieceChain::create_on(mem.clone(), dir, ResultId([9; 32]), 0, &[], chain_options())
        .unwrap();
    for op in &ops[..ops.len() / 2] {
        apply_chain(&mut c, *op);
    }
    drop(c);
    let (mut c, resumed) =
        PieceChain::resume_on(mem.clone(), dir, ResultId([9; 32]), 0, chain_options()).unwrap();
    assert!(!resumed.entries.is_empty());
    assert!(!c.finished().is_empty());
    let next = c.writer().unwrap().next_seq();
    for (op, seq) in ops.iter().zip(&seqs) {
        if *seq >= next {
            apply_chain(&mut c, *op);
        }
    }
    let got = c.finish().unwrap();
    assert_eq!(got.len(), paths.len());
    for (f, p) in got.iter().zip(&paths) {
        assert_eq!(
            mem.get(&f.path).unwrap().unwrap().to_vec().unwrap(),
            std::fs::read(p).unwrap()
        );
    }
    // no piece of a chain in a place: nothing to resume
    assert!(Files.finished(&tmp.join("none")).unwrap().is_none());
    assert!(PieceChain::resume(&tmp.join("none"), ResultId([9; 32]), 0, chain_options()).is_err());
    std::fs::remove_dir_all(&tmp).unwrap();
}
