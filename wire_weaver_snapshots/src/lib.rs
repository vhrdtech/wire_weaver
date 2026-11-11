//! API snapshots of ww_global and ww_stdlib crates, embedded at build time.
//!
//! Bundles refer to traits and types of these crates by crate name and version only (`Skipped*` locations):
//! codegen leaves them out of the introspection data a device sends, and a client puts them back with
//! [inline_skipped]. Snapshots are saved by `ww api save` into each crate's `api_snapshots/` and copied into
//! `wire_weaver_snapshots/api_snapshots/` by `just save-snapshots`.
//!
//! A crate version not published yet can change, and its snapshot is then refreshed in place. Another crate's
//! snapshot, or a device built before, still names the definitions it had by their old signatures: `ww api save` keeps
//! each one the refresh replaced in `api_snapshots/superseded/`, and [superseded] finds it by signature (EVO-68).

use std::collections::HashMap;
use std::sync::OnceLock;
use ww_self::ApiBundleOwned;
use ww_self::definition_cache::{self, Kind};
use ww_self::inline::NotInlined;
use ww_self::snapshot;
use ww_version::FullVersionOwned;

/// (file name of the RON source, shrink_wrap bytes), encoded by the build script from `api_snapshots/*.ron`.
const EMBEDDED: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/api_snapshots.rs"));

/// (file name, canonical bytes) of the definitions in-place refreshes of snapshots replaced, as
/// [definition_cache::file_name] names them, from `api_snapshots/superseded/*.ww`.
const SUPERSEDED: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/superseded.rs"));

/// Decoded superseded definitions, one slot per [SUPERSEDED] entry.
static SUPERSEDED_DECODED: [OnceLock<ApiBundleOwned>; SUPERSEDED.len()] =
    [const { OnceLock::new() }; SUPERSEDED.len()];

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

/// The canonical bundle (the definition first, everything it refers to in-line) of the trait or type `name` of the
/// crate `version` with `signature`, kept when the snapshot of that version was refreshed in place and the definition
/// changed (EVO-68). `None` when no refresh replaced it: the snapshot's own definition is found with [get].
pub fn superseded(
    kind: Kind,
    version: &FullVersionOwned,
    name: &str,
    signature: &[u8],
) -> Option<&'static ApiBundleOwned> {
    let file_name = definition_cache::file_name(kind, version, name, signature);
    let index = SUPERSEDED.iter().position(|(n, _)| *n == file_name)?;
    Some(SUPERSEDED_DECODED[index].get_or_init(|| {
        // all of them are checked to decode by a test
        definition_cache::decode(kind, name, signature, SUPERSEDED[index].1)
            .unwrap_or_else(|e| panic!("embedded superseded definition {file_name}: {e:#}"))
    }))
}

/// File names of the embedded superseded definitions, see [superseded].
pub fn superseded_files() -> impl Iterator<Item = &'static str> {
    SUPERSEDED.iter().map(|(name, _)| *name)
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
    render_with_header(bundle, header)
}

/// [render_ron] for the snapshot of one API root of a crate with several (EVO-38), naming the root and its wire
/// version in the header.
pub fn render_root_ron(bundle: &ApiBundleOwned, root: &str) -> Result<String, String> {
    let version = bundle.crate_version(0).map_err(|e| e.to_string())?;
    let v = &version.version;
    let wire = bundle
        .wire
        .ok_or_else(|| format!("API root {root} has no wire version"))?;
    let header = format!(
        "// {} {}.{}.{}: API root {root} at wire {wire} and what it reaches, saved by `ww api save`.\n// Do not edit, \
         bump the crate version and save a new snapshot instead.\n",
        version.crate_id, v.major.0, v.minor.0, v.patch.0
    );
    render_with_header(bundle, header)
}

fn render_with_header(bundle: &ApiBundleOwned, header: String) -> Result<String, String> {
    let bytes = snapshot::encode(bundle).map_err(|e| e.to_string())?;
    let ron =
        ron::ser::to_string_pretty(bundle, ron::ser::PrettyConfig::new().compact_structs(true))
            .map_err(|e| e.to_string())?;
    Ok(header + &snapshot::hash_line(&bytes) + &ron)
}

/// Put definitions of skipped traits and types known from snapshots back into a bundle, see
/// [ww_self::inline::inline_skipped]. Returns the ones that are left skipped. A definition the snapshot of its crate
/// version has under another signature is looked up in the [superseded] ones.
pub fn inline_skipped(bundle: &mut ApiBundleOwned) -> Vec<NotInlined> {
    inline_skipped_or_cached(bundle, None).not_inlined
}

/// Same as [inline_skipped], a definition that the snapshots don't have under its signature (a build from another
/// revision of a crate that is not published yet) and that is not [superseded] is looked up in the definition cache in
/// `cache_dir`, see [ww_self::definition_cache].
pub fn inline_skipped_or_cached(
    bundle: &mut ApiBundleOwned,
    cache_dir: Option<&std::path::Path>,
) -> ww_self::inline::Inlined {
    let by_signature = |kind, version: &FullVersionOwned, name: &str, signature: &[u8]| {
        if let Some(found) = superseded(kind, version, name, signature) {
            return Ok(Some(found.clone()));
        }
        match cache_dir {
            Some(dir) => definition_cache::load(dir, kind, version, name, signature),
            None => Ok(None),
        }
    };
    ww_self::inline::inline_skipped_by_signature(
        bundle,
        &|version| get(version),
        Some(&by_signature),
    )
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

    /// Every superseded definition decodes, matches the signature in its name and is of a crate version with an
    /// embedded snapshot, which has another definition of it (or none) (EVO-68).
    #[test]
    fn superseded_definitions_load() {
        use ww_self::signature::{trait_signature, type_signature};
        for file_name in superseded_files() {
            let (bundle, kind, name, signature) = parse_superseded(file_name);
            let version = bundle.crate_version(0).unwrap();
            assert_eq!(
                definition_cache::file_name(kind, version, &name, &signature),
                file_name
            );
            let found = superseded(kind, version, &name, &signature).unwrap();
            assert!(std::ptr::eq(
                found,
                superseded(kind, version, &name, &signature).unwrap()
            ));
            let snapshot =
                get(version).unwrap_or_else(|| panic!("{file_name}: no snapshot of its version"));
            let resolve = |v: &FullVersionOwned| get(v);
            let current = match kind {
                Kind::Trait => (0..snapshot.traits.len() as u32).find_map(|idx| {
                    match &snapshot.traits[idx as usize] {
                        ww_self::ApiLevelLocationOwned::InLine { level, .. }
                            if level.trait_name == name =>
                        {
                            Some(trait_signature(snapshot, idx, &resolve).unwrap())
                        }
                        _ => None,
                    }
                }),
                Kind::Type => (0..snapshot.types.len() as u32).find_map(|idx| {
                    match &snapshot.types[idx as usize] {
                        ww_self::TypeLocationOwned::InLine {
                            ty: ww_self::TypeOwned::Struct(s),
                            ..
                        } if s.qualified_name() == name => {
                            Some(type_signature(snapshot, idx, &resolve).unwrap())
                        }
                        ww_self::TypeLocationOwned::InLine {
                            ty: ww_self::TypeOwned::Enum(e),
                            ..
                        } if e.qualified_name() == name => {
                            Some(type_signature(snapshot, idx, &resolve).unwrap())
                        }
                        _ => None,
                    }
                }),
            };
            assert_ne!(
                current,
                Some(signature),
                "{file_name}: the snapshot has it, it is not superseded"
            );
        }
    }

    /// The bundle, kind, name and signature of the superseded definition `file_name`, from its bytes.
    fn parse_superseded(file_name: &str) -> (&'static ApiBundleOwned, Kind, String, Vec<u8>) {
        let bytes = SUPERSEDED.iter().find(|(n, _)| *n == file_name).unwrap().1;
        let signature = ww_self::signature::signature_of(bytes);
        let kind = match file_name.contains("-trait-") {
            true => Kind::Trait,
            false => Kind::Type,
        };
        let name = file_name
            .rsplit_once('-')
            .and_then(|(rest, _)| rest.split_once(&format!("-{}-", kind.name())))
            .map(|(_, name)| name.replace('.', "::"))
            .unwrap();
        let bundle = definition_cache::decode(kind, &name, &signature, bytes).unwrap();
        let version = bundle.crate_version(0).unwrap().clone();
        (
            superseded(kind, &version, &name, &signature).unwrap(),
            kind,
            name,
            signature,
        )
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
    /// `Clock`'s changed since, with `Quantity<U, B>` (STD-59, a stated break of the unpublished ww_si and
    /// ww_timebase): `Second` is `Quantity<units::Second, Unbounded>` in the model; and with `Prefix`'s table-free
    /// scales and its `Unknown(u16)` catch-all (STD-59, DER-25); and again moving `exact_units` to its level-rates
    /// main (STD-99), which changes `Quantity`'s skipped `Prefix` and `SIExpr` signatures the same way; and with
    /// `Argument::default` and `flatten` in the model (EVO-22), which every method argument writes; and with
    /// `exact_units`' NaN-free floats (NUM-19), which changed the definition of `Unbounded` that `Quantity` names;
    /// and with `ww_numeric` moving to a new wire line, w2.0 (SW-24, `NumericBaseType`/`NumericValue::INib32`
    /// have no catch-all on either enum, so the addition is breaking), which changes `AnyQuantity`'s skipped
    /// `NumericValue` signature that `Quantity` carries, hence `Second`'s; and with `ww_si` moving to 0.3.0 and a
    /// new wire line, w3.0, for exact_units' new `Dim` and `UnitNames` (UNIT-10, IO-9, EVO-44), which `Second` now
    /// names; and with `ww_si` moving to 0.4.0 and wire w4.0 for exact_units' systems of units (UNIT-9, EVO-47); and
    /// with `ww_si` moving to 0.5.0 and wire w5.0 for `Timed.t` as a device point (STD-110, EVO-51); and with
    /// `Clock::shared_now` returning `Option<UnixMicros<u64>>` instead of `Option<u64>` (typed numbers for shared
    /// time, rate, error and slew, P2401#11nov-0947g). Since EVO-68 a refresh in place keeps the definition it
    /// replaces, which keeps a pinned signature known.
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
        stays_known(
            Kind::Type,
            date_time,
            "DateTime",
            &signature,
            [0x1d, 0x27, 0xf8, 0x6f, 0xf9, 0xee, 0x85, 0x38],
        );

        let timebase = snapshot_of("ww_timebase");
        let idx = timebase
            .traits
            .iter()
            .position(|t| matches!(t, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Clock"))
            .unwrap();
        let signature = trait_signature(timebase, idx as u32, &resolve).unwrap();
        stays_known(
            Kind::Trait,
            timebase,
            "Clock",
            &signature,
            [0xf5, 0xa7, 0xa6, 0x7c, 0x63, 0xa9, 0xed, 0x88],
        );
    }

    /// The definition with the signature `pinned` is the snapshot's own (`signature`), or one an in-place refresh of
    /// the snapshot replaced and kept (EVO-68): a device naming it is still understood.
    fn stays_known(
        kind: Kind,
        snapshot: &ApiBundleOwned,
        name: &str,
        signature: &[u8],
        pinned: [u8; 8],
    ) {
        let version = snapshot.crate_version(0).unwrap();
        assert!(
            signature == pinned || superseded(kind, version, name, &pinned).is_some(),
            "{} {name}: {signature:02x?}, and no superseded definition with {pinned:02x?}",
            kind.name()
        );
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
