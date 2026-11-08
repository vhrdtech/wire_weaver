//! Every saved snapshot of `ww_global`, `ww_tap` and the `ww_stdlib` crates, checked against the snapshot of the
//! version before it, as `ww api check <snapshot>` does: both the crate and the wire version must be bumped enough
//! (EVO-27). A wrong wire version on an older snapshot, or a released snapshot that changed, shows here, not only
//! when someone runs the check by hand.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{Check, Outcome};

/// The crate directories `just save-snapshots` saves snapshots of.
fn snapshot_crates(root: &Path) -> Vec<PathBuf> {
    let mut crates = vec![root.join("ww_global"), root.join("ww_tap")];
    let mut stdlib: Vec<PathBuf> = std::fs::read_dir(root.join("ww_stdlib"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join(SNAPSHOTS_DIR).is_dir())
        .collect();
    stdlib.sort();
    crates.extend(stdlib);
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
