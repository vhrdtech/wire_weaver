//! ww_tape's writer resuming an unfinished piece cut and mutated by the input: it never panics, and a piece it resumes
//! takes more blocks and finishes whole, found by its tail and its content hash verified.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::resume(data));
