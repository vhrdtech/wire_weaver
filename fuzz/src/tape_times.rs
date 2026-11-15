//! ww_tape's event time codec on any bytes: a chunk block's times read and salvage without panic; times built from
//! the input read back exactly and survive, without panic, a flipped bit and input bytes written over their columns
//! (a good head over a damaged payload, as salvage meets it); every bit-pack kernel decodes any bytes as the scalar
//! code does.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::times(data));
