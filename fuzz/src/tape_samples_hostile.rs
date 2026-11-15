//! A ww_tape series track and chunk whose sample column, count or shape take hostile values
//! (ww_tape_fuzz::hostile::samples), its samples decoded.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::samples_hostile(data));
