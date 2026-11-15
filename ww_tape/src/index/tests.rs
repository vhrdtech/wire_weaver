use super::*;
use crate::{BlockWriter, ClockTime, EndMode};

fn entry(track_id: u64, offset: u64) -> IndexEntry {
    IndexEntry {
        offset,
        len: 100 + offset as u32,
        kind: BlockKind::Events,
        level: 0,
        tree: 0,
        track_id,
        track_number: track_id as u32 + 1,
        first_item: offset * 10,
        count: 10,
        first_tick: -(offset as i64),
        last_tick: offset as i64,
        latest_end: offset as i64 + 3,
    }
}

fn write(head: &IndexHead, entries: &[IndexEntry]) -> Vec<u8> {
    let tracks = match head.kind {
        IndexKind::Checkpoint => 0,
        IndexKind::Final => track_count(entries),
    };
    let mut buf = vec![0u8; index_len(entries.len(), tracks)];
    let n = write_index(&mut buf, 5, head, entries).unwrap();
    assert_eq!(n, buf.len());
    buf
}

#[test]
fn checkpoint_round_trip() {
    let entries: Vec<_> = (0..5).map(|i| entry(i % 2, 1000 + i * 300)).collect();
    let head = IndexHead {
        kind: IndexKind::Checkpoint,
        self_offset: 2500,
        prev: Some(400),
    };
    let bytes = write(&head, &entries);
    let block = Block::read(&bytes).unwrap();
    assert_eq!(
        (block.kind, block.seq, block.record_count),
        (BlockKind::Index, 5, 5)
    );
    let ix = IndexBlock::read(&block).unwrap();
    assert_eq!(ix.kind(), IndexKind::Checkpoint);
    assert_eq!((ix.self_offset(), ix.prev()), (2500, Some(400)));
    assert_eq!(ix.entries().collect::<Vec<_>>(), entries);
    assert_eq!(ix.tracks().count(), 0);
    assert_eq!(bytes.len(), 32 + 24 + 5 * 72 + 4);
    let first = IndexHead { prev: None, ..head };
    let ix_bytes = write(&first, &entries);
    assert_eq!(
        IndexBlock::read(&Block::read(&ix_bytes).unwrap())
            .unwrap()
            .prev(),
        None
    );
}

/// A reader of one track reads the head and that track's entries alone, each checked by its CRC.
#[test]
fn final_index_reads_one_track_from_its_head() {
    let mut entries: Vec<_> = (0..40)
        .map(|i| entry([0, 3, 7, 9][i as usize % 4], i * 64))
        .collect();
    entries.sort_by_key(|e| (e.track_id, e.offset));
    let head = IndexHead {
        kind: IndexKind::Final,
        self_offset: 9000,
        prev: None,
    };
    let bytes = write(&head, &entries);
    let ix = IndexBlock::read(&Block::read(&bytes).unwrap()).unwrap();
    assert_eq!(
        ix.tracks().map(|t| t.track_id).collect::<Vec<_>>(),
        [0, 3, 7, 9]
    );

    let head_len = IndexBlock::head_len(&bytes[..HEADER_LEN]).unwrap();
    assert_eq!(head_len, 32 + 24 + 4 * 24);
    let only_head = IndexBlock::read_head(&bytes[..head_len]).unwrap();
    assert_eq!(only_head.entries().count(), 0, "entries aren't in the head");
    let list = only_head.track(7).unwrap();
    assert_eq!((list.first, list.count), (20, 10));
    let (from, n) = only_head.track_range(&list);
    let got: Vec<_> = only_head
        .track_entries(&list, &bytes[from..from + n])
        .unwrap()
        .collect();
    assert_eq!(got, entries[20..30]);
    assert!(only_head.track(4).is_none());

    let mut bad = bytes[from..from + n].to_vec();
    bad[3] ^= 1;
    assert!(matches!(
        only_head.track_entries(&list, &bad),
        Err(IndexError::BadEntries)
    ));
    let mut head_bad = bytes[..head_len].to_vec();
    head_bad[60] ^= 1;
    assert_eq!(
        IndexBlock::read_head(&head_bad).unwrap_err(),
        IndexError::BadHeadCrc
    );
}

#[test]
fn final_index_must_be_sorted() {
    let entries = [entry(3, 0), entry(1, 64)];
    let head = IndexHead {
        kind: IndexKind::Final,
        self_offset: 0,
        prev: None,
    };
    let mut buf = vec![0u8; 4096];
    assert!(matches!(
        write_index(&mut buf, 0, &head, &entries),
        Err(IndexError::Invalid(_))
    ));
    let short = &mut [0u8; 40][..];
    assert!(matches!(
        write_index(short, 0, &head, &[]),
        Err(IndexError::Full { .. })
    ));
}

#[test]
fn entries_of_blocks() {
    let mut buf = [0u8; 128];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, 3, &[1]);
    w.push(2, ClockTime::new(1, 5), &[b"x"]).unwrap();
    w.push(4, ClockTime::new(1, 6), &[b"y"]).unwrap();
    let len = w.finish();
    let e = IndexEntry::of_block(77, &Block::read(&buf[..len]).unwrap()).unwrap();
    assert_eq!(
        (e.offset, e.len, e.kind, e.track_id, e.count),
        (77, len as u32, BlockKind::Records, 0, 2)
    );

    let head = crate::ChunkHead {
        kind: BlockKind::Events,
        track_id: 0xfeed,
        track_number: 2,
        first_item: 4096,
        count: 3,
        first_tick: 10,
        last_tick: 30,
        latest_end: 45,
        time_mode: 0,
        end_mode: EndMode::Durations,
    };
    let col = [0u8; 24];
    let cols = [(
        crate::ColumnSpec {
            kind: crate::ColumnKind::Starts,
            codec: crate::Codec::RAW,
            param: 0,
            len: 24,
        },
        &col[..],
    )];
    let mut cb = vec![0u8; crate::chunk::chunk_len(&[cols[0].0], 0)];
    let n = crate::chunk::write_chunk(&mut cb, 1, &head, &cols, 0).unwrap();
    let e = IndexEntry::of_block(4000, &Block::read(&cb[..n]).unwrap()).unwrap();
    assert_eq!(e, IndexEntry::of_chunk(4000, n, &head));
    assert_eq!(
        (
            e.track_id,
            e.first_item,
            e.count,
            e.first_tick,
            e.last_tick,
            e.latest_end
        ),
        (0xfeed, 4096, 3, 10, 30, 45)
    );

    // a summary block's head fields, at the offsets the summary blocks put them
    let mut sb = vec![0u8; 100];
    put_header(&mut sb, BlockKind::Other(SUMMARY_KIND), 100, 2, 9);
    sb[32..40].copy_from_slice(&0xabcu64.to_le_bytes());
    put_u32(&mut sb, 40, 5);
    sb[44] = 2;
    sb[45] = 3;
    sb[56..64].copy_from_slice(&512u64.to_le_bytes());
    sb[64..72].copy_from_slice(&1024u64.to_le_bytes());
    sb[72..80].copy_from_slice(&(-7i64).to_le_bytes());
    sb[80..88].copy_from_slice(&70i64.to_le_bytes());
    sb[88..96].copy_from_slice(&75i64.to_le_bytes());
    seal(&mut sb);
    let e = IndexEntry::of_block(10, &Block::read(&sb).unwrap()).unwrap();
    assert_eq!(
        (
            e.kind.to_u8(),
            e.track_id,
            e.track_number,
            e.tree,
            e.level,
            e.first_item,
            e.count
        ),
        (6, 0xabc, 5, 2, 3, 512, 1024)
    );
    assert_eq!((e.first_tick, e.last_tick, e.latest_end), (-7, 70, 75));

    let ix = write(
        &IndexHead {
            kind: IndexKind::Checkpoint,
            self_offset: 0,
            prev: None,
        },
        &[],
    );
    assert!(
        IndexEntry::of_block(0, &Block::read(&ix).unwrap()).is_none(),
        "an index lists no index"
    );
}

#[test]
fn damaged_heads_are_refused() {
    let entries: Vec<_> = (0..3).map(|i| entry(1, i * 100)).collect();
    let bytes = write(
        &IndexHead {
            kind: IndexKind::Final,
            self_offset: 0,
            prev: None,
        },
        &entries,
    );
    for at in 24..bytes.len() - 4 {
        let mut b = bytes.clone();
        b[at] ^= 0x10;
        // the block CRC is rewritten: only the head CRC and the ranges stand between the damage and the reader
        seal(&mut b);
        if let Ok(block) = Block::read(&b) {
            let r = IndexBlock::read(&block);
            if at < 32 + 24 + 24 {
                assert!(r.is_err(), "head byte {at}");
            }
        }
    }
}

/// Renumbering an index block rewrites its head CRC too, so its head still reads.
#[test]
fn renumbered_index_block_reads() {
    let entries: Vec<_> = (0..3).map(|i| entry(i, 100 * i)).collect();
    for kind in [IndexKind::Checkpoint, IndexKind::Final] {
        let head = IndexHead {
            kind,
            self_offset: 900,
            prev: None,
        };
        let mut bytes = write(&head, &entries);
        crate::set_seq(&mut bytes, 77).unwrap();
        let block = Block::read(&bytes).unwrap();
        assert_eq!(block.seq, 77);
        assert_eq!(IndexBlock::read(&block).unwrap().entry_count(), 3);
        let head_len = IndexBlock::head_len(&bytes).unwrap();
        assert!(IndexBlock::read_head(&bytes[..head_len]).is_ok());
    }
}

/// Index, finish, tail and piece head blocks are the writer's own: not taken as deferred blocks.
#[test]
fn structural_blocks_are_not_deferred() {
    let dir = std::env::temp_dir().join(format!("ww_tape_index_defer_{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut w = crate::TapeWriter::create(&dir.join("t.wwt"), crate::SyncPolicy::None).unwrap();
    let head = IndexHead {
        kind: IndexKind::Checkpoint,
        self_offset: 0,
        prev: None,
    };
    let index = write(&head, &[entry(1, 0)]);
    assert!(matches!(
        w.defer_block(index),
        Err(crate::WriteError::Index(IndexError::Invalid(_)))
    ));
    let mut tail = [0u8; crate::piece::TAIL_LEN];
    crate::piece::Tail {
        piece_len: 1000,
        finish_offset: 0,
        finish_len: 200,
        state: crate::FinishState::Complete,
        hash: crate::piece::ContentHash([0; 32]),
        ident: None,
        next: None,
    }
    .write(&mut tail, 0);
    assert!(w.defer_block(tail.to_vec()).is_err());
    let mut buf = [0u8; 64];
    let mut b = BlockWriter::new(&mut buf, BlockKind::Records, 0, &[1]);
    b.push(1, ClockTime::new(1, 5), &[b"x"]).unwrap();
    let len = b.finish();
    w.defer_block(buf[..len].to_vec()).unwrap();
    w.finish().unwrap();
    _ = std::fs::remove_dir_all(&dir);
}
