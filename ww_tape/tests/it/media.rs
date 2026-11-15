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
        while !out.len().is_multiple_of(512) {
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
    while !stale.len().is_multiple_of(512) {
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

/// A block written twice (a rewrite after a failed write, two copies merged) is a duplicate, not a cut, for the
/// positional reader too: the same bytes are dropped, other bytes replace the earlier copy.
#[test]
fn a_duplicate_block_is_not_a_cut() {
    let card = run(NEW, RunTag::NONE, Layout::UNALIGNED, 6);
    let at = block_start(&card, 4);
    let again = card[block_start(&card, 3)..at].to_vec();
    let dup = [&card[..at], &again, &card[at..]].concat();
    let r = TapeReader::open_with(&dup[..], raw()).unwrap();
    let p = &r.pieces()[0];
    assert_eq!((p.end, p.cut), (dup.len() as u64, None));
    assert!(p.replaced.is_empty());
    assert_eq!(
        r.entries(0).unwrap().len(),
        7,
        "describe and 6 records, the copy dropped"
    );
    let (got, errors) = walk(&dup, Medium::Raw);
    assert_eq!((got, errors.len()), (seqs(NEW, 1..7), 0));
    // other bytes under the same id: the later copy wins, reported
    let other = {
        let mut buf = [0u8; 256];
        let mut b = BlockWriter::new(&mut buf, BlockKind::Records, BlockId::new(NEW, 3), &[1]);
        b.push(9, ClockTime::new(1, 3), &[b"rewritten"]).unwrap();
        let len = b.finish();
        buf[..len].to_vec()
    };
    let rewritten = [&card[..at], &other, &card[at..]].concat();
    let r = TapeReader::open_with(&rewritten[..], raw()).unwrap();
    let p = &r.pieces()[0];
    assert_eq!((p.cut, p.replaced.as_slice()), (None, &[at as u64][..]));
    assert_eq!(r.entries(0).unwrap().len(), 7);
    let (got, errors) = walk(&rewritten, Medium::Raw);
    assert_eq!(got, [seqs(NEW, 1..4), seqs(NEW, 3..7)].concat());
    assert!(
        matches!(errors[..], [FileError::Replaced { seq: 3, .. }]),
        "{errors:?}"
    );
}

/// A block written again after padding to the next sector (a rewrite after a failed program) resumes the data like
/// its next block does: same bytes dropped, other bytes taking its place, for both readers.
#[test]
fn a_duplicate_after_a_gap_resumes_the_data() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let card = run(NEW, RunTag::NONE, sd, 10);
    let (from, at) = (block_start(&card, 3), block_start(&card, 4));
    let other = {
        let mut buf = [0u8; 256];
        let mut b = BlockWriter::new(&mut buf, BlockKind::Records, BlockId::new(NEW, 3), &[1]);
        b.push(9, ClockTime::new(1, 3), &[b"rewritten"]).unwrap();
        let len = b.finish();
        buf[..len].to_vec()
    };
    for (again, replaced) in [(card[from..at].to_vec(), false), (other, true)] {
        let mut padded = card[..at].to_vec();
        while !padded.len().is_multiple_of(512) {
            padded.push(0xFF);
        }
        let gap = (at as u64, (padded.len() - at) as u64);
        let bytes = [&padded[..], &again, &card[at..]].concat();
        let (got, errors) = walk(&bytes, Medium::Raw);
        let want = if replaced {
            [seqs(NEW, 1..4), seqs(NEW, 3..11)].concat()
        } else {
            seqs(NEW, 1..11)
        };
        assert_eq!(got, want);
        assert!(
            matches!(errors[0], FileError::Gap { offset, len } if (offset, len) == gap),
            "{errors:?}"
        );
        assert_eq!(errors.len(), 1 + usize::from(replaced), "{errors:?}");
        let r = TapeReader::open_with(&bytes[..], raw()).unwrap();
        let p = &r.pieces()[0];
        assert_eq!((p.end, p.cut), (bytes.len() as u64, None));
        assert_eq!(p.gaps, [gap]);
        assert_eq!(p.replaced.len(), usize::from(replaced));
    }
}

/// A layout whose gap holds more than [ww_tape::GAP_PROBES_MAX] resume places, or is longer than
/// [ww_tape::GAP_MAX_MAX], is not one a reader takes: its describe record doesn't decode, and a walk gives it no gap.
#[test]
fn a_layout_past_the_gap_limits_is_refused() {
    use exact_units::Bytes;
    use shrink_wrap::prelude::*;
    let layout = |gap_max: u32, resume_align: u32| Layout {
        gap_max: Bytes::new(gap_max),
        resume_align: Bytes::new(resume_align),
        ..Layout::UNALIGNED
    };
    assert!(Layout::UNALIGNED.on_sd_card().is_valid());
    assert!(
        layout(
            ww_tape::GAP_MAX_MAX,
            ww_tape::GAP_MAX_MAX / ww_tape::GAP_PROBES_MAX
        )
        .is_valid()
    );
    for bad in [
        layout(u32::MAX, 1),
        layout(ww_tape::GAP_MAX_MAX + 512, 512),
        layout(ww_tape::GAP_PROBES_MAX + 1, 1),
        layout(512, 0),
    ] {
        assert!(!bad.is_valid(), "{bad:?}");
        let mut item = [0u8; 64];
        let item = Describe::Layout(bad).to_ww_bytes(&mut item).unwrap();
        let mut buf = [0u8; 256];
        let mut b = BlockWriter::new(&mut buf, BlockKind::Describe, BlockId::new(NEW, 0), &[]);
        b.push_describe(&[item]).unwrap();
        let len = b.finish();
        let mut bytes = buf[..len].to_vec();
        let parsed = ww_tape::Block::read(&bytes).unwrap();
        let rec = parsed.records().next().unwrap().unwrap();
        assert!(Describe::from_ww_bytes(rec.item).is_err());
        // a byte after it: no gap rules, the end of data at once
        bytes.push(0x41);
        let (got, errors) = walk(&bytes, Medium::Raw);
        assert!(got.is_empty());
        assert!(
            matches!(errors[..], [FileError::Cut { offset: o, .. }] if o == len as u64),
            "{errors:?}"
        );
    }
}

/// Counts what a positional reader reads.
struct Counted<'a> {
    bytes: &'a [u8],
    reads: core::cell::Cell<u64>,
    read_bytes: core::cell::Cell<u64>,
}

impl ww_tape::reader::ByteSource for Counted<'_> {
    type Error = ww_tape::reader::OutOfRange;

    fn size(&self) -> Result<u64, Self::Error> {
        self.bytes.size()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.reads.set(self.reads.get() + 1);
        self.read_bytes
            .set(self.read_bytes.get() + buf.len() as u64);
        self.bytes.read_at(offset, buf)
    }
}

/// A stretch whose every sector starts with a header of the run's next block, each 1 MiB long by its header but
/// none good: a reader reads each header once and checks at most [ww_tape::GAP_CHECKS_MAX] of them whole, then
/// takes the stretch's start as the end of data.
#[test]
fn a_gap_costs_one_header_per_resume_place() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let card = run(NEW, RunTag::NONE, sd, 6);
    let mut header = records(BlockId::new(NEW, 100))[..32].to_vec();
    let claimed = 1u32 << 20;
    header[8..12].copy_from_slice(&claimed.to_le_bytes());
    let mut bytes = card.clone();
    let at = bytes.len() as u64;
    // a bad block where the next would start, then a header on every sector
    bytes.extend_from_slice(&[0x5A; 100]);
    while bytes.len() < card.len() + ww_tape::SD_GAP_MAX as usize + (2 << 20) {
        if bytes.len().is_multiple_of(512) {
            bytes.extend_from_slice(&header);
        } else {
            bytes.push(0x5A);
        }
    }
    let src = Counted {
        bytes: &bytes,
        reads: 0.into(),
        read_bytes: 0.into(),
    };
    let r = TapeReader::open_with(&src, raw()).unwrap();
    let p = &r.pieces()[0];
    assert_eq!((p.end, p.cut), (at, Some(at)));
    let places = u64::from(ww_tape::SD_GAP_MAX / ww_tape::SD_RESUME_ALIGN) + 1;
    let whole = u64::from(ww_tape::GAP_CHECKS_MAX) * u64::from(claimed);
    let most = card.len() as u64 * 4 + places * 32 + whole + (1 << 20);
    assert!(
        src.read_bytes.get() <= most,
        "{} bytes read, at most {most}",
        src.read_bytes.get()
    );
    // the stream reader cuts at the same place
    let (got, errors) = walk(&bytes, Medium::Raw);
    assert_eq!(got, seqs(NEW, 1..7));
    assert!(
        matches!(errors[..], [FileError::Cut { offset: o, .. }] if o == at),
        "{errors:?}"
    );
}

/// Counts the bytes a stream reader takes.
struct CountedRead<'a> {
    bytes: &'a [u8],
    read: u64,
}

impl std::io::Read for CountedRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.bytes.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

/// Erased space after the end of data on a raw medium is the end once it is longer than the gap: neither reader reads
/// the rest of the medium, and stale data after it is no cut.
#[test]
fn erased_space_past_the_gap_ends_a_raw_walk() {
    let sd = Layout::UNALIGNED.on_sd_card();
    let card = run(NEW, RunTag::NONE, sd, 6);
    let at = card.len() as u64;
    let mut bytes = card.clone();
    bytes.resize(card.len() + (16 << 20), 0xFF);
    bytes.extend(records(BlockId::new(OLD, 9)));
    let src = Counted {
        bytes: &bytes,
        reads: 0.into(),
        read_bytes: 0.into(),
    };
    let r = TapeReader::open_with(&src, raw()).unwrap();
    let p = &r.pieces()[0];
    assert_eq!((p.end, p.cut), (at, None));
    let most = at * 4 + 2 * u64::from(ww_tape::SD_GAP_MAX) + (1 << 20);
    assert!(
        src.read_bytes.get() <= most,
        "{} bytes read, at most {most}",
        src.read_bytes.get()
    );
    let mut rd = CountedRead {
        bytes: &bytes,
        read: 0,
    };
    let mut fb = FileBlocks::new(&mut rd).on(Medium::Raw);
    let mut n = 0;
    while let Some(b) = fb.next_block() {
        b.unwrap();
        n += 1;
    }
    drop(fb);
    assert_eq!(
        n,
        walk(&card, Medium::Raw).0.len() + 2,
        "piece head, describe, records"
    );
    assert!(
        rd.read <= at + u64::from(ww_tape::SD_GAP_MAX) + (1 << 20),
        "{} bytes read",
        rd.read
    );
}

/// On a raw medium a piece whose head describe set is damaged and whose run ends there, with an earlier life's
/// describe set behind it, has no describe set: the stale one is never taken for it.
#[test]
fn a_stale_describe_set_is_not_the_pieces() {
    let set = |run: RunTag| {
        let mut s = DescribeSet::new();
        s.insert(&Describe::Producer(Producer { name: "media" }))
            .unwrap();
        s.insert(&Describe::Layout(Layout::UNALIGNED)).unwrap();
        let mut id = BlockId::new(run, 0);
        let mut out = vec![];
        s.write_blocks(&mut id, 64, &[], |b| out.push(b.to_vec()))
            .unwrap();
        assert_eq!(out.len(), 2);
        out
    };
    let mut card = piece_head(NEW, RunTag::NONE);
    let new = set(NEW);
    let damaged_at = card.len() + new[0].len();
    card.extend(new.concat());
    card[damaged_at + 40] ^= 1;
    card.extend(set(OLD).concat());
    let r = TapeReader::open_with(&card[..], raw()).unwrap();
    assert_eq!(r.pieces()[0].cut, Some(damaged_at as u64));
    assert!(r.describe_set(0).unwrap().is_empty());
}

/// A file that allows gaps, read from its newest checkpoint: the stretch after it is read with the file's gap rules,
/// as a walk from the start reads it.
#[test]
fn a_gap_after_the_newest_checkpoint_is_read_past() {
    let dir = std::env::temp_dir().join(format!("ww_tape_media_ckpt_{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut w = TapeWriter::create(&dir.join("c.wwt"), SyncPolicy::None).unwrap();
    w.set_index_policy(ww_tape::IndexPolicy {
        bytes: 2048,
        interval: None,
    });
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "media" }))
        .unwrap();
    set.insert(&Describe::Layout(Layout::UNALIGNED.on_sd_card()))
        .unwrap();
    let mut id = w.next_id();
    let mut blocks = vec![];
    set.write_blocks(&mut id, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    for b in &blocks {
        w.write_block(b).unwrap();
    }
    for _ in 0..100 {
        w.write_block(&records(w.next_id())).unwrap();
    }
    // a few after the newest checkpoint
    w.set_index_policy(ww_tape::IndexPolicy::NONE);
    for _ in 0..5 {
        w.write_block(&records(w.next_id())).unwrap();
    }
    let bytes = std::fs::read(w.current_path()).unwrap();
    drop(w);
    std::fs::remove_dir_all(&dir).unwrap();
    // a blank sector before a records block after the newest checkpoint
    let mut rd = FileBlocks::new(&bytes[..]);
    let (mut index, mut recs) = (vec![], vec![]);
    while let Some(Ok(b)) = rd.next_block() {
        match b.kind {
            BlockKind::Index => index.push(rd.offset()),
            BlockKind::Records => recs.push(rd.offset()),
            _ => {}
        }
    }
    let last_index = *index.last().unwrap();
    let after = *recs.iter().find(|r| **r > last_index).unwrap();
    assert!(after > last_index && last_index > 0);
    let (file, offset, len) = with_stretch(&bytes, after as usize, &[0u8; 512], false);
    let (got, errors) = walk(&file, Medium::File);
    assert_eq!(got.len(), 105);
    assert!(matches!(errors[..], [FileError::Gap { .. }]), "{errors:?}");
    let r = TapeReader::open(&file[..]).unwrap();
    let p = &r.pieces()[0];
    assert!(
        matches!(p.found, ww_tape::reader::Found::Checkpoint { .. }),
        "{:?}",
        p.found
    );
    assert_eq!(
        (p.gaps.as_slice(), p.cut, p.end),
        (&[(offset, len)][..], None, file.len() as u64)
    );
}
