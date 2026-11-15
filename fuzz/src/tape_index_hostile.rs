//! A ww_tape index, finish, piece head or tail block of hostile offsets, directory or entries
//! (ww_tape_fuzz::hostile::index), read alone and as a file.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::index_hostile(data));
