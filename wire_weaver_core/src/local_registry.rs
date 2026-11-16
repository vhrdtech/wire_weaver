use anyhow::anyhow;
use ron::ser::{PrettyConfig, to_string_pretty};
use std::fs;
use ww_self::definition_cache::{self, Kind};
use ww_self::signature::{canonical_trait_bytes, canonical_type_bytes, signatures_of};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};
use ww_snapshots::Resolver;

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
    // NOTE: wire_weaver_client/src/local_registry.rs uses the same naming scheme, keep in sync
    let filename = format!("{}-{hash}{with_docs}.ron", api_crate.filename_friendly());
    let local_registry_path = ww_self::host::root().ok_or(anyhow!(
        "no home directory and no {}",
        ww_self::host::HOME_ENV
    ))?;
    let file_path = local_registry_path.join(filename);
    // kept when it reads back: a file of an older ww_self format is written again
    if read_back(&file_path) {
        return Ok(());
    }

    fs::create_dir_all(&local_registry_path)?;
    let as_ron = to_string_pretty(&api_bundle, PrettyConfig::new().compact_structs(true))?;
    fs::write(&file_path, as_ron)?;
    Ok(())
}

/// Store the definitions `sent` leaves out (taken from `full`, the same bundle with everything in-line) in the
/// definition cache on this machine, by signature, so that a host here finds them when this build's snapshot of an
/// unpublished crate is not the host's (see [ww_self::definition_cache]).
pub(crate) fn cache_definitions(full: &ApiBundleOwned, sent: &ApiBundleOwned, resolver: &Resolver) {
    let Some(dir) = definition_cache::default_dir() else {
        return;
    };
    if let Err(e) = cache_definitions_in(&dir, full, sent, resolver) {
        eprintln!("Failed to cache definitions in {}: {e:#}", dir.display());
    }
}

/// Copies the snapshots codegen skipped definitions against (the walked crates' `api_snapshots/`, the directories
/// of `resolver`) into the snapshot cache on this machine, so a host here finds a dependency's snapshot by name
/// without Cargo's sources or the network.
pub(crate) fn cache_snapshots(resolver: &Resolver) {
    let Some(root) = ww_snapshots::cache::default_root() else {
        return;
    };
    let into = ww_snapshots::cache::snapshots_dir(&root);
    for dir in resolver.dirs() {
        if let Err(e) = ww_snapshots::cache::store_crate(&into, dir) {
            eprintln!(
                "Failed to copy {} into {}: {e:#}",
                dir.path().display(),
                into.path().display()
            );
        }
    }
}

pub(crate) fn cache_definitions_in(
    dir: &std::path::Path,
    full: &ApiBundleOwned,
    sent: &ApiBundleOwned,
    resolver: &Resolver,
) -> anyhow::Result<()> {
    let resolve = |v: &_| resolver.snapshot(v);
    for location in &sent.types {
        let TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            signature,
        } = location
        else {
            continue;
        };
        let version = sent.crate_version(crate_idx.0)?;
        let Some(idx) = full.types.iter().position(|l| match l {
            TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } => {
                s.qualified_name() == *type_name
                    && full.crate_version(s.crate_idx.0).ok() == Some(version)
            }
            TypeLocationOwned::InLine {
                ty: TypeOwned::Enum(e),
                ..
            } => {
                e.qualified_name() == *type_name
                    && full.crate_version(e.crate_idx.0).ok() == Some(version)
            }
            _ => false,
        }) else {
            continue;
        };
        let bytes = canonical_type_bytes(full, idx as u32, &resolve)?;
        if signatures_of(&bytes)?.matches(signature).is_same_shape() {
            definition_cache::store(dir, Kind::Type, version, type_name, &bytes)?;
        }
    }
    for location in &sent.traits {
        let ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name,
            signature,
        } = location
        else {
            continue;
        };
        let version = sent.crate_version(crate_idx.0)?;
        let Some(idx) = full.traits.iter().position(|l| match l {
            ApiLevelLocationOwned::InLine { level, crate_idx } => {
                level.trait_name == *trait_name
                    && full.crate_version(crate_idx.0).ok() == Some(version)
            }
            _ => false,
        }) else {
            continue;
        };
        let bytes = canonical_trait_bytes(full, idx as u32, &resolve)?;
        if signatures_of(&bytes)?.matches(signature).is_same_shape() {
            definition_cache::store(dir, Kind::Trait, version, trait_name, &bytes)?;
        }
    }
    Ok(())
}

/// Whether the cached bundle at `path` exists and is readable in this ww_self format.
fn read_back(path: &std::path::Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .is_some_and(|s| ron::from_str::<ApiBundleOwned>(&s).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn left_out_definitions_are_cached_by_signature() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/uart_api");
        let (full, _, _, paths) = crate::load_with_hints(&crate_path, None, false).unwrap();
        let resolver = Resolver::new().with_crate_dirs(paths.crate_dirs());
        let sent = crate::transform::skip_known(&full, &resolver).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "ww_core_definition_cache_test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        cache_definitions_in(&dir, &full, &sent, &resolver).unwrap();

        let mut checked = 0;
        for location in &sent.types {
            let TypeLocationOwned::SkippedFullVersion {
                crate_idx,
                type_name,
                signature,
            } = location
            else {
                continue;
            };
            let version = sent.crate_version(crate_idx.0).unwrap();
            let cached = definition_cache::load(&dir, Kind::Type, version, type_name, signature)
                .unwrap()
                .unwrap_or_else(|| panic!("{type_name} is not cached"));
            assert!(matches!(cached.types[0], TypeLocationOwned::InLine { .. }));
            checked += 1;
        }
        for location in &sent.traits {
            let ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx,
                trait_name,
                signature,
            } = location
            else {
                continue;
            };
            let version = sent.crate_version(crate_idx.0).unwrap();
            assert!(
                definition_cache::load(&dir, Kind::Trait, version, trait_name, signature)
                    .unwrap()
                    .is_some(),
                "{trait_name} is not cached"
            );
            checked += 1;
        }
        // ww_uart's Uart trait, with the ww_si types it uses inside its canonical bundle
        assert!(checked >= 1, "uart_api leaves out ww_uart definitions");
        // an unknown signature is not there
        let version = sent.ext_crates.last().unwrap();
        assert!(
            definition_cache::load(&dir, Kind::Type, version, "Volt", &[0; 8])
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
