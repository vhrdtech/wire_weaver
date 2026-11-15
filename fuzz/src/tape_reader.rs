//! The ww_tape file reader on any bytes: it never panics, every block it gives is a good one (or, in the salvage walk,
//! a chunk with a good head), and the prefix walk is the start of what the salvage walk gives. Describe records of a good block decode or fail, never panic.
//! A good records or describe block coded with each body codec decodes to the same records.
//! Walked as a raw medium with the gap rules of an SD card too: every step still consumes bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| ww_tape_fuzz::targets::reader(data));
