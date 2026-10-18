use super::*;

#[test]
fn write_and_read_back() {
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Data, 7, 1_000);
    w.push(1, 1_000, &[b"ab"]).unwrap();
    w.push(300, 900, &[b"c", b"de"]).unwrap();
    w.push_with(2, 5_000, |b| {
        b[..3].copy_from_slice(b"xyz");
        Some(3)
    })
    .unwrap();
    assert_eq!(w.records(), 3);
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    assert_eq!(
        (block.kind, block.seq, block.t_base, block.record_count),
        (BlockKind::Data, 7, 1_000, 3)
    );
    let records: Vec<_> = block.records().map(|r| r.unwrap()).collect();
    assert_eq!(
        records,
        [
            Record {
                track: 1,
                t: 1_000,
                item: b"ab"
            },
            Record {
                track: 300,
                t: 900,
                item: b"cde"
            },
            Record {
                track: 2,
                t: 5_000,
                item: b"xyz"
            },
        ]
    );
    // header + (1+1+1+2) + (2+2+1+3: -100 ns is 2 bytes) + (1+2+1+3) + crc
    assert_eq!(len, 32 + 5 + 8 + 7 + 4);
}

#[test]
fn full_leaves_the_block_as_it_was() {
    let mut buf = [0u8; HEADER_LEN + CRC_LEN + 6];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Data, 0, 0);
    w.push(1, 0, &[b"abc"]).unwrap();
    assert_eq!(w.push(1, 0, &[b"ab"]), Err(Full));
    assert_eq!(w.push_with(1, 0, |_| None), Err(Full));
    let len = w.finish();
    assert_eq!(Block::read(&buf[..len]).unwrap().record_count, 1);
}

#[test]
fn push_with_big_item_moves_down() {
    let mut buf = vec![0u8; 4096];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Data, 0, 0);
    let item: Vec<u8> = (0..200).map(|i| i as u8).collect();
    w.push_with(1, 10, |b| {
        b[..200].copy_from_slice(&item);
        Some(200)
    })
    .unwrap();
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    let r = block.records().next().unwrap().unwrap();
    assert_eq!(r.item, &item[..]);
    assert_eq!(len, HEADER_LEN + 1 + 1 + 2 + 200 + CRC_LEN);
}

#[test]
fn corrupt_blocks_are_refused() {
    let mut buf = [0u8; 64];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, 0, 0);
    w.push(0, 0, &[b"x"]).unwrap();
    let len = w.finish();
    let mut bad = buf;
    bad[HEADER_LEN + 3] ^= 1;
    assert_eq!(Block::read(&bad[..len]).unwrap_err(), BlockError::BadCrc);
    assert_eq!(
        Block::read(&buf[..len - 1]).unwrap_err(),
        BlockError::Short { needed: len }
    );
    let mut bad = buf;
    bad[0] = b'X';
    assert_eq!(Block::read(&bad[..len]).unwrap_err(), BlockError::BadMagic);
}

#[test]
fn describe_records_round_trip() {
    let path = [UNib32(1), UNib32(3), UNib32(0)];
    let d = Describe::Track(TrackDef {
        number: UNib32(4),
        key: track_key(api_root_id("ww_tap", 0), &[1, 3, 0]),
        api: FullVersion::new("ww_tap", Version::new(0, 1, 0)),
        bundle: RefVec::Slice { slice: &[1, 2] },
        path: RefVec::Slice { slice: &path },
        label: "link[3].tx.frames",
        sticky: false,
    });
    let mut scratch = [0u8; 128];
    let bytes = d.to_ww_bytes(&mut scratch).unwrap();
    let back = Describe::from_ww_bytes(bytes).unwrap();
    assert_eq!(back, d);
}

#[test]
fn resync_skips_bad_and_reports_cut() {
    let mut file = Vec::new();
    for seq in 0..3u64 {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Data, seq, 0);
        w.push(1, seq, &[&[seq as u8; 5]]).unwrap();
        let len = w.finish();
        file.extend_from_slice(&buf[..len]);
    }
    let one = file.len() / 3;
    // damage the middle block, cut the last one
    file[one + HEADER_LEN + 2] ^= 0xff;
    file.truncate(file.len() - 3);
    let mut rd = FileBlocks::with_recovery(&file[..], Recovery::Resync);
    assert_eq!(rd.next_block().unwrap().unwrap().seq, 0);
    match rd.next_block().unwrap() {
        Err(FileError::Bad { offset, error, .. }) => {
            assert_eq!((offset, error), (one as u64, BlockError::BadCrc))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        rd.next_block().unwrap(),
        Err(FileError::Truncated { .. })
    ));
    assert!(rd.next_block().is_none());
}

/// A later version's block and a kind this reader doesn't know are checked, skipped by their length and given out
/// without records, not taken for damage; a describe variant it doesn't know reads as `Unknown`.
#[test]
fn unknown_versions_kinds_and_describe_variants_are_skipped() {
    let mut file = Vec::new();
    for (seq, version, kind) in [
        (0u64, VERSION, 1u8),
        (1, VERSION + 1, 1),
        (2, VERSION, 9),
        (3, VERSION, 1),
    ] {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Data, seq, 0);
        w.push(1, seq, &[&[seq as u8; 5]]).unwrap();
        let len = w.finish();
        buf[4] = version;
        buf[5] = kind;
        // a later version's body: not records
        if version != VERSION {
            buf[HEADER_LEN] = 0xff;
        }
        let crc = CRC.checksum(&buf[..len - CRC_LEN]);
        buf[len - CRC_LEN..len].copy_from_slice(&crc.to_le_bytes());
        file.extend_from_slice(&buf[..len]);
    }
    let mut rd = FileBlocks::new(&file[..]);
    let mut seen = vec![];
    while let Some(b) = rd.next_block() {
        let b = b.unwrap();
        seen.push((b.seq, b.body_known(), b.records().count()));
    }
    assert_eq!(
        seen,
        [(0, true, 1), (1, false, 0), (2, false, 0), (3, true, 1)]
    );
    assert_eq!(BlockKind::from_u8(9), BlockKind::Other(9));

    // a variant appended later, as a newer producer writes it
    #[derive_shrink_wrap(derive(Debug), ww_repr = u4)]
    enum NewerDescribe<'i> {
        Producer(Producer<'i>),
        Track(TrackDef<'i>),
        Bundle {
            hash: RefVec<'i, u8>,
            bytes: RefVec<'i, u8>,
        },
        Index {
            first_seq: u64,
        },
    }
    let mut scratch = [0u8; 32];
    let bytes = NewerDescribe::Index { first_seq: 7 }
        .to_ww_bytes(&mut scratch)
        .unwrap();
    assert!(matches!(
        Describe::from_ww_bytes(bytes),
        Ok(Describe::Unknown(3))
    ));
}

/// `n` data blocks of different sizes back to back, and where each ends.
fn tape(n: u64) -> (Vec<u8>, Vec<usize>) {
    let mut file = Vec::new();
    let mut ends = vec![];
    for seq in 0..n {
        let mut buf = [0u8; 128];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Data, seq, seq * 10);
        for i in 0..=seq % 3 {
            w.push(
                1 + i as u32,
                seq * 10 + i,
                &[&[seq as u8; 7][..(seq as usize % 7) + 1]],
            )
            .unwrap();
        }
        let len = w.finish();
        file.extend_from_slice(&buf[..len]);
        ends.push(file.len());
    }
    (file, ends)
}

/// Seqs of the blocks a reader gives, and whether it reported a cut or damage.
fn read_all(bytes: &[u8], recovery: Recovery) -> (Vec<u64>, usize) {
    let mut rd = FileBlocks::with_recovery(bytes, recovery);
    let (mut seqs, mut errors) = (vec![], 0);
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) => seqs.push(b.seq),
            Err(_) => errors += 1,
        }
        assert!(errors < 10_000, "the walk ends");
    }
    (seqs, errors)
}

/// A capture cut at every byte reads as the blocks before the cut, the cut said when it is inside a block.
#[test]
fn prefix_of_a_capture_cut_anywhere() {
    let (file, ends) = tape(8);
    for at in 0..=file.len() {
        let whole = ends.iter().filter(|e| **e <= at).count() as u64;
        let at_boundary = at == 0 || ends.contains(&at);
        let (seqs, errors) = read_all(&file[..at], Recovery::Prefix);
        assert_eq!(seqs, (0..whole).collect::<Vec<_>>(), "cut at {at}");
        assert_eq!(errors, usize::from(!at_boundary), "cut at {at}");
        // the salvage reader doesn't panic either, and loses nothing before the cut
        let (seqs, _) = read_all(&file[..at], Recovery::Resync);
        assert_eq!(seqs, (0..whole).collect::<Vec<_>>(), "resync, cut at {at}");
    }
}

/// A flipped byte anywhere: the blocks before its block, then one cut; the salvage reader gets every other block.
#[test]
fn prefix_of_a_capture_with_a_flipped_byte() {
    let (file, ends) = tape(8);
    for at in 0..file.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut bad = file.clone();
            bad[at] ^= flip;
            let block = ends.iter().position(|e| at < *e).unwrap() as u64;
            let (seqs, errors) = read_all(&bad, Recovery::Prefix);
            assert_eq!(
                seqs,
                (0..block).collect::<Vec<_>>(),
                "flip {flip:#x} at {at}"
            );
            assert_eq!(errors, 1, "flip {flip:#x} at {at}");
            let (seqs, _) = read_all(&bad, Recovery::Resync);
            let others: Vec<u64> = (0..8).filter(|s| *s != block).collect();
            assert_eq!(seqs, others, "resync, flip {flip:#x} at {at}");
        }
    }
}

/// Zeros after the last block (preallocated space) are the end; anything else there is a cut.
#[test]
fn zeros_at_the_end_are_the_end() {
    let (mut file, _) = tape(3);
    file.extend_from_slice(&[0u8; 100_000]);
    assert_eq!(read_all(&file, Recovery::Prefix), (vec![0, 1, 2], 0));
    file.push(1);
    assert_eq!(read_all(&file, Recovery::Prefix), (vec![0, 1, 2], 1));
}

/// A data block whose seq goes back is a cut; a describe block may go back (another producer run appended).
#[test]
fn seq_going_back() {
    let block = |kind, seq| {
        let mut buf = [0u8; 64];
        let mut w = BlockWriter::new(&mut buf, kind, seq, 0);
        w.push(1, 0, &[b"x"]).unwrap();
        let len = w.finish();
        buf[..len].to_vec()
    };
    let file: Vec<u8> = [
        block(BlockKind::Data, 5),
        block(BlockKind::Data, 6),
        block(BlockKind::Describe, 0),
        block(BlockKind::Data, 1),
        block(BlockKind::Data, 1),
        block(BlockKind::Data, 2),
    ]
    .concat();
    let mut rd = FileBlocks::new(&file[..]);
    let mut seqs = vec![];
    let mut cut = None;
    while let Some(b) = rd.next_block() {
        match b {
            Ok(b) => seqs.push(b.seq),
            Err(FileError::Cut { why, .. }) => cut = Some(why),
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(seqs, [5, 6, 0, 1]);
    assert_eq!(cut, Some(Cut::SeqBackwards { seq: 1, after: 1 }));
}

#[test]
fn writer_partial_then_renamed() {
    let dir = std::env::temp_dir().join(format!("ww_tape_writer_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let (file, ends) = tape(4);
    let mut w = TapeWriter::create(&path, SyncPolicy::EveryBlocks(2)).unwrap();
    assert_eq!(w.current_path(), writer::partial_name(&path));
    let mut start = 0;
    for end in &ends {
        w.write_block(&file[start..*end]).unwrap();
        start = *end;
    }
    assert!(!path.exists(), "named only when finished");
    assert_eq!(std::fs::read(w.current_path()).unwrap(), file);
    assert_eq!((w.blocks(), w.bytes()), (4, file.len() as u64));
    assert_eq!(w.finish().unwrap(), path);
    assert_eq!(std::fs::read(&path).unwrap(), file);
    assert!(!writer::partial_name(&path).exists());
    assert!(matches!(
        TapeWriter::create(&path, SyncPolicy::None),
        Err(WriteError::Exists(_))
    ));
    // appended: the second run's blocks start with a describe block
    let mut w = TapeWriter::append(&path, SyncPolicy::default()).unwrap();
    let mut buf = [0u8; 64];
    let len = BlockWriter::new(&mut buf, BlockKind::Describe, 0, 0).finish();
    w.write_block(&buf[..len]).unwrap();
    w.tick().unwrap();
    w.finish().unwrap();
    let (seqs, errors) = read_all(&std::fs::read(&path).unwrap(), Recovery::Prefix);
    assert_eq!((seqs, errors), (vec![0, 1, 2, 3, 0], 0));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A failed write is the end of the file: later writes and syncs are refused, never retried.
#[cfg(target_os = "linux")]
#[test]
fn writer_stops_after_a_failure() {
    let mut w = TapeWriter::append(std::path::Path::new("/dev/full"), SyncPolicy::None).unwrap();
    let (file, ends) = tape(1);
    // /dev/full takes nothing: the write fails with ENOSPC
    let first = w.write_block(&file[..ends[0]]);
    assert!(matches!(first, Err(WriteError::Io { .. })), "{first:?}");
    assert!(matches!(
        w.write_block(&file[..ends[0]]),
        Err(WriteError::Failed)
    ));
    assert!(matches!(w.sync(), Err(WriteError::Failed)));
    assert!(matches!(w.finish(), Err(WriteError::Failed)));
}

#[test]
fn varints() {
    use varint::*;
    for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
        let mut b = [0u8; 10];
        let n = put_uvar(&mut b, v);
        assert_eq!(n, uvar_len(v));
        assert_eq!(Reader(&b[..n]).uvar(), Some(v));
    }
    for v in [0i64, -1, 1, -64, 64, i64::MIN, i64::MAX] {
        let mut b = [0u8; 10];
        let n = put_ivar(&mut b, v);
        assert_eq!(n, ivar_len(v));
        assert_eq!(Reader(&b[..n]).ivar(), Some(v));
    }
}
