//! ww_tape chunk blocks on any bytes, and on chunks built from the input with one byte flipped: reading and salvaging
//! never panic, a salvaged chunk's head reads back, a byte flipped in the payload of a good chunk costs exactly the
//! segment it is in, and a block given another run and seq stays good.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::chunk(data));
