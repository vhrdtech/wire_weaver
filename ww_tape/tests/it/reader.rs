//! The positional reader over crafted bytes: offsets from the file never panic or loop.

use ww_tape::TapeReader;
use ww_tape::piece::{ContentHash, FinishState, TAIL_LEN, Tail};

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
