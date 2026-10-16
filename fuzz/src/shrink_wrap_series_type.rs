#![no_main]

//! `Series<T>` (SW-60): items of a type with every kind of leaf and group (numbers, floats, bools, `Option`s,
//! strings, bytes, tuples, arrays, enums with and without data, a nested evolvable struct), generated from the
//! input as slowly changing values with jumps and switches, round-trip through every codec policy, borrowed and
//! owned and as a field; the input itself goes into the readers of several item types, which return an error or
//! items, never a panic, an endless loop or an allocation driven by the data. The checks are in
//! `shrink_wrap::series::fuzz_type` (public under `cargo fuzz`'s `--cfg fuzzing`), shared with the unit tests.

use libfuzzer_sys::fuzz_target;
use shrink_wrap::series::fuzz_type;

fuzz_target!(|data: &[u8]| {
    fuzz_type::fuzz_one(data);
});
