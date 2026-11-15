//! The ww_tape file reader on any bytes: it never panics, every block it gives is a good one (or, in the salvage walk,
//! a chunk with a good head), and the prefix walk is the start of what the salvage walk gives. Describe records of a good block decode or fail, never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use shrink_wrap::DeserializeShrinkWrap;
use ww_tape::{Block, Describe, FileBlocks, Recovery};

fn walk(data: &[u8], recovery: Recovery) -> Vec<Vec<u8>> {
    let mut rd = FileBlocks::with_recovery(data, recovery);
    let mut blocks = vec![];
    let mut steps = 0usize;
    while let Some(b) = rd.next_block() {
        if let Ok(b) = b {
            let bytes = b.bytes().to_vec();
            if b.is_intact() {
                assert!(Block::read(&bytes).is_ok());
            } else {
                assert_eq!(recovery, Recovery::Resync);
                assert!(Block::salvage(&bytes).is_ok_and(|b| !b.is_intact()));
            }
            for r in b.records().flatten() {
                if r.at.is_none() {
                    _ = Describe::from_ww_bytes(r.item);
                }
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
