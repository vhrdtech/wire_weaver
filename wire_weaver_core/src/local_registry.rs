use crate::ast::api::ApiLevelSourceLocation;
use anyhow::anyhow;
use ron::ser::{to_string_pretty, PrettyConfig};
use std::fs;
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned};

pub(crate) fn cache_api_bundle(
    source: &ApiLevelSourceLocation,
    hash: &[u8],
    api_bundle: &ApiBundleOwned,
) {
    if let Err(e) = cache_api_bundle_inner(source, hash, api_bundle) {
        eprintln!("Failed to cache API bundle: {}", e);
    }
}

fn cache_api_bundle_inner(
    source: &ApiLevelSourceLocation,
    hash: &[u8],
    api_bundle: &ApiBundleOwned,
) -> anyhow::Result<()> {
    let api_crate_name = source.crate_name();
    if api_crate_name == "crate" || api_crate_name == "super" {
        // ignore tests
        return Ok(());
    }
    let hash = hex::encode(hash);
    let with_docs = if contains_docs(api_bundle) {
        "+docs"
    } else {
        ""
    };
    let filename = format!("{}-{hash}{with_docs}.ron", api_crate_name);
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

fn contains_docs(api_bundle: &ApiBundleOwned) -> bool {
    for item in &api_bundle.root.items {
        if !item.docs.is_empty() {
            return true;
        }
    }
    for t in &api_bundle.traits {
        let ApiLevelLocationOwned::InLine(level) = t else {
            continue;
        };
        if !level.docs.is_empty() {
            return true;
        }
        for item in &level.items {
            if !item.docs.is_empty() {
                return true;
            }
        }
    }
    false
}
