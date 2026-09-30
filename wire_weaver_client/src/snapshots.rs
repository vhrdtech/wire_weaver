//! API snapshots of ww_global and ww_stdlib crates, embedded at build time.
//!
//! Bundles refer to traits and types of these crates by crate name and version only (`Skipped*` locations),
//! snapshots are used to look up their definitions, e.g. to calculate signatures (see [ww_self::signature]).
//! Saved by `ww api save` into each crate's `api_snapshots/` and copied into `wire_weaver_client/api_snapshots/`
//! by `just save-snapshots`.

use std::collections::HashMap;
use std::sync::OnceLock;
use tracing::warn;
use ww_self::ApiBundleOwned;
use ww_version::FullVersionOwned;

/// (file name, contents)
const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/api_snapshots.rs"));

/// Snapshot of exactly this crate version, if embedded.
/// Can be used as a [ww_self::signature::Resolve] callback.
pub fn get(version: &FullVersionOwned) -> Option<&'static ApiBundleOwned> {
    all().get(version)
}

/// All embedded snapshots, by crate name and version. Parsed on first use.
pub fn all() -> &'static HashMap<FullVersionOwned, ApiBundleOwned> {
    static SNAPSHOTS: OnceLock<HashMap<FullVersionOwned, ApiBundleOwned>> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| {
        let mut snapshots = HashMap::new();
        for (file_name, contents) in EMBEDDED {
            match parse(contents) {
                Ok(bundle) => {
                    snapshots.insert(bundle.ext_crates[0].clone(), bundle);
                }
                Err(e) => warn!("Failed to load embedded API snapshot {file_name}: {e:?}"),
            }
        }
        snapshots
    })
}

fn parse(contents: &str) -> anyhow::Result<ApiBundleOwned> {
    let bundle: ApiBundleOwned = ron::from_str(contents)?;
    if bundle.ext_crates.is_empty() {
        anyhow::bail!("no ext_crates[0], snapshot's own crate");
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use ww_self::signature::{trait_signature, type_signature};
    use ww_self::{
        ApiLevelLocationOwned, ItemEnumOwned, ItemStructOwned, TypeLocationOwned, TypeOwned,
    };

    #[test]
    fn all_embedded_snapshots_load() {
        for (file_name, contents) in EMBEDDED {
            let bundle = parse(contents).unwrap_or_else(|e| panic!("{file_name}: {e:?}"));
            assert_eq!(
                format!("{}.ron", bundle.ext_crates[0].filename_friendly()),
                *file_name
            );
        }
        assert_eq!(all().len(), EMBEDDED.len());
    }

    /// Signatures of skipped traits and types match the definitions in the snapshots of their crates.
    #[test]
    fn skipped_signatures_match_referenced_snapshots() {
        let resolve = |version: &FullVersionOwned| get(version);
        for (version, bundle) in all() {
            for location in &bundle.types {
                let TypeLocationOwned::SkippedFullVersion {
                    crate_idx,
                    type_name,
                    signature,
                } = location
                else {
                    continue;
                };
                let ext_crate = &bundle.ext_crates[crate_idx.0 as usize];
                let snapshot = get(ext_crate).unwrap_or_else(|| {
                    panic!("{version:?}: no snapshot of {ext_crate:?} for {type_name}")
                });
                let idx = snapshot
                    .types
                    .iter()
                    .position(|l| {
                        matches!(l, TypeLocationOwned::InLine { ty: TypeOwned::Struct(ItemStructOwned { ident, .. }) | TypeOwned::Enum(ItemEnumOwned { ident, .. }), .. } if ident == type_name)
                    })
                    .unwrap_or_else(|| panic!("{type_name} not in {ext_crate:?} snapshot"));
                let expected = type_signature(snapshot, idx as u32, &resolve).unwrap();
                assert_eq!(signature, &expected, "{version:?}: {type_name}");
            }
            for location in &bundle.traits {
                let ApiLevelLocationOwned::SkippedFullVersion {
                    crate_idx,
                    trait_name,
                    signature,
                } = location
                else {
                    continue;
                };
                let ext_crate = &bundle.ext_crates[crate_idx.0 as usize];
                let snapshot = get(ext_crate).unwrap_or_else(|| {
                    panic!("{version:?}: no snapshot of {ext_crate:?} for {trait_name}")
                });
                let idx = snapshot
                    .traits
                    .iter()
                    .position(|l| {
                        matches!(l, ApiLevelLocationOwned::InLine { level, .. } if &level.trait_name == trait_name)
                    })
                    .unwrap_or_else(|| panic!("{trait_name} not in {ext_crate:?} snapshot"));
                let expected = trait_signature(snapshot, idx as u32, &resolve).unwrap();
                assert_eq!(signature, &expected, "{version:?}: {trait_name}");
            }
        }
    }

    /// Snapshots are saved for the current source of each crate and the embedded copies are the same.
    /// Only checked in the repo, where the crates are next to this one.
    #[test]
    fn snapshots_are_up_to_date() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut crate_paths = vec![root.join("ww_global")];
        for entry in fs::read_dir(root.join("ww_stdlib")).unwrap() {
            let path = entry.unwrap().path();
            // ww_client_server is the protocol itself, not used in APIs
            if path.join("Cargo.toml").exists() && !path.ends_with("ww_client_server") {
                crate_paths.push(path);
            }
        }
        let hint =
            "run `just save-snapshots` (with --force for versions that were never published)";
        let mut copied = 0;
        for crate_path in crate_paths {
            let bundle = wire_weaver_core::load_crate(&crate_path).unwrap();
            let file_name = format!("{}.ron", bundle.ext_crates[0].filename_friendly());
            let saved = fs::read_to_string(crate_path.join("api_snapshots").join(&file_name))
                .unwrap_or_else(|e| panic!("{file_name}: {e}, {hint}"));
            let saved = parse(&saved).unwrap();
            assert_eq!(
                ron::to_string(&saved).unwrap(),
                ron::to_string(&bundle).unwrap(),
                "{file_name} is out of date, {hint}"
            );
            for entry in fs::read_dir(crate_path.join("api_snapshots")).unwrap() {
                let path = entry.unwrap().path();
                let file_name = path.file_name().unwrap().to_str().unwrap();
                let embedded = EMBEDDED.iter().find(|(name, _)| *name == file_name);
                let embedded =
                    embedded.unwrap_or_else(|| panic!("{file_name} is not embedded, {hint}"));
                assert_eq!(
                    embedded.1,
                    fs::read_to_string(&path).unwrap(),
                    "embedded {file_name} differs, {hint}"
                );
                copied += 1;
            }
        }
        assert_eq!(
            copied,
            EMBEDDED.len(),
            "stale snapshots are embedded, {hint}"
        );
    }
}
