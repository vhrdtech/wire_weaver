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
fn file_blocks_skip_bad_and_report_cut() {
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
    let mut rd = FileBlocks::new(&file[..]);
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
