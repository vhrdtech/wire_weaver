//! A ww_tape records or describe block whose clock table, record heads, lengths or coded body take hostile values
//! (ww_tape_fuzz::hostile::records), read alone and walked as a file.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::records_hostile(data));
