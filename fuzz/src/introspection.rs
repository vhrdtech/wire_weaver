#![no_main]

//! Introspection data (API-43): a bundle a device sends is untrusted. Bytes are read as a serialized bundle or as a
//! program that builds one with generic definitions, instances and parameters, valid or not, then checked,
//! instantiated and used by the dynamic decoder: an error or a result, never a panic, a hang, a stack overflow or an
//! allocation driven by the data. The steps are in `ww_self::fuzz_bundle` (public under `cargo fuzz`'s
//! `--cfg fuzzing`), shared with its unit test.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ww_self::fuzz_bundle::fuzz_one(data);
});
