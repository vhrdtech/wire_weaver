#![no_main]

//! `ww_stream::batch` (API-41): a `Batcher` of items with a sequence number, a signal and discrete leaves, driven by
//! random pushes (one by one, timed, in blocks), polls with random clocks, ages and budgets, encodes into rooms of
//! random size, outbox pushes and removals, flushes and discards, under every codec policy. Every encoded batch fits
//! its room and decodes to the items pushed, in order; every item is encoded exactly once or counted as lost exactly
//! once; outbox entries always decode. The checks are in `ww_stream::batch::check` (public under `cargo fuzz`'s
//! `--cfg fuzzing`), shared with the unit tests.

use libfuzzer_sys::fuzz_target;
use ww_stream::batch::check;

fuzz_target!(|data: &[u8]| {
    check::run(data);
});
