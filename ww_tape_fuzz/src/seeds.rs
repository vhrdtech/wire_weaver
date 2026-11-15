//! The seed files of the targets, in `seeds/<target>/`: valid blocks from ww_tape's golden files, the inputs behind
//! ww_tape's review fixes (named by the fix's commit), inputs of the hostile streams and the inputs of the known
//! failures. The same code gives the same files; `ww_tape_fuzz_seeds write` writes them.

use crate::hostile::CODECS;
use shrink_wrap::prelude::*;
use std::path::Path;
use ww_tape::index::{IndexEntry, IndexHead, IndexKind, index_len, write_index};
use ww_tape::lz4::LZ4_DECODED_MAX;
use ww_tape::piece::{
    ContentHash, Finish, FinishState, PieceHead, PieceId, PieceIdent, ResultId, TAIL_LEN, Tail,
    finish_len, piece_head_len, write_finish, write_piece_head,
};
use ww_tape::times::EncodedTimes;
use ww_tape::{
    Block, BlockId, BlockKind, BlockWriter, ClockTime, ColumnKind, Describe, Element,
    FLAG_SET_FIRST, GAP_MAX_MAX, GAP_PROBES_MAX, Layout, Producer, RunTag, SeriesSegment,
    TrackKind,
};

/// Longest seed file.
pub const SEED_LEN_MAX: usize = 64 << 10;
/// Most seed files of one target.
pub const SEEDS_PER_TARGET_MAX: usize = 64;
/// Inputs of a hostile stream kept as seeds per hostile target.
const STREAM_SEEDS: usize = 24;
/// Bytes of a hostile stream input.
const STREAM_INPUT_LEN: usize = 96;

/// One seed file.
pub struct Seed {
    pub target: &'static str,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The inputs of the known failures, minimised (`ww_tape_fuzz_seeds minimize`): target, name, input in hex.
pub const KNOWN_INPUTS: &[(&str, &str, &str)] = &[
    (
        "tape-reader-hostile",
        "prefix-and-salvage-walks-differ",
        "eb9543000000a10000000000008e3d00002600000000000000000000d34f00000000000000000000000000008900d6",
    ),
    (
        "tape-times-hostile",
        "chunk-count-unbounded",
        "80f70000000000000000000000000000000000000000002703a94d75",
    ),
];

/// Inputs these tests found that failed until STD-131 fixed them: target, name (seed `fixed-<name>`), hex.
pub const FIXED_INPUTS: &[(&str, &str, &str)] = &[
    (
        "tape-index-hostile",
        "reader-entries-past-the-piece",
        "00ef86000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b6",
    ),
    (
        "tape-reader-hostile",
        "reader-entries-past-the-piece",
        "000011000000000000000000000000000000a10000003300000000000000000072",
    ),
    (
        "tape-summary-hostile",
        "node-items-overflow",
        "00000000000000000000000000000000000000000000006b003b",
    ),
    (
        "tape-reader-hostile",
        "reader-entries-past-the-piece-2",
        "c07a113987eed2e835e80f8c7db504afa55aa13c2341339c50117a5b4efb0be472b186398442d67290a057c7d0c48c2bd12b8110c47f7451c6fd1c660edf6e17bd752d8944c9c2d616e0f9257a475697",
    ),
];

fn seed(target: &'static str, name: impl Into<String>, bytes: Vec<u8>) -> Seed {
    Seed {
        target,
        name: name.into(),
        bytes,
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

/// The blocks of a golden file: one per line, in hex.
fn golden(text: &str) -> Vec<Vec<u8>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| unhex(l.trim()))
        .collect()
}

const GOLDEN_V2: &str = include_str!("../../ww_tape/golden/v2.hex");
const GOLDEN_CHUNKS: &str = include_str!("../../ww_tape/golden/v2-chunks.hex");
const GOLDEN_INDEX: &str = include_str!("../../ww_tape/golden/v2-index.hex");
const GOLDEN_PAYLOADS: &str = include_str!("../../ww_tape/golden/v2-payloads.hex");
const GOLDEN_SAMPLES: &str = include_str!("../../ww_tape/golden/v2-samples.hex");
const GOLDEN_SERIES: &str = include_str!("../../ww_tape/golden/v2-series.hex");
const GOLDEN_SUMMARY: &str = include_str!("../../ww_tape/golden/v2-summary.hex");
const GOLDEN_TIMES: &str = include_str!("../../ww_tape/golden/v2-times.hex");

/// Each block of a golden file as a seed of `target`.
fn golden_blocks(target: &'static str, file: &str, text: &str) -> Vec<Seed> {
    golden(text)
        .into_iter()
        .enumerate()
        .map(|(i, b)| seed(target, format!("golden-{file}-{i:02}"), b))
        .collect()
}

/// The golden sample columns as `tape-samples` inputs: `[codec][element][channels - 1][count u16][from u16][column]`.
fn golden_samples() -> Vec<Seed> {
    let mut out = vec![];
    for (i, line) in GOLDEN_SAMPLES.lines().enumerate() {
        let Some((head, hex)) = line.split_once(':') else {
            continue;
        };
        let f: Vec<&str> = head.split_whitespace().collect();
        let [codec, element, channels, count] = f[..] else {
            continue;
        };
        let codec: u8 = codec.parse().expect("a codec id");
        let codec = CODECS
            .iter()
            .position(|c| c.0 == codec)
            .expect("a known codec") as u8;
        let element = Element::ALL
            .iter()
            .position(|e| format!("{e:?}") == element)
            .expect("a known element") as u8;
        let channels: u8 = channels.parse().expect("channels");
        let count: u16 = count.parse().expect("a count");
        let mut b = vec![codec, element, channels - 1];
        b.extend_from_slice(&count.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend(unhex(hex.trim()));
        out.push(seed("tape-samples", format!("golden-v2-samples-{i:02}"), b));
    }
    out
}

/// The describe records of series tracks and the segment records in the golden series blocks.
fn golden_series() -> Vec<Seed> {
    let mut out = vec![];
    for b in golden(GOLDEN_SERIES) {
        let Ok(block) = Block::read(&b) else { continue };
        let mut plain = Vec::new();
        let Ok(decoded) = block.decode(&mut plain) else {
            continue;
        };
        for r in decoded.records().flatten() {
            let name = match Describe::from_ww_bytes(r.item) {
                Ok(Describe::Track(t)) if matches!(t.kind, TrackKind::Series(_)) => "track",
                _ if r.at.is_some() && SeriesSegment::from_ww_bytes(r.item).is_ok() => "segment",
                _ => continue,
            };
            let n = out.len();
            out.push(seed(
                "tape-series",
                format!("golden-v2-series-{name}-{n:02}"),
                r.item.to_vec(),
            ));
        }
    }
    out
}

/// A chunk block of `cols` with `head`.
fn chunk_block(enc: &EncodedTimes) -> Vec<u8> {
    let cols: Vec<_> = enc.columns().collect();
    let specs: Vec<_> = cols.iter().map(|c| c.0).collect();
    let mut buf = vec![0u8; ww_tape::chunk::chunk_len(&specs, 0)];
    let n = ww_tape::chunk::write_chunk(
        &mut buf,
        BlockId::new(RunTag::writable(1), 1),
        &enc.head.chunk_head(1, 1, 0),
        &cols,
        0,
    )
    .expect("a valid chunk");
    buf.truncate(n);
    buf
}

/// The block offset of the first byte of column `kind`.
fn column_start(b: &[u8], kind: ColumnKind) -> usize {
    let block = Block::read(b).expect("written good");
    let chunk = block.chunk().expect("a chunk");
    let col = chunk
        .columns()
        .find(|c| c.kind == kind)
        .expect("the column");
    chunk.payload_offset() + col.offset
}

fn le(b: &[u8], at: usize, n: usize) -> usize {
    let mut v = [0u8; 8];
    v[..n].copy_from_slice(&b[at..at + n]);
    usize::from_le_bytes(v)
}

/// fix 32f75a289 and c1d25a45b: times whose group records or values were damaged after the CRCs, read by salvage.
fn times_fixes() -> Vec<Seed> {
    let mut out = vec![];
    // 33 groups of linear starts with jitter
    let s: Vec<i64> = (0..33 * 128i64).map(|i| i * 1000 + (i * i) % 17).collect();
    let good = chunk_block(&EncodedTimes::encode(&s, None, None).expect("valid times"));
    let p = column_start(&good, ColumnKind::Starts);
    let rec = |g: usize, at: usize| p + g * 25 + at;
    let residuals = |g: usize| p + 33 * 25 + le(&good, rec(g, 21), 4);
    let w31 = usize::from(good[rec(31, 20)]);
    let top = 64 * w31 + w31 - 1;
    let flips = [
        ("residual-minimum", rec(5, 16), 0x01),
        ("item-0-residual", residuals(5), 0x01),
        ("first-tick", rec(5, 7), 0x01),
        ("first-tick-order", rec(5, 3), 0x80),
        ("slope", rec(5, 14), 0x01),
        (
            "residual-past-last-tick",
            residuals(31) + top / 8,
            1 << (top % 8),
        ),
    ];
    for (what, at, bit) in flips {
        let mut b = good.clone();
        b[at] ^= bit;
        out.push(seed("tape-times", format!("fix-32f75a289-{what}"), b));
    }
    // durations and flags kept in bounds
    let s: Vec<i64> = (0..300i64).map(|i| i * 1000 + (i * i) % 17).collect();
    let spans: Vec<i64> = s.iter().zip(0..).map(|(x, i)| x + i % 100).collect();
    let flags: Vec<u32> = (0..300).map(|i| u32::from(i % 4 == 0) * 2).collect();
    let good = chunk_block(&EncodedTimes::encode(&s, Some(&spans), Some(&flags)).expect("valid"));
    let mut b = good.clone();
    let e = column_start(&good, ColumnKind::Ends);
    b[e + 2 * 13 + 2] ^= 0x04;
    let f = column_start(&good, ColumnKind::Flags);
    let f_off = le(&good, f + 8 + 4, 2);
    b[f + 3 * 8 + f_off + 64 * 2 / 8] ^= 1;
    out.push(seed("tape-times", "fix-32f75a289-duration-and-flag", b));
    // a group's values past the column: group 3's offset one past the column's end
    let mut b = good.clone();
    let col_len = good.len() - 4 - 16 - p;
    b[rec(3, 21)..rec(3, 25)].copy_from_slice(&(col_len as u32 + 1).to_le_bytes());
    out.push(seed("tape-times", "fix-c1d25a45b-offset-past-column", b));
    // gaps of 2^63 ticks and more
    let wide = EncodedTimes::encode(&[i64::MIN, -1, 0, i64::MAX], None, None).expect("valid");
    out.push(seed(
        "tape-times",
        "fix-60e5d677c-gaps-past-i64",
        chunk_block(&wide),
    ));
    out
}

/// fix 1cc795107: lz4 columns stating decoded lengths past what their bytes can give, as `tape-samples` inputs.
fn lz4_fixes() -> Vec<Seed> {
    let max = (LZ4_DECODED_MAX as u32).to_le_bytes();
    let columns: [(&str, [u8; 5]); 3] = [
        ("max-length-in-5-bytes", [max[0], max[1], max[2], max[3], 0]),
        ("bound-271", [15, 1, 0, 0, 0]),
        ("bound-272", [16, 1, 0, 0, 0]),
    ];
    columns
        .into_iter()
        .map(|(what, col)| {
            // raw u8 samples, no count: the bytes go to the lz4 decoder
            let mut b = vec![0, 4, 0, 0, 0, 0, 0];
            b.extend_from_slice(&col);
            seed("tape-samples", format!("fix-1cc795107-{what}"), b)
        })
        .collect()
}

const RUN: RunTag = RunTag::writable(0x600d_0002);
const OLD: RunTag = RunTag::writable(0x0bad_0001);

fn tail(piece_len: u64, finish_offset: u64, finish_len: u32) -> Vec<u8> {
    let t = Tail {
        piece_len,
        finish_offset,
        finish_len,
        state: FinishState::Complete,
        hash: ContentHash([0; 32]),
        ident: None,
        next: None,
    };
    let mut b = vec![0u8; TAIL_LEN];
    t.write(&mut b, BlockId::new(RUN, 0));
    b
}

/// A finish naming `final_index`, then its tail.
fn finish_only(final_index: u64, piece_len: Option<u64>) -> Vec<u8> {
    let finish = Finish {
        state: FinishState::Complete,
        hash: ContentHash([0; 32]),
        checkpoint: None,
        final_index: Some(final_index),
        describe: None,
        ident: None,
        next: None,
    };
    let mut b = vec![0u8; finish_len(0)];
    let n = write_finish(&mut b, BlockId::new(RUN, 0), &finish, &[]).expect("a finish");
    b.truncate(n);
    let len = piece_len.unwrap_or((n + TAIL_LEN) as u64);
    b.extend(tail(len, 0, n as u32));
    b
}

/// fix baf02dd6d, 208b9b90f, 5c6fa63a7: tails and finishes whose offsets point past the piece or wrap.
fn index_fixes() -> Vec<Seed> {
    let mut out = vec![seed(
        "tape-index",
        "fix-baf02dd6d-tail-offsets-wrap",
        tail(TAIL_LEN as u64, u64::MAX - 343, 180),
    )];
    for (what, at) in [
        ("max-10", u64::MAX - 10),
        ("max-40", u64::MAX - 40),
        ("1-tib", 1 << 40),
        ("8", 8),
    ] {
        out.push(seed(
            "tape-index",
            format!("fix-208b9b90f-final-index-at-{what}"),
            finish_only(at, None),
        ));
    }
    out.push(seed(
        "tape-index",
        "fix-5c6fa63a7-piece-of-5-gib",
        finish_only(0, Some(5 << 30)),
    ));
    out
}

fn records(id: BlockId) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]).expect("a block");
    b.push(9, ClockTime::new(1, id.seq as i64), &[&[id.seq as u8; 40]])
        .expect("fits");
    let len = b.finish();
    buf[..len].to_vec()
}

fn describe(id: BlockId, layout: Layout, flags: u16) -> Vec<u8> {
    let mut items = [[0u8; 64]; 2];
    let [a, b] = &mut items;
    let producer = Describe::Producer(Producer { name: "media" })
        .to_ww_bytes(a)
        .expect("a producer encodes");
    let layout = Describe::Layout(layout)
        .to_ww_bytes(b)
        .expect("a layout encodes");
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, id, &[]).expect("a block");
    w.push_describe(&[producer]).expect("fits");
    w.push_describe(&[layout]).expect("fits");
    w.set_flags(flags);
    let len = w.finish();
    buf[..len].to_vec()
}

fn layout(gap_max: u32, resume_align: u32) -> Layout {
    Layout {
        gap_max: exact_units::Bytes::new(gap_max),
        resume_align: exact_units::Bytes::new(resume_align),
        ..Layout::UNALIGNED
    }
}

/// A run on a medium: a piece head, a describe block with `layout`, then `n` records blocks.
fn run(layout: Layout, n: u64) -> Vec<u8> {
    let mut out = vec![0u8; piece_head_len(0)];
    let head = PieceHead {
        ident: PieceIdent {
            id: PieceId([2; 16]),
            result: ResultId([1; 32]),
            seq: 0,
        },
        prev: None,
        prev_run: RunTag::NONE,
    };
    let len = write_piece_head(&mut out, BlockId::new(RUN, 0), &head, &[]).expect("a head");
    out.truncate(len);
    let first_last = FLAG_SET_FIRST | ww_tape::FLAG_SET_LAST;
    out.extend(describe(BlockId::new(RUN, 0), layout, first_last));
    for seq in 0..n {
        out.extend(records(BlockId::new(RUN, seq)));
    }
    out
}

/// fix 74e52ec33, 23ceb1731, 4205a405e and the describe set and gap fixes: media with gaps, erased space, stale
/// blocks and layouts past the limits.
fn media_fixes() -> Vec<Seed> {
    let mut out = vec![];
    // layouts a reader refuses, then one more byte
    for (what, l) in [
        ("gap-max-u32", layout(u32::MAX, 1)),
        ("gap-past-max", layout(GAP_MAX_MAX + 512, 512)),
        ("probes-past-max", layout(GAP_PROBES_MAX + 1, 1)),
        ("resume-align-0", layout(512, 0)),
    ] {
        let mut b = describe(BlockId::new(RUN, 0), l, 0);
        b.push(0x41);
        out.push(seed(
            "tape-reader",
            format!("fix-74e52ec33-layout-{what}"),
            b,
        ));
    }
    // a bad block, then the header of the run's next block, 1 MiB long by its header, on every sector
    let card = run(layout(32 << 10, 512), 6);
    let mut header = records(BlockId::new(RUN, 100))[..32].to_vec();
    header[8..12].copy_from_slice(&(1u32 << 20).to_le_bytes());
    let mut b = card.clone();
    b.extend_from_slice(&[0x5a; 100]);
    while b.len() < card.len() + (40 << 10) {
        if b.len().is_multiple_of(512) {
            b.extend_from_slice(&header);
        } else {
            b.push(0x5a);
        }
    }
    out.push(seed("tape-reader", "fix-74e52ec33-a-header-per-sector", b));
    // erased space longer than the gap, then a stale block of an older run
    let mut b = run(layout(4096, 512), 6);
    let erased = b.len() + (16 << 10);
    b.resize(erased, 0xff);
    b.extend(records(BlockId::new(OLD, 9)));
    out.push(seed("tape-reader", "fix-23ceb1731-erased-past-the-gap", b));
    // a checkpoint, then a blank sector before the next records block
    let mut b = run(layout(4096, 512), 20);
    let mut entries = vec![];
    let mut at = 0;
    while let Ok(len) = ww_tape::block_len(&b[at..]) {
        let block = Block::read(&b[at..at + len]).expect("written good");
        entries.extend(IndexEntry::of_block(at as u64, &block));
        at += len;
    }
    let mut ix = vec![0u8; index_len(entries.len(), 0)];
    let head = IndexHead {
        kind: IndexKind::Checkpoint,
        self_offset: b.len() as u64,
        prev: None,
    };
    let n = write_index(&mut ix, BlockId::new(RUN, 19), &head, &entries).expect("an index");
    b.extend_from_slice(&ix[..n]);
    b.extend_from_slice(&[0; 512]);
    for seq in 20..25 {
        b.extend(records(BlockId::new(RUN, seq)));
    }
    out.push(seed(
        "tape-reader",
        "fix-4205a405e-gap-after-checkpoint",
        b.clone(),
    ));
    out.push(seed("tape-index", "fix-4205a405e-gap-after-checkpoint", b));
    // part of a describe set: a block marked first and no block marked last
    let mut b = describe(BlockId::new(RUN, 0), layout(4096, 512), FLAG_SET_FIRST);
    b.extend(records(BlockId::new(RUN, 0)));
    out.push(seed(
        "tape-reader",
        "fix-47ab14ea2-part-of-a-describe-set",
        b,
    ));
    // a block written again after a gap
    let mut b = run(layout(4096, 512), 3);
    b.resize(b.len().next_multiple_of(512) + 512, 0);
    b.extend(records(BlockId::new(RUN, 2)));
    b.extend(records(BlockId::new(RUN, 3)));
    out.push(seed(
        "tape-reader",
        "fix-67e392294-duplicate-after-a-gap",
        b,
    ));
    out
}

/// xorshift64 inputs of a hostile target's stream.
fn stream(target: &'static str, mut x: u64) -> Vec<Seed> {
    let mut out = vec![seed(target, "zeros", vec![0; STREAM_INPUT_LEN])];
    for i in 0..STREAM_SEEDS {
        let bytes = (0..STREAM_INPUT_LEN)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        out.push(seed(target, format!("stream-{i:02}"), bytes));
    }
    out
}

/// Every seed file of every target.
pub fn all() -> Vec<Seed> {
    let mut out = vec![];
    let concat = |text: &str| golden(text).concat();
    out.push(seed("tape-reader", "golden-v2-file", concat(GOLDEN_V2)));
    out.push(seed(
        "tape-reader",
        "golden-v2-index-piece",
        concat(GOLDEN_INDEX),
    ));
    out.extend(media_fixes());
    out.extend(golden_blocks("tape-chunk", "v2-chunks", GOLDEN_CHUNKS));
    out.extend(golden_blocks("tape-index", "v2-index", GOLDEN_INDEX));
    out.push(seed(
        "tape-index",
        "golden-v2-index-piece",
        concat(GOLDEN_INDEX),
    ));
    out.extend(index_fixes());
    out.extend(golden_blocks(
        "tape-payloads",
        "v2-payloads",
        GOLDEN_PAYLOADS,
    ));
    out.extend(golden_samples());
    out.extend(lz4_fixes());
    out.extend(golden_series());
    out.extend(golden_blocks("tape-summary", "v2-summary", GOLDEN_SUMMARY));
    out.extend(golden_blocks("tape-times", "v2-times", GOLDEN_TIMES));
    out.extend(times_fixes());
    for (i, (name, _)) in crate::targets::ALL.iter().enumerate() {
        if name.ends_with("-hostile") {
            out.extend(stream(name, 0x5eed_0000_0000_0001 + i as u64 * 0x9e37_79b9));
        }
    }
    for (inputs, prefix) in [(KNOWN_INPUTS, "known"), (FIXED_INPUTS, "fixed")] {
        for (target, name, hex) in inputs {
            let target = crate::targets::ALL
                .iter()
                .find(|(t, _)| t == target)
                .expect("a known target")
                .0;
            out.push(seed(target, format!("{prefix}-{name}"), unhex(hex)));
        }
    }
    out
}

/// Write every seed into `dir/<target>/<name>`, replacing the files there; returns how many.
pub fn write(dir: &Path) -> std::io::Result<usize> {
    let seeds = all();
    for (target, _) in crate::targets::ALL {
        let d = dir.join(target);
        if d.exists() {
            std::fs::remove_dir_all(&d)?;
        }
        std::fs::create_dir_all(&d)?;
    }
    for s in &seeds {
        std::fs::write(dir.join(s.target).join(&s.name), &s.bytes)?;
    }
    Ok(seeds.len())
}
