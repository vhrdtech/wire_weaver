//! A 32-bit hash of a trait's crate and name (SRV-53): a device without introspection finds the trait a
//! `GlobalFull` path names by it, in a table its server was generated with, so no crate or trait names are in its
//! flash. The device hashes the crate id and trait name of the request, codegen hashes those of every trait it can
//! reach, and refuses an API in which two traits hash the same.
//!
//! FNV-1a, 32 bits: one multiply per byte, no 64-bit arithmetic (a Cortex-M0 has no `UMULL`, a 64-bit FNV would
//! call `__aeabi_lmul` per byte). Not a cryptographic hash: a request still has to match the trait's version, and a
//! collision within one API is a build error.

const OFFSET_BASIS: u32 = 0x811c_9dc5;
const PRIME: u32 = 0x0100_0193;

/// Byte between the crate id and the trait name: never part of UTF-8 text, so `("ab", "c")` and `("a", "bc")` hash
/// differently.
const SEPARATOR: u8 = 0xff;

/// Hash of the trait `trait_name` of crate `crate_id` (the crate name, as in a `FullVersion`).
pub const fn trait_hash(crate_id: &str, trait_name: &str) -> u32 {
    let h = fnv1a(OFFSET_BASIS, crate_id.as_bytes());
    let h = fnv1a(h, &[SEPARATOR]);
    fnv1a(h, trait_name.as_bytes())
}

const fn fnv1a(mut h: u32, bytes: &[u8]) -> u32 {
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_reference_values() {
        // the published FNV-1a 32 test vectors
        assert_eq!(fnv1a(OFFSET_BASIS, b""), 0x811c_9dc5);
        assert_eq!(fnv1a(OFFSET_BASIS, b"a"), 0xe40c_292c);
        assert_eq!(fnv1a(OFFSET_BASIS, b"foobar"), 0xbf9c_f968);
    }

    #[test]
    fn crate_and_trait_are_separated() {
        assert_ne!(trait_hash("ab", "c"), trait_hash("a", "bc"));
        assert_eq!(
            trait_hash("ww_dfu", "BootControl"),
            fnv1a(
                fnv1a(fnv1a(OFFSET_BASIS, b"ww_dfu"), &[0xff]),
                b"BootControl"
            )
        );
    }

    #[test]
    fn usable_in_const() {
        const H: u32 = trait_hash("ww_dfu", "BootControl");
        assert_eq!(H, trait_hash("ww_dfu", "BootControl"));
    }
}
