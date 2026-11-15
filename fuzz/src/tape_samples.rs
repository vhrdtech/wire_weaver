//! ww_tape sample codecs and the lz4 payload codec on any bytes: reading a column never panics, and values built
//! from the input code and decode back bit for bit, whole and from any sample on.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::samples(data));
