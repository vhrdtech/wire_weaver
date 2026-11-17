//! Every saved snapshot of `ww_tap`, `ww_client_server`, `ww_timebase` and the ww_base crates
//! (from the checkout `Cargo.lock` pins), checked against the snapshot of the version before it, as `ww api check <snapshot>` does: both the crate and
//! the wire version must be bumped enough (EVO-27). A wrong wire version on an older snapshot, or a released
//! snapshot that changed, shows here, not only when someone runs the check by hand.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{Check, Outcome};

/// The crate directories `just save-snapshots` saves snapshots of, then the ww_base crates with snapshots.
fn snapshot_crates(root: &Path) -> Vec<PathBuf> {
    let mut crates = vec![
        root.join("ww_tap"),
        root.join("ww_client_server"),
        root.join("ww_timebase"),
    ];
    let mut sources = ww_snapshots::cargo::CargoSources::default();
    sources.entry_dir = Some(root.to_path_buf());
    for name in [
        "ww_date_time",
        "ww_error",
        "ww_global",
        "ww_keyed",
        "ww_numeric",
        "ww_si",
        "ww_version",
    ] {
        crates.push(sources.locked_package_dir(name).unwrap());
    }
    crates
}

#[test]
fn every_snapshot_is_bumped_enough_from_the_one_before() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut pairs = 0;
    let mut failures = vec![];
    for crate_dir in snapshot_crates(&root) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(crate_dir.join(SNAPSHOTS_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "ron"))
            .collect();
        files.sort();
        for file in files {
            let outcome = Check::new(&file).run().unwrap_or_else(|e| panic!("{e}"));
            if matches!(outcome, Outcome::Compared(_)) {
                pairs += 1;
            }
            if let Err(e) = outcome.verdict() {
                failures.push(format!("{outcome}\n{e}"));
            }
        }
    }
    assert!(pairs >= 3, "only {pairs} snapshot pairs found");
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
