#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unreachable,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "tests fail by panicking and compute expected values freely"
)]
//! Integration tests of ww_tape, through the public API.

mod media;
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
