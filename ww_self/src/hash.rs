//! The API hash of a serialized bundle, without std: a firmware hashes the model a module hands it (SRV-81).

use sha2::Digest;

/// Length in bytes of one signature (truncated SHA-256), same as the API hash.
pub const SIGNATURE_LEN: usize = 8;

/// API hash (`API_HASH_NO_DOCS` / `API_HASH_WITH_DOCS`) of a serialized bundle, as a device sends it:
/// truncated SHA-256 over the bytes, `ww_self_version` included, so it changes with the introspection data.
pub fn api_hash(bundle_bytes: &[u8]) -> [u8; SIGNATURE_LEN] {
    let mut hash = [0u8; SIGNATURE_LEN];
    hash.copy_from_slice(&sha2::Sha256::digest(bundle_bytes)[..SIGNATURE_LEN]);
    hash
}
