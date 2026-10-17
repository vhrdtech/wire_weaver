#![no_main]

//! `Series<T>` read dynamically (CLT-29), from the type tree a device's introspection data describes: items generated
//! from the input written by the typed `Series` and read back by `ww_self`'s dynamic decoder must equal the same
//! items read as a plain `Vec<T>`; the input itself into the typed and the dynamic decoder of several types gives the
//! same items or an error in both, never a panic, a hang or an allocation driven by the data. The checks are in
//! `ww_self::fuzz_series` (public under `cargo fuzz`'s `--cfg fuzzing`), shared with its unit tests.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ww_self::fuzz_series::fuzz_one(data);
});
