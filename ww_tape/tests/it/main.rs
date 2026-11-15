//! Integration tests of ww_tape, through the public API.

mod pieces;
mod reader;
mod series;

use ww_tape::{BlockId, RunTag};

/// The run of blocks made by the tests.
const TEST_RUN: RunTag = RunTag::writable(0x7465_7374);

/// Block `seq` of [TEST_RUN].
fn id(seq: u64) -> BlockId {
    BlockId::new(TEST_RUN, seq)
}
