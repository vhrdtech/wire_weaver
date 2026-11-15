//! ww_tape series tracks on any bytes: a describe record (a track with its kind) and series segment records decode
//! without panic; segments built from the input, then damaged by input bytes written over their records, give a
//! timeline whose kept segments hold its rules, and every tick, scale and chunk check on it answers without panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::series(data));
