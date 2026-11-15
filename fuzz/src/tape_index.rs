//! ww_tape index, finish, tail and piece head blocks and the positional reader on any bytes: decoding never panics,
//! a reader opens any bytes and every entry it gives lies in its piece; index blocks built from the input read back
//! equal, a final one also track by track from its head.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ww_tape::index::{
    IndexBlock, IndexEntry, IndexHead, IndexKind, index_len, track_count, write_index,
};
use ww_tape::piece::{FinishBlock, PieceHeadBlock};
use ww_tape::{Block, BlockKind, Tail, TapeReader};

/// Look at everything the block decoders give for `data`.
fn decode(data: &[u8]) {
    if let Ok(len) = IndexBlock::head_len(data) {
        if let Ok(ix) = IndexBlock::read_head(&data[..len.min(data.len())]) {
            for t in ix.tracks() {
                let (from, n) = ix.track_range(&t);
                if let Some(bytes) = data.get(from..from.saturating_add(n)) {
                    if let Ok(it) = ix.track_entries(&t, bytes) {
                        it.for_each(drop);
                    }
                }
            }
        }
    }
    if let Ok(b) = Block::read(data) {
        if let Ok(ix) = IndexBlock::read(&b) {
            assert_eq!(ix.entries().count(), ix.entry_count());
            let _ = ix.track(0);
        }
        if let Ok(f) = FinishBlock::read(&b) {
            let _ = f.finish();
            f.tracks().for_each(drop);
        }
        if let Ok(h) = PieceHeadBlock::read(&b) {
            let _ = h.head();
            h.tracks().for_each(drop);
        }
    }
    let _ = Tail::read(data);
}

fn read(data: &[u8]) {
    let Ok(r) = TapeReader::open(data) else {
        return;
    };
    let mut end = 0;
    for (i, p) in r.pieces().iter().enumerate() {
        assert!(p.start >= end && p.end >= p.start && p.end <= data.len() as u64);
        end = p.end;
        if let Ok(entries) = r.entries(i) {
            for e in &entries {
                assert!(p.start + e.offset < data.len() as u64);
                let mut buf = vec![];
                let _ = r.read_block(i, e, &mut buf);
            }
        }
        let _ = r.track_entries(i, 0);
        let _ = r.verify(i);
    }
    let _ = r.chain();
}

fuzz_target!(|data: &[u8]| {
    decode(data);
    read(data);
    // index blocks of entries made from the input: [final?][entries of 9 bytes: track u8, offset u64]
    let Some((&flag, rest)) = data.split_first() else {
        return;
    };
    let mut entries: Vec<IndexEntry> = rest
        .chunks_exact(9)
        .map(|c| IndexEntry {
            offset: u64::from_le_bytes(c[1..9].try_into().unwrap()),
            len: c[0].into(),
            kind: BlockKind::from_u8(c[0]),
            level: c[0],
            tree: 0,
            track_id: u64::from(c[0] % 5),
            track_number: c[0].into(),
            first_item: 0,
            count: 1,
            first_tick: -1,
            last_tick: 1,
            latest_end: 2,
        })
        .collect();
    let kind = if flag & 1 == 1 {
        entries.sort_by_key(|e| (e.track_id, e.offset));
        entries.dedup_by_key(|e| (e.track_id, e.offset));
        IndexKind::Final
    } else {
        IndexKind::Checkpoint
    };
    let tracks = if kind == IndexKind::Final {
        track_count(&entries)
    } else {
        0
    };
    let mut buf = vec![0u8; index_len(entries.len(), tracks)];
    let head = IndexHead {
        kind,
        self_offset: u64::from(flag),
        prev: (flag & 2 == 2).then_some(7),
    };
    let n = write_index(
        &mut buf,
        ww_tape::BlockId::new(ww_tape::RunTag::writable(1), 1),
        &head,
        &entries,
    )
    .expect("a valid index");
    let b = Block::read(&buf[..n]).expect("written good");
    let ix = IndexBlock::read(&b).expect("an index");
    assert_eq!(ix.entries().collect::<Vec<_>>(), entries);
    assert_eq!(
        (ix.kind(), ix.self_offset(), ix.prev()),
        (kind, head.self_offset, head.prev)
    );
    let len = IndexBlock::head_len(&buf[..ww_tape::index::HEAD_LEN_PREFIX]).expect("a head");
    let only = IndexBlock::read_head(&buf[..len]).expect("a head");
    let mut again = vec![];
    for t in only.tracks() {
        let (from, n) = only.track_range(&t);
        again.extend(
            only.track_entries(&t, &buf[from..from + n])
                .expect("good entries"),
        );
    }
    if kind == IndexKind::Final {
        assert_eq!(again, entries);
    }
});
