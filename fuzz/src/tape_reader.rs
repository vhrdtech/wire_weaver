//! The ww_tape file reader on any bytes: it never panics, every block it gives is a good one, and the prefix walk is
//! the start of what the salvage walk gives.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ww_tape::{Block, FileBlocks, Recovery};

fn walk(data: &[u8], recovery: Recovery) -> Vec<Vec<u8>> {
    let mut rd = FileBlocks::with_recovery(data, recovery);
    let mut blocks = vec![];
    let mut steps = 0usize;
    while let Some(b) = rd.next_block() {
        if let Ok(b) = b {
            let bytes = b.bytes().to_vec();
            assert!(Block::read(&bytes).is_ok());
            for r in b.records() {
                let _ = r;
            }
            blocks.push(bytes);
        }
        steps += 1;
        assert!(steps <= data.len() + 1, "every step consumes bytes");
    }
    blocks
}

fuzz_target!(|data: &[u8]| {
    let prefix = walk(data, Recovery::Prefix);
    let salvage = walk(data, Recovery::Resync);
    assert!(salvage.len() >= prefix.len());
    // the prefix walk stops at the first damage; up to there both read the same blocks, unless it stopped at a seq
    // going back, which the salvage walk doesn't check
    assert_eq!(&salvage[..prefix.len()], &prefix[..]);
});
