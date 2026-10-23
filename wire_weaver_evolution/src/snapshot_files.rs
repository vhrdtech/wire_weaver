//! Snapshot files of a crate, as `ww api save` writes them into its `api_snapshots/`.

use std::fs;
use std::path::{Path, PathBuf};

use ww_self::ApiBundleOwned;
use ww_version::VersionOwned;

/// Directory inside a crate where its snapshots are kept.
pub const SNAPSHOTS_DIR: &str = "api_snapshots";

/// Reads a snapshot file.
pub fn parse(path: &Path) -> Result<ApiBundleOwned, String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    ron::from_str(&contents).map_err(|e| format!("parsing {}: {e}", path.display()))
}

/// Newest snapshot of the same crate as `bundle` saved in `dir`, with a version older than `version`, or the same
/// one if `or_same` is true. `None` if there is none, also when `dir` doesn't exist.
pub fn latest_snapshot(
    dir: &Path,
    bundle: &ApiBundleOwned,
    version: &VersionOwned,
    or_same: bool,
) -> Result<Option<(PathBuf, ApiBundleOwned)>, String> {
    let crate_id = &bundle.crate_version(0).map_err(|e| e.to_string())?.crate_id;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading {}: {e}", dir.display())),
    };
    let mut latest: Option<(PathBuf, ApiBundleOwned)> = None;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("reading {}: {e}", dir.display()))?
            .path();
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let snapshot = parse(&path)?;
        let snapshot_version = snapshot.crate_version(0).map_err(|e| e.to_string())?;
        let v = key(&snapshot_version.version);
        if &snapshot_version.crate_id != crate_id
            || v > key(version)
            || (v == key(version) && !or_same)
        {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|(_, l)| key(&l.ext_crates[0].version) < v)
        {
            latest = Some((path, snapshot));
        }
    }
    Ok(latest)
}

fn key(v: &VersionOwned) -> (u32, u32, u32) {
    (v.major.0, v.minor.0, v.patch.0)
}

/// `major.minor.patch`.
pub fn triplet(v: &VersionOwned) -> String {
    format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0)
}
