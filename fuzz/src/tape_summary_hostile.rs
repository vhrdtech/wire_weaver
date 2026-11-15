//! A ww_tape summary block whose head, scale or node values take hostile values (ww_tape_fuzz::hostile::summary), every
//! node read.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::summary_hostile(data));
