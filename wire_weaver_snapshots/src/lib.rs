//! API snapshots of ww_global and ww_stdlib crates, embedded at build time.
//!
//! Bundles refer to traits and types of these crates by crate name and version only (`Skipped*` locations):
//! codegen leaves them out of the introspection data a device sends, and a client puts them back with
//! [inline_skipped]. Snapshots are saved by `ww api save` into each crate's `api_snapshots/` and copied into
//! `wire_weaver_snapshots/api_snapshots/` by `just save-snapshots`.

use std::collections::HashMap;
use std::sync::OnceLock;
use ww_self::ApiBundleOwned;
use ww_self::inline::NotInlined;
use ww_version::FullVersionOwned;

/// (file name, contents)
const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/api_snapshots.rs"));

/// Snapshot of exactly this crate version, if embedded.
pub fn get(version: &FullVersionOwned) -> Option<&'static ApiBundleOwned> {
    all().get(version)
}

/// All embedded snapshots, by crate name and version. Parsed on first use.
pub fn all() -> &'static HashMap<FullVersionOwned, ApiBundleOwned> {
    static SNAPSHOTS: OnceLock<HashMap<FullVersionOwned, ApiBundleOwned>> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| {
        EMBEDDED
            .iter()
            .map(|(file_name, contents)| {
                // all of them are checked to load by tests
                let bundle = parse(contents)
                    .unwrap_or_else(|e| panic!("embedded API snapshot {file_name}: {e:?}"));
                (bundle.ext_crates[0].clone(), bundle)
            })
            .collect()
    })
}

/// File names and contents of the embedded snapshots, as they were saved.
pub fn files() -> &'static [(&'static str, &'static str)] {
    EMBEDDED
}

/// Put definitions of skipped traits and types known from snapshots back into a bundle, see
/// [ww_self::inline::inline_skipped]. Returns the ones that are left skipped.
pub fn inline_skipped(bundle: &mut ApiBundleOwned) -> Vec<NotInlined> {
    ww_self::inline::inline_skipped(bundle, &|version| get(version))
}

fn parse(contents: &str) -> Result<ApiBundleOwned, String> {
    let bundle: ApiBundleOwned = ron::from_str(contents).map_err(|e| e.to_string())?;
    if bundle.ext_crates.is_empty() {
        return Err("no ext_crates[0], snapshot's own crate".into());
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_snapshots_load() {
        for (file_name, contents) in EMBEDDED {
            let bundle = parse(contents).unwrap_or_else(|e| panic!("{file_name}: {e}"));
            assert_eq!(
                format!("{}.ron", bundle.ext_crates[0].filename_friendly()),
                *file_name
            );
        }
        assert_eq!(all().len(), EMBEDDED.len());
    }

    /// Every skipped trait and type in snapshots is found in the snapshot of its crate, with the same signature.
    #[test]
    fn snapshots_can_be_inlined() {
        for (version, snapshot) in all() {
            let mut bundle = snapshot.clone();
            let not_inlined = inline_skipped(&mut bundle);
            assert!(
                not_inlined.is_empty(),
                "{}: {}",
                version.crate_id,
                not_inlined
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}
