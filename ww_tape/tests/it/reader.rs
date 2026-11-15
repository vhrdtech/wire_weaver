//! The positional reader over crafted bytes: offsets from the file never panic or loop.

use ww_tape::index::IndexEntry;
use ww_tape::piece::{ContentHash, Finish, FinishState, TAIL_LEN, Tail, finish_len, write_finish};
use ww_tape::reader::ReadError;
use ww_tape::{BlockKind, TapeReader};

fn tail(piece_len: u64, finish_offset: u64, finish_len: u32) -> [u8; TAIL_LEN] {
    let t = Tail {
        piece_len,
        finish_offset,
        finish_len,
        state: FinishState::Complete,
        hash: ContentHash([0; 32]),
        ident: None,
        next: None,
    };
    let mut b = [0u8; TAIL_LEN];
    t.write(&mut b, 0);
    b
}

#[test]
fn tail_with_wrapping_offsets_is_no_tail() {
    let bytes = tail(TAIL_LEN as u64, u64::MAX - 343, 180);
    let r = TapeReader::open(&bytes[..]).unwrap();
    assert!(r.pieces().iter().all(|p| p.end <= TAIL_LEN as u64));
}

/// A finished piece of a finish block naming `final_index` and its tail, nothing else.
fn finish_only(final_index: u64) -> Vec<u8> {
    let finish = Finish {
        state: FinishState::Complete,
        hash: ContentHash([0; 32]),
        checkpoint: None,
        final_index: Some(final_index),
        describe: None,
        ident: None,
        next: None,
    };
    let mut bytes = vec![0u8; finish_len(0)];
    let n = write_finish(&mut bytes, 0, &finish, &[]).unwrap();
    bytes.truncate(n);
    bytes.extend(tail((n + TAIL_LEN) as u64, 0, n as u32));
    bytes
}

#[test]
fn final_index_past_the_piece_is_an_error() {
    for at in [u64::MAX - 10, u64::MAX - 40, 1 << 40, 8] {
        let bytes = finish_only(at);
        let r = TapeReader::open(&bytes[..]).unwrap();
        assert_eq!(r.pieces().len(), 1, "final index at {at}");
        assert!(r.pieces()[0].finish.is_some());
        assert!(r.entries(0).is_err(), "final index at {at}");
        assert!(r.track_entries(0, 1).is_err(), "final index at {at}");
    }
}

#[test]
fn entry_past_the_piece_is_an_error() {
    let bytes = finish_only(0);
    let r = TapeReader::open(&bytes[..]).unwrap();
    let mut buf = Vec::new();
    for (offset, len) in [(u64::MAX - 10, 64), (u64::MAX, 1), (0, u32::MAX), (8, 400)] {
        let entry = IndexEntry {
            offset,
            len,
            kind: BlockKind::Series,
            level: 0,
            tree: 0,
            track_id: 1,
            track_number: 1,
            first_item: 0,
            count: 0,
            first_tick: 0,
            last_tick: 0,
            latest_end: 0,
        };
        assert!(
            matches!(
                r.read_block(0, &entry, &mut buf),
                Err(ReadError::Block { .. })
            ),
            "entry at {offset}, {len} bytes"
        );
    }
}
