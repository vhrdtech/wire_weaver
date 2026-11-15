//! The ww_tape file reader on any bytes: it never panics, every block it gives is a good one (or, in the salvage walk,
//! a chunk with a good head), and the prefix walk is the start of what the salvage walk gives. Describe records of a good block decode or fail, never panic.
//! A good records or describe block coded with each body codec decodes to the same records.
//! Walked as a raw medium with the gap rules of an SD card too: every step still consumes bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use shrink_wrap::DeserializeShrinkWrap;
use ww_tape::{
    Block, BodyCodec, Describe, FileBlocks, Layout, Medium, Recovery, code_body, coded_len_max,
};

fn walk(data: &[u8], recovery: Recovery, medium: Medium) -> Vec<Vec<u8>> {
    let mut rd = FileBlocks::with_recovery(data, recovery).on(medium);
    if medium == Medium::Raw {
        rd = rd.with_layout(&Layout::UNALIGNED.on_sd_card());
    }
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
            let mut plain = Vec::new();
            let decoded = b.decode(&mut plain);
            for r in decoded.iter().flat_map(|d| d.records()).flatten() {
                if r.at.is_none() {
                    _ = Describe::from_ww_bytes(r.item);
                }
            }
            round_trip(&b);
            blocks.push(bytes);
        }
        steps += 1;
        assert!(steps <= data.len() + 1, "every step consumes bytes");
    }
    blocks
}

/// Code a plain records or describe block's body with each codec: it reads back to the same records.
fn round_trip(b: &Block<'_>) {
    if b.body_codec().is_some() || !b.is_intact() {
        return;
    }
    let records: Vec<_> = b.records().collect();
    for codec in [BodyCodec::STORED, BodyCodec::LZ4] {
        let mut out = vec![0u8; coded_len_max(b.len())];
        let Ok(n) = code_body(b.bytes(), codec, &mut out) else {
            continue;
        };
        let coded = Block::read(&out[..n]).expect("a coded block reads");
        assert_eq!(
            (coded.seq, coded.run, coded.record_count),
            (b.seq, b.run, b.record_count)
        );
        let mut plain = Vec::new();
        let decoded = coded.decode(&mut plain).expect("a coded block decodes");
        assert_eq!(decoded.records().collect::<Vec<_>>(), records);
    }
}

fuzz_target!(|data: &[u8]| {
    let prefix = walk(data, Recovery::Prefix, Medium::File);
    let salvage = walk(data, Recovery::Resync, Medium::File);
    walk(data, Recovery::Prefix, Medium::Raw);
    walk(data, Recovery::Resync, Medium::Raw);
    assert!(salvage.len() >= prefix.len());
    // the prefix walk stops at the first damage; up to there both read the same blocks, unless it stopped at a seq
    // going back, which the salvage walk doesn't check
    assert_eq!(&salvage[..prefix.len()], &prefix[..]);
});
