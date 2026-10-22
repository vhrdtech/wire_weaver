//! Binary form of saved API snapshots (`ww api save`).
//!
//! A snapshot is stored in git as RON for people to read, with the SHA-256 of the shrink_wrap bytes it was made from
//! in its header ([HASH_PREFIX]). Whoever builds with the snapshot (`wire_weaver_snapshots`'s build script) encodes
//! the RON bundle with [encode], compares [sha256_hex] of the result with [saved_hash] and embeds the bytes only, so
//! the RON round trip is proven lossless at build time and never parsed at runtime.

use crate::ApiBundleOwned;
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use shrink_wrap::prelude::*;

/// Start of the header line holding the hash, `// shrink_wrap sha256: <64 hex digits>`.
pub const HASH_PREFIX: &str = "// shrink_wrap sha256: ";

/// shrink_wrap bytes of a bundle.
pub fn encode(bundle: &ApiBundleOwned) -> Result<Vec<u8>> {
    bundle
        .to_ww_bytes_owned()
        .map_err(|e| anyhow!("encoding bundle: {e:?}"))
}

/// Bundle from [encode]d bytes.
pub fn decode(bytes: &[u8]) -> Result<ApiBundleOwned> {
    ApiBundleOwned::from_ww_bytes_owned(bytes).map_err(|e| anyhow!("decoding bundle: {e:?}"))
}

/// Lower case hex SHA-256 of [encode]d bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The header line of a snapshot with the hash of `bytes`, with a trailing newline.
pub fn hash_line(bytes: &[u8]) -> String {
    format!("{HASH_PREFIX}{}\n", sha256_hex(bytes))
}

/// Hash saved in the header of a snapshot's RON text, if it has one.
pub fn saved_hash(ron: &str) -> Option<&str> {
    ron.lines()
        .take_while(|line| line.starts_with("//"))
        .find_map(|line| line.strip_prefix(HASH_PREFIX))
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_line_is_found_in_header() {
        let line = hash_line(b"abc");
        assert_eq!(
            line,
            "// shrink_wrap sha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );
        let ron = format!("// x 0.1.0\n{line}(magic: 1)");
        assert_eq!(
            saved_hash(&ron),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(saved_hash("// x\n(magic: 1)"), None);
    }
}
