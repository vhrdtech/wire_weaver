use anyhow::anyhow;
use ron::ser::{PrettyConfig, to_string_pretty};
use std::fs;
use ww_self::ApiBundleOwned;

pub(crate) fn cache_api_bundle(api_bundle: &ApiBundleOwned, contains_docs: bool, hash: &[u8]) {
    if let Err(e) = cache_api_bundle_inner(api_bundle, contains_docs, hash) {
        eprintln!("Failed to cache API bundle: {}", e);
    }
}

fn cache_api_bundle_inner(
    api_bundle: &ApiBundleOwned,
    contains_docs: bool,
    hash: &[u8],
) -> anyhow::Result<()> {
    let api_crate = api_bundle.crate_version(api_bundle.root.crate_idx.0)?;
    if api_crate.crate_id == "crate" || api_crate.crate_id == "super" {
        // ignore tests
        return Ok(());
    }
    let hash = hex::encode(hash);
    let with_docs = if contains_docs { "+docs" } else { "" };
    let filename = format!("{}-{hash}{with_docs}.ron", api_crate.filename_friendly());
    let local_registry_path = std::env::home_dir()
        .ok_or(anyhow!("no home directory"))?
        .join(".wire_weaver");
    let file_path = local_registry_path.join(filename);
    if matches!(fs::exists(&file_path), Ok(true)) {
        return Ok(());
    }

    fs::create_dir_all(&local_registry_path)?;
    let as_ron = to_string_pretty(&api_bundle, PrettyConfig::new().compact_structs(true))?;
    fs::write(&file_path, as_ron)?;
    Ok(())
}
