//! `TapeWriter::resume_on` on an unfinished piece a writer wrote, cut and mutated by the input: it never panics, and a
//! piece it resumes takes more blocks and finishes whole: a reader finds it by its tail at the start of the bytes and
//! its content hash verifies.
//!
//! Input: a mode byte (bit 0 aligned, bit 1 no piece head), a cut (u16 little endian, modulo the piece's length + 1),
//! then flips of three bytes each (an offset, u16 little endian, modulo the cut length, and the bits to flip).
use std::path::Path;
use std::sync::OnceLock;

use ww_tape::chunk::ChunkHead;
use ww_tape::index::IndexPolicy;
use ww_tape::piece::{PieceHead, PieceId, PieceIdent, ResultId};
use ww_tape::reader::Found;
use ww_tape::{
    ALIGNMENT, BlockId, BlockKind, BlockWriter, ClockTime, Codec, ColumnKind, ColumnSpec,
    Describe, DescribeSet, EndMode, Layout, MemSink, Producer, RunTag, SyncPolicy, TapeReader,
    TapeSink, TapeWriter, WriterOptions,
};

/// Rounds of blocks in a template piece.
const ROUNDS: u64 = 6;

fn options(aligned: bool) -> WriterOptions {
    WriterOptions {
        sync: SyncPolicy::None,
        index: IndexPolicy {
            bytes: 1024,
            interval: None,
        },
        alignment: if aligned { ALIGNMENT } else { 0 },
        describe_repeat: 4 << 10,
    }
}

fn records(id: BlockId, n: u64) -> Vec<u8> {
    let mut buf = [0u8; 128];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id, &[1]).expect("a block fits");
    b.push(9, ClockTime::new(1, n as i64), &[&[n as u8; 20]])
        .expect("a record fits");
    let len = b.finish();
    buf[..len].to_vec()
}

/// Track `t`'s chunk `k`: 16 items with raw starts.
fn chunk(t: u64, k: u64) -> (ChunkHead, Vec<u8>) {
    let first = k * 16;
    let starts: Vec<u8> = (first..first + 16)
        .flat_map(|i| (i as i64 * 4 + t as i64).to_le_bytes())
        .collect();
    let head = ChunkHead {
        kind: BlockKind::Events,
        track_id: 0x1000 + t,
        track_number: t as u32 + 1,
        first_item: first,
        count: 16,
        first_tick: first as i64 * 4 + t as i64,
        last_tick: (first + 15) as i64 * 4 + t as i64,
        latest_end: (first + 15) as i64 * 4 + t as i64,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    (head, starts)
}

fn write_chunk<S: TapeSink>(w: &mut TapeWriter<S>, t: u64, k: u64) {
    let (h, c) = chunk(t, k);
    let spec = ColumnSpec {
        kind: ColumnKind::Starts,
        codec: Codec::RAW,
        param: 0,
        len: c.len(),
    };
    w.write_chunk(&h, &[(spec, &c)]).expect("a chunk writes");
}

/// An unfinished piece: a describe set of several blocks, then rounds of chunks, records and deferred records.
fn template(aligned: bool, head: bool) -> Vec<u8> {
    let o = options(aligned);
    let mut w = TapeWriter::create_on(MemSink::new(Path::new("t")), o.sync);
    w.set_alignment(o.alignment).expect("a valid alignment");
    w.set_index_policy(o.index);
    w.set_describe_repeat(o.describe_repeat);
    if head {
        let result = ResultId([1; 32]);
        let h = PieceHead {
            ident: PieceIdent {
                id: PieceId::first(&result),
                result,
                seq: 0,
            },
            prev: None,
            prev_run: RunTag::NONE,
        };
        w.begin_piece(&h, &[]).expect("a head writes");
    }
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "fuzz" }))
        .expect("a producer");
    set.insert(&Describe::Layout(Layout::UNALIGNED))
        .expect("a layout");
    let mut id = w.next_id();
    let mut blocks = vec![];
    set.write_blocks(&mut id, 60, &[], |b| blocks.push(b.to_vec()))
        .expect("the set writes");
    for b in &blocks {
        w.write_block(b).expect("a describe block writes");
    }
    for k in 0..ROUNDS {
        write_chunk(&mut w, 0, k);
        w.write_block(&records(w.next_id(), k))
            .expect("a records block writes");
        write_chunk(&mut w, 1, k);
        w.defer_block(records(w.next_id(), 100 + k))
            .expect("a deferred block");
    }
    w.flush_deferred().expect("deferred blocks write");
    w.sink().tape().to_vec().expect("the bytes")
}

/// The template pieces by mode, made once.
fn templates() -> &'static [Vec<u8>; 4] {
    static T: OnceLock<[Vec<u8>; 4]> = OnceLock::new();
    T.get_or_init(|| core::array::from_fn(|m| template(m & 1 != 0, m & 2 == 0)))
}

pub fn resume(data: &[u8]) {
    let (mode, rest) = data.split_first().map_or((0, data), |(m, r)| (*m, r));
    let aligned = mode & 1 != 0;
    let mut bytes = templates()[usize::from(mode & 3)].clone();
    let (cut, flips) = match rest {
        [a, b, flips @ ..] => (usize::from(u16::from_le_bytes([*a, *b])), flips),
        _ => (usize::MAX, rest),
    };
    bytes.truncate(cut % (bytes.len() + 1));
    if !bytes.is_empty() {
        for f in flips.chunks_exact(3) {
            let at = usize::from(u16::from_le_bytes([f[0], f[1]])) % bytes.len();
            bytes[at] ^= f[2];
        }
    }
    let mut sink = MemSink::new(Path::new("p"));
    sink.write_all(&bytes).expect("memory takes the bytes");
    let tape = sink.tape();
    let Ok((mut w, resumed)) = TapeWriter::resume_on(sink, options(aligned)) else {
        return;
    };
    assert!(resumed.kept_bytes <= bytes.len() as u64);
    assert_eq!(resumed.kept_bytes + resumed.dropped_bytes, bytes.len() as u64);
    assert!(resumed.entries.iter().all(|e| e.offset < resumed.kept_bytes));
    for k in 0..2 {
        w.write_block(&records(w.next_id(), 200 + k))
            .expect("a resumed piece takes blocks");
        write_chunk(&mut w, 0, ROUNDS + k);
    }
    let finished = w.finish().expect("a resumed piece finishes");
    let out = tape.to_vec().expect("the bytes");
    assert_eq!(finished.len, out.len() as u64);
    let r = TapeReader::open(&out[..]).expect("a finished piece opens");
    let last = r.pieces().len().checked_sub(1).expect("a piece");
    let p = &r.pieces()[last];
    assert_eq!((p.start, p.found), (0, Found::Tail), "found whole by its tail");
    assert!(r.verify(last).expect("a finished piece"), "the content hash verifies");
}
