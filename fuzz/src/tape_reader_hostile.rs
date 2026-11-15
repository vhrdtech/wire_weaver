//! A ww_tape piece of hostile offsets, lengths, layout or stretches between blocks (ww_tape_fuzz::hostile::file),
//! opened by the positional reader and walked.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::reader_hostile(data));
