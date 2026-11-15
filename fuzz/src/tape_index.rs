//! ww_tape index, finish, tail and piece head blocks and the positional reader on any bytes: decoding never panics,
//! a reader opens any bytes and every entry it gives lies in its piece; index blocks built from the input read back
//! equal, a final one also track by track from its head.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::index(data));
