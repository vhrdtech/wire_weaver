use super::*;
use crate::tests::id;
use crate::{
    Block, BlockError, BlockId, BlockWriter, ClockTime, Describe, DescribeSet, FileBlocks,
    FileError, Layout, Producer, Recovery, RunTag, SyncPolicy, TapeWriter, set_id,
};

fn raw(kind: ColumnKind, param: u16, bytes: &[u8]) -> (ColumnSpec, &[u8]) {
    (
        ColumnSpec {
            kind,
            codec: Codec::RAW,
            param,
            len: bytes.len(),
        },
        bytes,
    )
}

fn i64s(v: impl IntoIterator<Item = i64>) -> Vec<u8> {
    v.into_iter().flat_map(i64::to_le_bytes).collect()
}

/// An events chunk of `n` items with raw starts, durations and payloads.
fn events(first_item: u64, n: u32) -> (ChunkHead, Vec<Vec<u8>>) {
    let starts: Vec<i64> = (0..n)
        .map(|i| 1_000 + 10 * i64::from(i) + first_item as i64)
        .collect();
    let head = ChunkHead {
        kind: BlockKind::Events,
        track_id: 0x1122_3344_5566_7788,
        track_number: 4,
        first_item,
        count: n,
        first_tick: starts[0],
        last_tick: *starts.last().unwrap(),
        latest_end: *starts.last().unwrap() + 4,
        time_mode: 0,
        end_mode: EndMode::Durations,
    };
    let cols = vec![
        i64s(starts),
        i64s((0..n).map(|i| i64::from(i % 5))),
        (0..n).flat_map(|i| (i % 3).to_le_bytes()).collect(),
        (0..n).flat_map(|i| [b'a' + (i % 26) as u8; 3]).collect(),
    ];
    (head, cols)
}

fn event_columns(cols: &[Vec<u8>]) -> Vec<(ColumnSpec, &[u8])> {
    vec![
        raw(ColumnKind::Starts, 0, &cols[0]),
        raw(ColumnKind::Ends, 0, &cols[1]),
        raw(ColumnKind::PayloadLengths, 0, &cols[2]),
        raw(ColumnKind::Payloads, 0, &cols[3]),
    ]
}

/// A series chunk of `n` i16 samples on two channels.
fn series(first: u64, n: u32) -> (ChunkHead, Vec<Vec<u8>>) {
    let head = ChunkHead {
        kind: BlockKind::Series,
        track_id: 0xabcd,
        track_number: 7,
        first_item: first,
        count: n,
        first_tick: first as i64 * 20,
        last_tick: (first + u64::from(n) - 1) as i64 * 20,
        latest_end: (first + u64::from(n) - 1) as i64 * 20,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    let ch = |c: u64| -> Vec<u8> {
        (0..u64::from(n))
            .flat_map(|i| (((first + i) * (c + 3)) as i16).to_le_bytes())
            .collect()
    };
    (head, vec![ch(0), ch(1)])
}

fn series_columns(cols: &[Vec<u8>]) -> Vec<(ColumnSpec, &[u8])> {
    vec![
        raw(ColumnKind::Samples, 0, &cols[0]),
        raw(ColumnKind::Samples, 1, &cols[1]),
    ]
}

fn encode(head: &ChunkHead, cols: &[(ColumnSpec, &[u8])], seq: u64, padding: usize) -> Vec<u8> {
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = vec![0u8; chunk_len(&specs, padding)];
    let n = write_chunk(&mut buf, id(seq), head, cols, padding).unwrap();
    assert_eq!(n, buf.len());
    buf
}

#[test]
fn crc_combine_is_the_crc_of_both() {
    let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 13) as u8).collect();
    let whole = CRC16.checksum(&data);
    for split in [0, 1, 3, 64, 1000, 4999, 5000] {
        let (a, b) = data.split_at(split);
        assert_eq!(
            crc32c_combine(CRC16.checksum(a), CRC16.checksum(b), b.len() as u64),
            whole,
            "split at {split}"
        );
    }
}

#[test]
fn events_chunk_round_trip() {
    let (head, cols) = events(100, 50);
    let columns = event_columns(&cols);
    let bytes = encode(&head, &columns, 9, 0);
    let block = Block::read(&bytes).unwrap();
    assert_eq!(
        (block.kind, block.seq, block.record_count),
        (BlockKind::Events, 9, 50)
    );
    assert!(block.is_intact());
    assert_eq!(block.records().count(), 0, "a chunk block has no records");
    let chunk = block.chunk().unwrap();
    assert_eq!(chunk.head(), head);
    assert_eq!(chunk.column_count(), 4);
    for (c, (spec, b)) in chunk.columns().zip(&columns) {
        assert_eq!(
            (c.kind, c.codec, c.param, c.bytes),
            (spec.kind, spec.codec, spec.param, *b)
        );
    }
    assert_eq!(
        chunk.find(ColumnKind::Payloads, 0).unwrap().bytes,
        &cols[3][..]
    );
    assert_eq!(chunk.segment_count(), 1);
    assert_eq!(chunk.bad_segments().count(), 0);
    // 16 zero bytes after the last column, still in the block
    let p = chunk.payload();
    assert!(p[p.len() - PAYLOAD_PADDING..].iter().all(|b| *b == 0));
    // head: header, fixed part, 4 columns, 1 segment CRC
    assert_eq!(chunk.payload_offset(), 32 + 52 + 4 * 8 + 4);
}

#[test]
fn chunk_heads_are_checked_on_write() {
    let (head, cols) = events(0, 3);
    let columns = event_columns(&cols);
    let mut buf = vec![0u8; 4096];
    let bad = |h: ChunkHead, c: &[(ColumnSpec, &[u8])]| {
        let mut b = vec![0u8; 4096];
        matches!(
            write_chunk(&mut b, id(0), &h, c, 0),
            Err(ChunkError::Invalid(_))
        )
    };
    assert!(bad(ChunkHead { count: 0, ..head }, &columns));
    assert!(bad(
        ChunkHead {
            last_tick: head.first_tick - 1,
            ..head
        },
        &columns
    ));
    assert!(bad(
        ChunkHead {
            first_item: u64::MAX,
            ..head
        },
        &columns
    ));
    assert!(bad(
        ChunkHead {
            kind: BlockKind::Records,
            ..head
        },
        &columns
    ));
    // the latest end: at least the last tick, and the last tick in a chunk of instants
    assert!(bad(
        ChunkHead {
            latest_end: head.last_tick - 1,
            ..head
        },
        &columns
    ));
    assert!(bad(
        ChunkHead {
            end_mode: EndMode::Absent,
            ..head
        },
        &columns
    ));
    assert!(!bad(
        ChunkHead {
            end_mode: EndMode::Absent,
            latest_end: head.last_tick,
            ..head
        },
        &columns
    ));
    assert!(bad(head, &[]));
    assert!(bad(head, &[raw(ColumnKind::Samples, 0, &cols[0])]));
    let (shead, scols) = series(0, 3);
    assert!(bad(shead, &[raw(ColumnKind::Starts, 0, &scols[0])]));
    assert!(bad(
        ChunkHead {
            latest_end: shead.last_tick + 1,
            ..shead
        },
        &series_columns(&scols)
    ));
    assert!(bad(
        ChunkHead {
            end_mode: EndMode::Durations,
            ..shead
        },
        &series_columns(&scols)
    ));
    assert!(matches!(
        write_chunk(&mut buf[..100], id(0), &head, &columns, 0),
        Err(ChunkError::Full { .. })
    ));
}

/// A series chunk over many segments: a flipped bit fails the block, and salvage loses exactly the segment it is in.
#[test]
fn flipped_bit_in_a_series_chunk_loses_one_segment() {
    // 2 channels × 300 000 samples × 2 bytes: 1.2 MB, 10 segments of 64 KiB
    let (head, cols) = series(0, 300_000);
    let bytes = encode(&head, &series_columns(&cols), 1, 0);
    let chunk = Block::read(&bytes).unwrap().chunk().unwrap();
    let (at, segs) = (chunk.payload_offset(), chunk.segment_count());
    assert_eq!(segs, (1_200_000 + PAYLOAD_PADDING).div_ceil(1 << 16));
    for offset in [0usize, 1, 65_535, 65_536, 600_000, 1_199_999, 1_200_010] {
        let mut bad = bytes.clone();
        bad[at + offset] ^= 0x10;
        assert_eq!(
            Block::read(&bad).unwrap_err(),
            BlockError::BadCrc,
            "flip at {offset}"
        );
        let b = Block::salvage(&bad).unwrap();
        assert!(!b.is_intact());
        let c = b.chunk().unwrap();
        assert_eq!(c.head(), head);
        assert_eq!(
            c.bad_segments().collect::<Vec<_>>(),
            [offset >> 16],
            "flip at {offset}"
        );
        let ch0 = c.column(0).unwrap();
        let ch1 = c.column(1).unwrap();
        // a channel is damaged where the bad segment overlaps it or the 16 bytes a codec may load past its end
        assert_eq!(
            c.column_damaged(&ch0),
            offset < 600_000 + PAYLOAD_PADDING || offset >> 16 == 599_999 >> 16
        );
        assert_eq!(
            c.column_damaged(&ch1),
            offset >= 600_000 || offset >> 16 == 600_000 >> 16
        );
        // the good segments are byte for byte what was written
        for s in 0..segs {
            if s != offset >> 16 {
                assert_eq!(c.segment(s), chunk.segment(s));
            }
        }
    }
    // the block CRC alone is damaged: every segment is good, the block is still not intact
    let mut bad = bytes.clone();
    let n = bad.len();
    bad[n - 1] ^= 1;
    let b = Block::salvage(&bad).unwrap();
    assert!(!b.is_intact());
    assert_eq!(b.chunk().unwrap().bad_segments().count(), 0);
}

/// A damaged head is not salvaged: nothing in the block, its length neither, can be trusted.
#[test]
fn damaged_head_is_not_salvaged() {
    let (head, cols) = events(0, 20);
    let bytes = encode(&head, &event_columns(&cols), 1, 0);
    let payload_at = Block::read(&bytes)
        .unwrap()
        .chunk()
        .unwrap()
        .payload_offset();
    for at in 0..payload_at {
        if (8..12).contains(&at) {
            continue; // the length: the block is then short or long, not a chunk to salvage
        }
        let mut bad = bytes.clone();
        bad[at] ^= 0x04;
        assert!(Block::salvage(&bad).is_err(), "flip at {at}");
    }
}

#[test]
fn padding_puts_the_payload_on_the_alignment() {
    let (head, cols) = series(10, 1000);
    let columns = series_columns(&cols);
    let specs: Vec<ColumnSpec> = columns.iter().map(|c| c.0).collect();
    let payload = specs.iter().map(|s| s.len).sum();
    let head_len = chunk_head_len(2, payload);
    for offset in [0u64, 1, 100, 4096 - head_len as u64, 4097, 123_456] {
        let pad = chunk_padding(offset, head_len, 4096).unwrap();
        let bytes = encode(&head, &columns, 0, pad);
        let c = Block::read(&bytes).unwrap().chunk().unwrap();
        assert_eq!(
            (offset + c.payload_offset() as u64) % 4096,
            0,
            "at {offset}"
        );
        assert!(pad < 4096);
    }
    assert_eq!(chunk_padding(77, head_len, 0), Some(0));
    assert_eq!(
        chunk_padding(77, head_len, 3000),
        None,
        "not a power of two"
    );
    assert_eq!(
        chunk_padding(u64::MAX, head_len, 4096).map(|p| p < 4096),
        Some(true)
    );
}

#[test]
fn renumbered_blocks_stay_good() {
    let (head, cols) = series(0, 70_000);
    let mut bytes = encode(&head, &series_columns(&cols), 1, 33);
    let other = BlockId::new(RunTag::writable(9), 42);
    set_id(&mut bytes, other).unwrap();
    let b = Block::read(&bytes).unwrap();
    assert_eq!((b.run, b.seq), (other.run, 42));
    let mut buf = [0u8; 64];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(3), &[1]).unwrap();
    w.push(1, ClockTime::new(1, 5), &[b"x"]).unwrap();
    let len = w.finish();
    set_id(&mut buf[..len], other.next()).unwrap();
    let b = Block::read(&buf[..len]).unwrap();
    assert_eq!((b.run, b.seq), (other.run, 43));
}

/// A describe block, small records blocks deferred into the gaps, and chunks, through [TapeWriter].
fn write_tape(path: &std::path::Path, layout: Layout) -> Vec<u8> {
    _ = std::fs::remove_file(path);
    let mut w = TapeWriter::create(path, SyncPolicy::None).unwrap();
    w.set_alignment(layout.alignment.get()).unwrap();
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "test" }))
        .unwrap();
    set.insert(&Describe::Layout(layout)).unwrap();
    let mut seq = id(0);
    let mut blocks = vec![];
    set.write_blocks(&mut seq, 4096, &[], |b| blocks.push(b.to_vec()))
        .unwrap();
    for b in &blocks {
        w.write_block(b).unwrap();
    }
    let small = |n: i64| {
        let mut buf = [0u8; 128];
        let mut b = BlockWriter::new(&mut buf, BlockKind::Records, id(0), &[1]).unwrap();
        b.push(2, ClockTime::new(1, n), &[&[n as u8; 20]]).unwrap();
        let len = b.finish();
        buf[..len].to_vec()
    };
    let (sh, sc) = series(0, 900);
    w.write_chunk(&sh, &series_columns(&sc)).unwrap();
    w.defer_block(small(1)).unwrap();
    w.defer_block(small(2)).unwrap();
    let (eh, ec) = events(0, 40);
    w.write_chunk(&eh, &event_columns(&ec)).unwrap();
    w.defer_block(small(3)).unwrap();
    let (sh, sc) = series(900, 300);
    w.write_chunk(&sh, &series_columns(&sc)).unwrap();
    w.defer_block(small(4)).unwrap();
    w.finish().unwrap();
    std::fs::read(path).unwrap()
}

/// A block a walk gave: offset, seq, kind, the chunk's payload offset in the file, intact.
type Seen = (u64, u64, BlockKind, Option<u64>, bool);

/// Every block of a file in order.
fn walk(bytes: &[u8], recovery: Recovery) -> (Vec<Seen>, usize) {
    let mut rd = FileBlocks::with_recovery(bytes, recovery);
    let (mut out, mut errors) = (vec![], 0);
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) => {
                let (seq, kind, intact) = (b.seq, b.kind, b.is_intact());
                let payload = b.chunk().map(|c| c.payload_offset() as u64);
                let off = rd.offset();
                out.push((off, seq, kind, payload.map(|p| p + off), intact));
            }
            Err(_) => errors += 1,
        }
        assert!(errors < 100_000, "the walk ends");
    }
    (out, errors)
}

#[test]
fn aligned_file_puts_payloads_on_pages_and_fills_gaps() {
    let dir = std::env::temp_dir().join(format!("ww_tape_aligned_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = write_tape(&dir.join("a.wwtape"), Layout::ALIGNED);
    let again = write_tape(&dir.join("b.wwtape"), Layout::ALIGNED);
    let plain = write_tape(&dir.join("c.wwtape"), Layout::UNALIGNED);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(file, again, "same input, same bytes");
    let (blocks, errors) = walk(&file, Recovery::Prefix);
    assert_eq!(errors, 0);
    // the describe backup, final index, finish and tail at the end carry the last counted seq
    let seqs: Vec<u64> = blocks.iter().map(|b| b.1).collect();
    assert_eq!(
        seqs,
        [0, 1, 1]
            .into_iter()
            .chain(2..8)
            .chain([7; 4])
            .collect::<Vec<_>>(),
        "seqs ascend"
    );
    let kinds: Vec<BlockKind> = blocks.iter().map(|b| b.2).collect();
    use BlockKind::*;
    // the writer's describe copy after the first data block; the deferred blocks 1 and 2 fill the gap before the
    // events chunk, 3 that before the second series chunk; 4 has no chunk after it and goes at the end
    // then the writer's describe backup, final index, finish and tail
    assert_eq!(
        kinds,
        [
            Describe, Series, Describe, Records, Records, Events, Records, Series, Records,
            Describe, Index, Finish, Tail
        ]
    );
    for (_, _, _, payload, _) in &blocks {
        if let Some(p) = payload {
            assert_eq!(p % 4096, 0);
        }
    }
    let (blocks, _) = walk(&plain, Recovery::Prefix);
    let kinds: Vec<BlockKind> = blocks.iter().map(|b| b.2).collect();
    assert_eq!(
        kinds,
        [
            Describe, Series, Describe, Records, Records, Events, Records, Series, Records,
            Describe, Index, Finish, Tail
        ]
    );
    assert!(plain.len() < file.len());
}

/// A file cut at every byte reads as its blocks before the cut, aligned or not.
#[test]
fn chunk_file_cut_anywhere_reads_its_longest_good_prefix() {
    let dir = std::env::temp_dir().join(format!("ww_tape_cut_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for layout in [Layout::UNALIGNED, Layout::ALIGNED] {
        let file = write_tape(&dir.join("t.wwtape"), layout);
        let (blocks, _) = walk(&file, Recovery::Prefix);
        let ends: Vec<usize> = blocks
            .iter()
            .skip(1)
            .map(|b| b.0 as usize)
            .chain([file.len()])
            .collect();
        for at in 0..=file.len() {
            let whole = ends.iter().filter(|e| **e <= at).count();
            let (got, errors) = walk(&file[..at], Recovery::Prefix);
            assert_eq!(got.len(), whole, "{layout:?} cut at {at}");
            assert_eq!(
                errors,
                usize::from(at != 0 && !ends.contains(&at)),
                "cut at {at}"
            );
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In an aligned file, a flipped bit in a series payload costs that chunk's one segment; a flipped bit in a head
/// costs that block, and the walk goes on at the next one.
#[test]
fn resync_salvages_chunks_in_a_file() {
    let dir = std::env::temp_dir().join(format!("ww_tape_salvage_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = write_tape(&dir.join("t.wwtape"), Layout::ALIGNED);
    std::fs::remove_dir_all(&dir).unwrap();
    let (blocks, _) = walk(&file, Recovery::Prefix);
    let series = blocks[1];
    let payload = series.3.unwrap() as usize;
    let mut bad = file.clone();
    bad[payload + 100] ^= 0x01;
    let (got, errors) = walk(&bad, Recovery::Resync);
    assert_eq!(errors, 0);
    assert_eq!(got.len(), blocks.len());
    assert_eq!(got.iter().filter(|b| !b.4).count(), 1);
    let mut rd = FileBlocks::with_recovery(&bad[..], Recovery::Resync);
    let mut lost = vec![];
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        if !b.is_intact() {
            lost.extend(b.chunk().unwrap().bad_segments());
        }
    }
    assert_eq!(lost, [0]);
    // next_good leaves it out and says so
    let mut rd = FileBlocks::with_recovery(&bad[..], Recovery::Resync);
    let mut reported = 0;
    let mut good = 0;
    while rd
        .next_good(|e| {
            assert!(matches!(e, FileError::Bad { .. }));
            reported += 1;
        })
        .is_some()
    {
        good += 1;
    }
    assert_eq!((good, reported), (blocks.len() - 1, 1));

    // the head of the events chunk: that block is lost, every other is read
    let events = blocks[4];
    let mut bad = file.clone();
    bad[events.0 as usize + 40] ^= 0x01;
    let (got, errors) = walk(&bad, Recovery::Resync);
    assert_eq!(errors, 1);
    let seqs: Vec<u64> = got.iter().map(|b| b.1).collect();
    let want: Vec<u64> = blocks
        .iter()
        .map(|b| b.1)
        .filter(|s| *s != events.1)
        .collect();
    assert_eq!(seqs, want);
    // the prefix reader stops at the damaged chunk
    let (got, errors) = walk(&bad, Recovery::Prefix);
    assert_eq!((got.len(), errors), (4, 1));
}

/// The aligned sample tape is the golden file's (`WW_TAPE_GOLDEN=update` rewrites it): one block per line, in hex.
#[test]
fn chunks_golden() {
    let dir = std::env::temp_dir().join(format!("ww_tape_chunks_golden_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = write_tape(&dir.join("g.wwtape"), Layout::ALIGNED);
    std::fs::remove_dir_all(&dir).unwrap();
    let mut hex = String::new();
    let mut rd = FileBlocks::new(&file[..]);
    while let Some(b) = rd.next_block() {
        for byte in b.unwrap().bytes() {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex.push('\n');
    }
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-chunks.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-chunks.hex: run with WW_TAPE_GOLDEN=update");
    assert!(
        hex == want,
        "the format changed: bytes differ from golden/v2-chunks.hex"
    );
}
