//! Tapes on media: runs, stale blocks of earlier lives, blank and torn stretches the data goes on after.

use ww_tape::piece::{PieceHead, PieceId, PieceIdent, ResultId, piece_head_len, write_piece_head};
use ww_tape::{
    BlockId, BlockKind, BlockWriter, ClockTime, Cut, Describe, DescribeSet, FileBlocks, FileError,
    Layout, Medium, OpenOptions, Producer, Recovery, RunTag, SyncPolicy, TapeReader, TapeWriter,
};

const OLD: RunTag = RunTag::writable(0x0bad_0001);
const NEW: RunTag = RunTag::writable(0x600d_0002);

fn records(id: BlockId) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]);
    b.push(9, ClockTime::new(1, id.seq as i64), &[&[id.seq as u8; 40]])
        .unwrap();
    let len = b.finish();
    buf[..len].to_vec()
}

fn piece_head(run: RunTag, prev_run: RunTag) -> Vec<u8> {
    let mut buf = vec![0u8; piece_head_len(0)];
    let head = PieceHead {
        ident: PieceIdent {
            id: PieceId([run.get() as u8; 16]),
            result: ResultId([1; 32]),
            seq: 0,
        },
        prev: None,
        prev_run,
    };
    let n = write_piece_head(&mut buf, BlockId::new(run, 0), &head, &[]).unwrap();
    buf.truncate(n);
    buf
}

/// A run on a medium: its piece head naming `prev_run`, a describe block with `layout`, then `n` records blocks.
fn run(run: RunTag, prev_run: RunTag, layout: Layout, n: u64) -> Vec<u8> {
    let mut out = piece_head(run, prev_run);
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "media" }))
        .unwrap();
    set.insert(&Describe::Layout(layout)).unwrap();
    let mut id = BlockId::new(run, 0);
    set.write_blocks(&mut id, 4096, &[], |b| out.extend_from_slice(b))
        .unwrap();
    for _ in 0..n {
        out.extend(records(id));
        id = id.next();
    }
    out
}

/// What a walk gives: (run, seq) of each records block, and the errors.
fn walk(bytes: &[u8], medium: Medium) -> (Vec<(RunTag, u64)>, Vec<FileError>) {
    let mut rd = FileBlocks::new(bytes).on(medium);
    let (mut got, mut errors) = (vec![], vec![]);
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) if b.kind == BlockKind::Records => got.push((b.run, b.seq)),
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
    }
    (got, errors)
}

fn seqs(run: RunTag, seqs: core::ops::Range<u64>) -> Vec<(RunTag, u64)> {
    seqs.map(|s| (run, s)).collect()
}

/// A card written once to its end, then again from its start by a shorter run: the old blocks behind the new end
/// of data are never data, whether the new run ends mid-way through an old block or on an old piece head.
#[test]
fn stale_blocks_behind_the_end_are_not_data() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let old = run(OLD, RunTag::NONE, sd, 200);
    let new = run(NEW, RunTag::NONE, sd, 20);
    // the new run overwrote the start of the old one, ending mid-way through an old block
    let mut card = new.clone();
    card.extend_from_slice(&old[new.len() + 13..]);
    let (got, errors) = walk(&card, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..21));
    assert!(
        matches!(errors[..], [FileError::Cut { offset, why: Cut::Bad(_) }] if offset == new.len() as u64),
        "{errors:?}"
    );
    // the old life starts right where the new one ends: its piece head names no run that ended there
    let card = [new.clone(), old.clone()].concat();
    let (got, errors) = walk(&card, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..21));
    assert!(
        matches!(errors[..], [FileError::Cut { offset, why: Cut::Stale { run } }] if offset == new.len() as u64 && run == OLD),
        "{errors:?}"
    );
    // a file takes any piece head as the start of a run: concatenated pieces
    let (got, errors) = walk(&card, Medium::File);
    assert_eq!(got, [seqs(NEW, 1..21), seqs(OLD, 1..201)].concat());
    assert!(errors.is_empty(), "{errors:?}");
    // on a medium too, a piece head naming the run just ended starts the next run
    let next = run(OLD, NEW, sd, 3);
    let (got, errors) = walk(&[new.clone(), next].concat(), Medium::Raw);
    assert_eq!(got, [seqs(NEW, 1..21), seqs(OLD, 1..4)].concat());
    assert!(errors.is_empty(), "{errors:?}");
    // a records block of another run is stale on a file too; the salvage walk reports and skips it
    let file = [
        new.clone(),
        records(BlockId::new(OLD, 99)),
        records(BlockId::new(NEW, 21)),
    ]
    .concat();
    let (got, errors) = walk(&file, Medium::File);
    assert_eq!(got, seqs(NEW, 1..21));
    assert!(matches!(
        errors[..],
        [FileError::Cut {
            why: Cut::Stale { .. },
            ..
        }]
    ));
    let mut rd = FileBlocks::with_recovery(&file[..], Recovery::Resync);
    let (mut got, mut stale) = (vec![], 0);
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) if b.kind == BlockKind::Records => got.push(b.seq),
            Ok(_) => {}
            Err(FileError::Stale { run, .. }) if run == OLD => stale += 1,
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!((got, stale), ((1..22).collect::<Vec<_>>(), 1));
}

/// `bytes` with `stretch` put in at `at`, padded with its first byte so the block after it starts on a multiple of
/// 512 when `align` is set; the stretch's offset and length.
fn with_stretch(bytes: &[u8], at: usize, stretch: &[u8], align: bool) -> (Vec<u8>, u64, u64) {
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(stretch);
    if align {
        while out.len() % 512 != 0 {
            out.push(stretch[0]);
        }
    }
    let len = (out.len() - at) as u64;
    out.extend_from_slice(&bytes[at..]);
    (out, at as u64, len)
}

/// Where the records block `seq` of a [run] starts.
fn block_start(bytes: &[u8], seq: u64) -> usize {
    let mut rd = FileBlocks::new(bytes).on(Medium::Raw);
    while let Some(Ok(b)) = rd.next_block() {
        if b.kind == BlockKind::Records && b.seq == seq {
            return rd.offset() as usize;
        }
    }
    panic!("no block {seq}");
}

/// A blank sector (erased or trimmed) followed by the run's next block is a gap, not the end, where the layout
/// allows gaps; on a file it stays the end.
#[test]
fn a_blank_sector_followed_by_the_runs_next_block_is_a_gap() {
    let sd = Layout::UNALIGNED.on_sd_card();
    for blank in [0x00u8, 0xFF] {
        let card = run(NEW, RunTag::NONE, sd, 10);
        let at = block_start(&card, 5);
        let (card, offset, len) = with_stretch(&card, at, &[blank; 512], false);
        let (got, errors) = walk(&card, Medium::Raw);
        assert_eq!(got, seqs(NEW, 1..11));
        assert!(
            matches!(errors[..], [FileError::Gap { offset: o, len: l }] if o == offset && l == len),
            "{errors:?}"
        );
        let file = run(NEW, RunTag::NONE, Layout::UNALIGNED, 10);
        let (file, offset, _) = with_stretch(&file, block_start(&file, 5), &[blank; 512], false);
        let (got, errors) = walk(&file, Medium::File);
        assert_eq!(got, seqs(NEW, 1..5));
        assert!(
            matches!(errors[..], [FileError::Cut { offset: o, why: Cut::Bad(_) }] if o == offset),
            "{errors:?}"
        );
    }
}

/// A torn unit (garbage) followed by the run's next block on the next resume boundary is a gap; an older block of
/// the run there, or one past the gap's reach, is not, and the stretch is the end of data.
#[test]
fn a_torn_unit_then_the_next_block_on_a_boundary_is_a_gap() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let card = run(NEW, RunTag::NONE, sd, 10);
    let at = block_start(&card, 5);
    let (torn, offset, len) = with_stretch(&card, at, &[0x5A; 700], true);
    let (got, errors) = walk(&torn, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..11));
    assert!(
        matches!(errors[..], [FileError::Gap { offset: o, len: l }] if o == offset && l == len),
        "{errors:?}"
    );
    // the block on the boundary is an older one of the run: stale, the end of data is the stretch's start
    let mut stale = card[..at].to_vec();
    stale.extend_from_slice(&[0x5A; 300]);
    while stale.len() % 512 != 0 {
        stale.push(0x5A);
    }
    stale.extend(records(BlockId::new(NEW, 2)));
    let (got, errors) = walk(&stale, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..5));
    assert!(matches!(errors[..], [FileError::Cut { offset: o, .. }] if o == at as u64));
    // a stretch longer than the gap's reach is the end
    let (far, _, _) = with_stretch(
        &card,
        at,
        &vec![0x5A; ww_tape::SD_GAP_MAX as usize + 1024],
        true,
    );
    let (got, errors) = walk(&far, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..5));
    assert!(matches!(errors[..], [FileError::Cut { offset: o, .. }] if o == at as u64));
}

/// A finished file of run `run` (describe with `layout`, `n` records blocks, final index, finish, tail).
fn finished(run: RunTag, layout: Layout, n: u64) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!(
        "ww_tape_media_{}_{}",
        run.get(),
        std::process::id()
    ));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.wwt");
    let mut w = TapeWriter::create(&path, SyncPolicy::None).unwrap();
    w.set_run(run);
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "media" }))
        .unwrap();
    set.insert(&Describe::Layout(layout)).unwrap();
    let mut id = w.next_id();
    let mut blocks = vec![];
    set.write_blocks(&mut id, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    for b in &blocks {
        w.write_block(b).unwrap();
    }
    for _ in 0..n {
        w.write_block(&records(w.next_id())).unwrap();
    }
    w.finish().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    bytes
}

fn raw() -> OpenOptions {
    OpenOptions {
        medium: Medium::Raw,
        ..Default::default()
    }
}

/// On a raw medium the old life's tail sits at the end; opened as a file it would pass for the newest piece. Opened
/// as a raw medium, the walk goes forward from the start and ends where the new run ends.
#[test]
fn a_raw_medium_opens_forward_and_never_takes_a_stale_tail() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let old = finished(OLD, sd, 200);
    let new = run(NEW, RunTag::NONE, sd, 20);
    // the new run overwrote the start of the old file in place
    let mut card = new.clone();
    card.extend_from_slice(&old[new.len()..]);
    let as_file = TapeReader::open(&card[..]).unwrap();
    assert!(
        as_file.pieces().iter().any(|p| p.tail.is_some()),
        "the stale tail fools a file reader"
    );
    let r = TapeReader::open_with(&card[..], raw()).unwrap();
    assert_eq!(r.pieces().len(), 1);
    let p = &r.pieces()[0];
    assert_eq!(
        (p.start, p.end, p.cut),
        (0, new.len() as u64, Some(new.len() as u64))
    );
    assert!(p.tail.is_none());
    assert_eq!(
        p.head.as_ref().unwrap().0.ident.id,
        PieceId([NEW.get() as u8; 16])
    );
    // from a hint: the walk starts there
    let hint = block_start(&card, 10) as u64;
    let r = TapeReader::open_with(
        &card[..],
        OpenOptions {
            start: hint,
            ..raw()
        },
    )
    .unwrap();
    assert_eq!(
        (r.pieces()[0].start, r.pieces()[0].end),
        (hint, new.len() as u64)
    );
    assert_eq!(r.entries(0).unwrap().len(), 11, "records 10 to 20");
}

/// The positional reader reads past a gap as the walk does, and lists it.
#[test]
fn the_positional_reader_reads_past_a_gap() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let card = run(NEW, RunTag::NONE, sd, 10);
    let at = block_start(&card, 5);
    let (card, offset, len) = with_stretch(&card, at, &[0x5A; 700], true);
    let r = TapeReader::open_with(&card[..], raw()).unwrap();
    let p = &r.pieces()[0];
    assert_eq!(p.gaps, [(offset, len)]);
    assert_eq!((p.end, p.cut), (card.len() as u64, None));
    // describe block and 10 records blocks
    assert_eq!(r.entries(0).unwrap().len(), 11);
}
