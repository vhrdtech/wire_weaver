//! A ww_tape events chunk whose payload lengths, lz4 length or codecs take hostile values
//! (ww_tape_fuzz::hostile::payloads), its payloads read and salvaged.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::payloads_hostile(data));
