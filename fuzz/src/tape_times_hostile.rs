//! A ww_tape events chunk whose time group records take hostile values (ww_tape_fuzz::hostile::times), its times read
//! and salvaged.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::times_hostile(data));
