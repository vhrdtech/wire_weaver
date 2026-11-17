//! The framework crates with API snapshots (`ww_link`, `ww_client_server`, `ww_timebase`, `ww_tap`) checked against
//! their own `api_snapshots/` (docs/evolution/checker_tool.md), one test per crate, from this crate rather than each
//! crate's own `tests/evolution.rs` (TOOL-31): the `wire_weaver_evolution` dev-dependency turns on wide number
//! features in shared deps (`ww_numeric`, `exact_units`) through cargo's feature unification, which broke narrow
//! feature-set tests when the dependency sat on the crate itself. `WW_API_SAVE=1 cargo test -p stdlib_evolution
//! --test evolution <crate>` saves that crate's current version's snapshot (`ww api save <crate>` does the same).
//! `ww_link`'s link setup payloads (`DeviceInfo`, `LinkSetup`, ...) are on its wire line, whose line is the link
//! generation of the frozen hello (EVO-71), so a break there asks for `w2.0` and the hello follows. Consecutive saved
//! versions of every crate are checked too. The ww_base crates (`ww_global`, `ww_si`, `ww_numeric`, `ww_version`,
//! `ww_error`, `ww_date_time`, `ww_keyed`) are checked from the checkout `Cargo.lock` pins: ww_base has no
//! `wire_weaver_evolution` (its layer is below it), so this crate keeps checking them. The standard library's crates
//! are checked in the ww_stdlib repository.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::Check;
use ww_snapshots::Resolver;
use ww_snapshots::ww_self::definition_cache::{self, Kind};
use ww_snapshots::ww_self::signature::{Signatures, trait_signatures, type_signatures};
use ww_snapshots::ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};
use ww_snapshots::ww_version::{FullVersionOwned, VersionOwned};

/// The crate at `path`, relative to the repository root.
fn check_at(path: &Path) {
    // `strict(true)` (TOOL-35): our own repo wants a reason on every `#[no_unit_hint]`, opted in explicitly
    // here rather than implied by `require_current` (external users copying the documented
    // `tests/evolution.rs` template, docs/evolution/checker_tool.md, aren't forced into writing one).
    Check::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )
    .require_current(true)
    .strict(true)
    .assert();
}

#[test]
fn ww_link() {
    check_at(Path::new("ww_link"));
}

/// A crate of ww_base, checked in the checkout `Cargo.lock` pins.
fn check_base(name: &str) {
    Check::new(base_dir(name))
        .require_current(true)
        .strict(true)
        .assert();
}

/// The ww_base crates with API snapshots.
const BASE_CRATES: [&str; 7] = [
    "ww_date_time",
    "ww_error",
    "ww_global",
    "ww_keyed",
    "ww_numeric",
    "ww_si",
    "ww_version",
];

/// The directory of a ww_base crate in the checkout `Cargo.lock` pins.
fn base_dir(name: &str) -> PathBuf {
    let mut sources = ww_snapshots::cargo::CargoSources::default();
    sources.entry_dir = Some(root());
    sources.locked_package_dir(name).unwrap()
}

#[test]
fn ww_global() {
    check_base("ww_global");
}

#[test]
fn ww_keyed() {
    check_base("ww_keyed");
}

#[test]
fn ww_tap() {
    check_at(Path::new("ww_tap"));
}

/// The repository root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The crates whose `api_snapshots/` are saved here: ww_tap, ww_link, ww_timebase and ww_client_server, then the
/// ww_base crates from the checkout `Cargo.lock` pins.
fn crate_dirs() -> Vec<PathBuf> {
    let root = root();
    let mut crate_dirs = vec![
        root.join("ww_tap"),
        root.join("ww_link"),
        root.join("ww_timebase"),
        root.join("ww_client_server"),
    ];
    crate_dirs.extend(BASE_CRATES.map(base_dir));
    crate_dirs
}

/// A resolver over [crate_dirs] and exact_units' checkout (the one `Cargo.lock` pins, whose `api_snapshots/` ww_si's
/// snapshots refer to), and nothing else: no cache, no network.
fn resolver() -> Resolver {
    let mut sources = ww_snapshots::cargo::CargoSources::default();
    sources.entry_dir = Some(root());
    let exact_units: Vec<PathBuf> = sources
        .locked_package_dirs()
        .unwrap()
        .into_iter()
        .filter(|(name, _, _)| name == "exact_units")
        .map(|(_, _, dir)| dir)
        .collect();
    assert!(
        !exact_units.is_empty(),
        "exact_units is not in Cargo's sources"
    );
    Resolver::new().with_crate_dirs(crate_dirs().into_iter().chain(exact_units))
}

/// Consecutive versions in each crate's `api_snapshots/` follow the rules: every crate of [crate_dirs].
#[test]
fn saved_snapshots_evolve_correctly() {
    for crate_dir in crate_dirs() {
        let dir = ww_snapshots::Dir::of_crate(&crate_dir);
        let mut snapshots: Vec<_> = dir
            .files()
            .unwrap()
            .into_iter()
            .map(|(version, path)| {
                let bundle = wire_weaver_evolution::snapshot_files::parse(&path).unwrap();
                (version, bundle)
            })
            .collect();
        snapshots.sort_by_key(|(v, _)| (v.version.major.0, v.version.minor.0, v.version.patch.0));
        for pair in snapshots.windows(2) {
            let report = wire_weaver_evolution::compare(&pair[0].1, &pair[1].1).unwrap();
            report
                .check_version()
                .unwrap_or_else(|e| panic!("{}: {e}: {:?}", crate_dir.display(), report.breaking));
        }
    }
}

#[test]
fn ww_client_server() {
    check_at(Path::new("ww_client_server"));
}

#[test]
fn ww_date_time() {
    check_base("ww_date_time");
}

#[test]
fn ww_error() {
    check_base("ww_error");
}

#[test]
fn ww_numeric() {
    check_base("ww_numeric");
}

#[test]
fn ww_si() {
    check_base("ww_si");
}

#[test]
fn ww_timebase() {
    check_at(Path::new("ww_timebase"));
}

#[test]
fn ww_version() {
    check_base("ww_version");
}

/// Every skipped trait and type in a saved snapshot is found in the snapshot of its crate, with the same signature.
/// The one gap: `shrink_wrap` has no snapshot and `TailBytes` no form in the model (INT-9), so ww_client_server's
/// reference to it stays a reference.
#[test]
fn snapshots_can_be_inlined() {
    let resolver = resolver();
    for dir in resolver.dirs() {
        for (version, path) in dir.files().unwrap() {
            let mut bundle = ww_snapshots::file::read(&path).unwrap().bundle;
            let mut not_inlined = resolver.inline_skipped(&mut bundle).not_inlined;
            not_inlined.retain(|n| {
                !(n.name == "tail_bytes::TailBytes"
                    && n.crate_version
                        .as_ref()
                        .is_some_and(|v| v.crate_id == "shrink_wrap"))
            });
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

/// Every superseded definition decodes, matches the signature in its name, is found by it, and is of a crate version
/// with a saved snapshot, which has another definition of it (or none) (EVO-68).
#[test]
fn superseded_definitions_load() {
    let resolver = resolver();
    for dir in resolver.dirs() {
        for path in dir.superseded_files().unwrap() {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            let (bundle, kind, name, signature) = parse_superseded(&path);
            let version = bundle.crate_version(0).unwrap();
            assert_eq!(
                definition_cache::file_name(kind, version, &name, &signature),
                file_name
            );
            let found = resolver
                .superseded(kind, version, &name, &signature)
                .unwrap_or_else(|| panic!("{file_name}: not found by signature"));
            assert!(bytes(found) == bytes(&bundle), "{file_name}");
            let snapshot = resolver
                .snapshot(version)
                .unwrap_or_else(|| panic!("{file_name}: no snapshot of its version"));
            let resolve = |v: &FullVersionOwned| resolver.snapshot(v);
            let current = match kind {
                Kind::Trait => (0..snapshot.traits.len() as u32).find_map(|idx| {
                    match &snapshot.traits[idx as usize] {
                        ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == name => {
                            Some(trait_signatures(snapshot, idx, &resolve).unwrap())
                        }
                        _ => None,
                    }
                }),
                Kind::Type => (0..snapshot.types.len() as u32).find_map(|idx| {
                    match &snapshot.types[idx as usize] {
                        TypeLocationOwned::InLine {
                            ty: TypeOwned::Struct(s),
                            ..
                        } if s.qualified_name() == name => {
                            Some(type_signatures(snapshot, idx, &resolve).unwrap())
                        }
                        TypeLocationOwned::InLine {
                            ty: TypeOwned::Enum(e),
                            ..
                        } if e.qualified_name() == name => {
                            Some(type_signatures(snapshot, idx, &resolve).unwrap())
                        }
                        _ => None,
                    }
                }),
            };
            assert!(
                current.is_none_or(|current| current.with_docs != signature),
                "{file_name}: the snapshot has it, it is not superseded"
            );
        }
    }
}

/// The shrink_wrap bytes of a bundle, to compare two.
fn bytes(bundle: &ApiBundleOwned) -> Vec<u8> {
    ww_snapshots::ww_self::snapshot::encode(bundle).unwrap()
}

/// The bundle, kind, name and signature (with docs) of the superseded definition file at `path`, from its bytes.
fn parse_superseded(path: &Path) -> (ApiBundleOwned, Kind, String, Vec<u8>) {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let bytes = std::fs::read(path).unwrap();
    let signature = ww_snapshots::ww_self::signature::signatures_of(&bytes)
        .unwrap()
        .with_docs;
    let kind = match file_name.contains("-trait-") {
        true => Kind::Trait,
        false => Kind::Type,
    };
    let name = file_name
        .rsplit_once('-')
        .and_then(|(rest, _)| rest.split_once(&format!("-{}-", kind.name())))
        .map(|(_, name)| name.replace('.', "::"))
        .unwrap();
    let bundle = definition_cache::decode(kind, &name, &signature, &bytes).unwrap();
    (bundle, kind, name, signature)
}

/// Signatures saved before INT-13 stay known: a trait using a unit marker that has a definition since
/// (`ww_timebase::Clock`, `ww_si::Second`, as the nucleo_g0b1re_api fixture refers to it). Deployed devices refer to
/// it by signature. The definition changed many times since (ww_si's wire lines, exact_units' moves, EVO-22, EVO-58,
/// ...): since EVO-68 a refresh in place keeps the definition it replaces, which keeps a pinned signature known. The
/// pin is read from `pins/nucleo_g0b1re_api_0_4_0.ron`, a byte-identical copy of the fixture's snapshot, which lives
/// in the ww_stdlib repository (`tests/nucleo_g0b1re_fixture`) with its `ww_date_time::DateTime` check: no signature
/// bytes are written here by hand.
#[test]
fn signatures_from_before_marker_definitions() {
    let resolver = resolver();
    let resolve = |v: &FullVersionOwned| resolver.snapshot(v);
    let timebase = resolver
        .snapshot(&FullVersionOwned::new(
            "ww_timebase".to_owned(),
            VersionOwned::new(0, 1, 0),
        ))
        .expect("no snapshot of ww_timebase 0.1.0");
    let idx = timebase
        .traits
        .iter()
        .position(|t| matches!(t, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Clock"))
        .unwrap();
    let signature = trait_signatures(timebase, idx as u32, &resolve).unwrap();
    // an old-format snapshot, read for the one pin
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("pins/nucleo_g0b1re_api_0_4_0.ron");
    let text = std::fs::read_to_string(&fixture).unwrap();
    let pinned = pinned_in(&text, "trait_name: \"Clock\"");
    stays_known(
        &resolver,
        Kind::Trait,
        timebase,
        "Clock",
        &signature,
        &pinned,
    );
}

/// The signature bytes of the reference `head` (`trait_name: "Clock"`) in the RON of a snapshot.
fn pinned_in(text: &str, head: &str) -> Vec<u8> {
    let start = text.find(head).unwrap_or_else(|| panic!("no {head}"));
    let list = &text[start..];
    let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
    list.split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.parse().unwrap())
        .collect()
}

/// The definition `pinned` names (a signature with docs, as every reference was before EVO-69) is the snapshot's own
/// (`signatures`), or one an in-place refresh of the snapshot replaced and kept (EVO-68): a device naming it is still
/// understood.
fn stays_known(
    resolver: &Resolver,
    kind: Kind,
    snapshot: &ApiBundleOwned,
    name: &str,
    signatures: &Signatures,
    pinned: &[u8],
) {
    let version = snapshot.crate_version(0).unwrap();
    assert!(
        signatures.matches(pinned).is_same_shape()
            || resolver.superseded(kind, version, name, pinned).is_some(),
        "{} {name}: {signatures:02x?}, and no superseded definition with {pinned:02x?}",
        kind.name()
    );
}
