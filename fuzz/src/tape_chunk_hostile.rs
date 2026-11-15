//! A ww_tape chunk block whose head takes hostile values (ww_tape_fuzz::hostile::chunk), read, salvaged and renumbered.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::chunk_hostile(data));
