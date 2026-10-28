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
use ww_self::snapshot;
use ww_version::FullVersionOwned;

/// (file name of the RON source, shrink_wrap bytes), encoded by the build script from `api_snapshots/*.ron`.
const EMBEDDED: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/api_snapshots.rs"));

/// Decoded snapshots, one slot per [EMBEDDED] entry, filled on first use of that entry.
static DECODED: [OnceLock<ApiBundleOwned>; EMBEDDED.len()] =
    [const { OnceLock::new() }; EMBEDDED.len()];

/// Snapshot of exactly this crate version, if embedded. Only this one is decoded (on its first use): a client
/// connecting to a device whose API uses one or two of these crates doesn't pay for decoding all of them.
pub fn get(version: &FullVersionOwned) -> Option<&'static ApiBundleOwned> {
    let file_name = format!("{}.ron", version.filename_friendly());
    let index = EMBEDDED.iter().position(|(name, _)| *name == file_name)?;
    Some(decoded(index))
}

/// All embedded snapshots, by crate name and version. Decodes every one not decoded yet.
pub fn all() -> &'static HashMap<FullVersionOwned, &'static ApiBundleOwned> {
    static ALL: OnceLock<HashMap<FullVersionOwned, &'static ApiBundleOwned>> = OnceLock::new();
    ALL.get_or_init(|| {
        (0..EMBEDDED.len())
            .map(|index| {
                let bundle = decoded(index);
                (bundle.ext_crates[0].clone(), bundle)
            })
            .collect()
    })
}

fn decoded(index: usize) -> &'static ApiBundleOwned {
    DECODED[index].get_or_init(|| {
        let (file_name, bytes) = EMBEDDED[index];
        // all of them are checked to decode, and to be named after their crate and version, by tests
        let bundle = snapshot::decode(bytes)
            .unwrap_or_else(|e| panic!("embedded API snapshot {file_name}: {e:?}"));
        assert!(
            !bundle.ext_crates.is_empty(),
            "embedded API snapshot {file_name}: no ext_crates[0], snapshot's own crate"
        );
        bundle
    })
}

/// File names and contents of the snapshots as `ww api save` wrote them (the RON the embedded bytes were made
/// from). Only the bytes are embedded, so the text is rendered from the decoded bundles on first call, with
/// [render_ron], and is identical to the saved files (a test checks that).
pub fn files() -> &'static [(&'static str, &'static str)] {
    static FILES: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    FILES.get_or_init(|| {
        (0..EMBEDDED.len())
            .map(|index| {
                let text = render_ron(decoded(index)).expect("rendering embedded API snapshot");
                (EMBEDDED[index].0, &*Box::leak(text.into_boxed_str()))
            })
            .collect()
    })
}

/// The text of a snapshot file as `ww api save` writes it: two comment lines naming the crate and version, the
/// [ww_self::snapshot::HASH_PREFIX] line with the SHA-256 of the bundle's shrink_wrap bytes, then the bundle as
/// pretty RON.
pub fn render_ron(bundle: &ApiBundleOwned) -> Result<String, String> {
    let version = bundle.crate_version(0).map_err(|e| e.to_string())?;
    let v = &version.version;
    let header = format!(
        "// {} {}.{}.{} traits and types, saved by `ww api save`.\n// Do not edit, bump the crate version and save a new snapshot instead.\n",
        version.crate_id, v.major.0, v.minor.0, v.patch.0
    );
    let bytes = snapshot::encode(bundle).map_err(|e| e.to_string())?;
    let ron =
        ron::ser::to_string_pretty(bundle, ron::ser::PrettyConfig::new().compact_structs(true))
            .map_err(|e| e.to_string())?;
    Ok(header + &snapshot::hash_line(&bytes) + &ron)
}

/// Put definitions of skipped traits and types known from snapshots back into a bundle, see
/// [ww_self::inline::inline_skipped]. Returns the ones that are left skipped.
pub fn inline_skipped(bundle: &mut ApiBundleOwned) -> Vec<NotInlined> {
    ww_self::inline::inline_skipped(bundle, &|version| get(version))
}

/// Same as [inline_skipped], a definition that the snapshots don't have under its signature (a build from another
/// revision of a crate that is not published yet) is looked up in the definition cache in `cache_dir`, see
/// [ww_self::definition_cache].
pub fn inline_skipped_or_cached(
    bundle: &mut ApiBundleOwned,
    cache_dir: Option<&std::path::Path>,
) -> ww_self::inline::Inlined {
    ww_self::inline::inline_skipped_or_cached(bundle, &|version| get(version), cache_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_snapshots_load() {
        for (index, (file_name, _)) in EMBEDDED.iter().enumerate() {
            let bundle = decoded(index);
            assert_eq!(
                format!("{}.ron", bundle.ext_crates[0].filename_friendly()),
                *file_name
            );
        }
        assert_eq!(all().len(), EMBEDDED.len());
        for (version, bundle) in all() {
            assert!(std::ptr::eq(get(version).unwrap(), *bundle), "{version:?}");
        }
    }

    /// `files()` renders the same snapshot `ww api save` wrote: same header and hash, and the saved text and the
    /// rendered one read back to the same bundle. The text may differ in fields added to ww_self since the file
    /// was saved (`meta: None` is in the rendered one and not in an older file; reading fills it in).
    #[test]
    fn files_are_the_saved_ron() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("api_snapshots");
        let files = files();
        assert_eq!(files.len(), EMBEDDED.len());
        for (index, (file_name, text)) in files.iter().enumerate() {
            let saved = std::fs::read_to_string(dir.join(file_name)).unwrap();
            let header = |t: &str| t.lines().take(3).collect::<Vec<_>>().join("\n");
            assert_eq!(header(&saved), header(text), "{file_name}");
            assert_eq!(
                snapshot::saved_hash(text),
                Some(snapshot::sha256_hex(EMBEDDED[index].1).as_str())
            );
            let from_saved: ApiBundleOwned = ron::from_str(&saved).unwrap();
            let from_rendered: ApiBundleOwned = ron::from_str(text).unwrap();
            let bytes = |b: &ApiBundleOwned| snapshot::encode(b).unwrap();
            assert!(bytes(&from_saved) == bytes(&from_rendered), "{file_name}");
            assert!(bytes(&from_rendered) == EMBEDDED[index].1, "{file_name}");
        }
    }

    /// Signatures saved before INT-13 stay: a plain struct (`ww_date_time::DateTime`, as `ww_board_info` 0.1.0 refers
    /// to it) and a trait using a unit marker that has a definition since (`ww_timebase::Clock`, `ww_si::Second`, as
    /// the nucleo_g0b1re_api fixture refers to it). Deployed devices refer to these definitions by signature.
    #[test]
    fn signatures_from_before_marker_definitions() {
        use ww_self::signature::{trait_signature, type_signature};
        use ww_self::{ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};
        let snapshot_of = |crate_id: &str| {
            all()
                .iter()
                .find(|(v, _)| {
                    v.crate_id == crate_id && v.version.major.0 == 0 && v.version.minor.0 == 1
                })
                .map(|(_, b)| *b)
                .unwrap()
        };
        let resolve = |v: &FullVersionOwned| get(v);

        let date_time = snapshot_of("ww_date_time");
        let idx = date_time
            .types
            .iter()
            .position(|t| {
                matches!(t, TypeLocationOwned::InLine { ty: TypeOwned::Struct(s), .. } if s.ident == "DateTime")
            })
            .unwrap();
        let signature = type_signature(date_time, idx as u32, &resolve).unwrap();
        assert_eq!(signature, [0x1d, 0x27, 0xf8, 0x6f, 0xf9, 0xee, 0x85, 0x38]);

        let timebase = snapshot_of("ww_timebase");
        let idx = timebase
            .traits
            .iter()
            .position(|t| matches!(t, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Clock"))
            .unwrap();
        let signature = trait_signature(timebase, idx as u32, &resolve).unwrap();
        assert_eq!(signature, [0x0b, 0x01, 0x4a, 0x04, 0x13, 0x1f, 0x79, 0xb2]);
    }

    /// Every skipped trait and type in snapshots is found in the snapshot of its crate, with the same signature.
    #[test]
    fn snapshots_can_be_inlined() {
        for (version, snapshot) in all() {
            let mut bundle = (*snapshot).clone();
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
