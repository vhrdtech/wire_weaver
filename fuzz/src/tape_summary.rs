//! ww_tape summary blocks on any bytes, and trees built from the input: reading never panics, every accessor of a
//! good summary block stays in bounds, and every block a builder writes reads back with its nodes covering the items,
//! also across prefix changes of a series, whose nodes then merge at a common scale.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::summary(data));
