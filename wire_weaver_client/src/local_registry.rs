//! Local cache of API bundles at `~/.wire_weaver/`.
//!
//! Filled by codegen (see `wire_weaver_core/src/local_registry.rs`) and by bundles downloaded from devices.
//! File names are `<crate>_<major>_<minor>_<patch>-<hash>[+docs].ron`, keep in sync with codegen.

use anyhow::{Result, anyhow};
use ron::ser::{PrettyConfig, to_string_pretty};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use tracing::{debug, warn};
use ww_self::ApiBundleOwned;
use ww_version::{ApiHashOwned, ApiHashPairOwned};

/// Load a bundle matching the hash pair, preferring the one with doc strings.
/// Any error is logged and treated as a cache miss.
pub(crate) fn load(hash: &ApiHashPairOwned) -> Option<ApiBundleOwned> {
    let candidates = [(&hash.with_docs, true), (&hash.no_docs, false)];
    for (hash, contains_docs) in candidates {
        if hash.hash.is_empty() {
            continue;
        }
        match load_inner(hash, contains_docs) {
            Ok(Some(bundle)) => return Some(bundle),
            Ok(None) => {}
            Err(e) => warn!("Failed to read API bundle from cache: {e:?}"),
        }
    }
    None
}

fn load_inner(hash: &ApiHashOwned, contains_docs: bool) -> Result<Option<ApiBundleOwned>> {
    let suffix = file_suffix(hash, contains_docs);
    let entries = match fs::read_dir(registry_path()?) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.ends_with(&suffix) {
            let contents = fs::read_to_string(entry.path())?;
            let api_bundle: ApiBundleOwned = ron::from_str(&contents)?;
            debug!("got API bundle from cache: {file_name}");
            return Ok(Some(api_bundle));
        }
    }
    Ok(None)
}

/// Cache a bundle downloaded from a device.
/// `ww_self_bytes` are hashed the same way as in codegen to find out whether they match the device reported
/// `hash` with or without docs, bundles matching neither are not cached.
pub(crate) fn store(api_bundle: &ApiBundleOwned, ww_self_bytes: &[u8], hash: &ApiHashPairOwned) {
    if let Err(e) = store_inner(api_bundle, ww_self_bytes, hash) {
        warn!("Failed to cache API bundle: {e:?}");
    }
}

fn store_inner(
    api_bundle: &ApiBundleOwned,
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
) -> Result<()> {
    let digest = ww_self::signature::api_hash(ww_self_bytes);
    let digest = &digest[..];
    let (hash, contains_docs) = if digest == hash.no_docs.hash.as_slice() {
        (&hash.no_docs, false)
    } else if !hash.with_docs.hash.is_empty() && digest == hash.with_docs.hash.as_slice() {
        (&hash.with_docs, true)
    } else {
        warn!(
            "Downloaded API bundle hash {} matches none of the device reported hashes {hash:?}, not caching it",
            hex::encode(digest)
        );
        return Ok(());
    };

    let api_crate = api_bundle.crate_version(api_bundle.root.crate_idx.0)?;
    let file_name = format!(
        "{}{}",
        api_crate.filename_friendly(),
        file_suffix(hash, contains_docs)
    );
    let registry_path = registry_path()?;
    let file_path = registry_path.join(&file_name);
    if matches!(fs::exists(&file_path), Ok(true)) {
        return Ok(());
    }
    fs::create_dir_all(&registry_path)?;
    let as_ron = to_string_pretty(api_bundle, PrettyConfig::new().compact_structs(true))?;
    fs::write(&file_path, as_ron)?;
    debug!("cached API bundle: {file_name}");
    Ok(())
}

fn file_suffix(hash: &ApiHashOwned, contains_docs: bool) -> String {
    let docs = if contains_docs { "+docs" } else { "" };
    format!("-{}{docs}.ron", hash)
}

fn registry_path() -> Result<PathBuf> {
    Ok(std::env::home_dir()
        .ok_or(anyhow!("no home directory"))?
        .join(".wire_weaver"))
}
