//! Snapshot files of a crate, as `ww api save` writes them into its `api_snapshots/`: one per crate version
//! (`<crate>_<major>_<minor>_<patch>.ron`), or, for a crate with several API roots (EVO-37), one per root and wire
//! version and the crate version that saved it (`<crate>_<major>_<minor>_<patch>.<module path>.<Trait>@w<line>.<revision>.ron`,
//! EVO-38).

use std::fs;
use std::path::{Path, PathBuf};

use ww_self::ApiBundleOwned;
use ww_version::{FullVersionOwned, VersionOwned, WireVersion};

use crate::recorded::Recorded;

/// Directory inside a crate where its snapshots are kept.
pub const SNAPSHOTS_DIR: &str = "api_snapshots";

/// A snapshot file as read.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub path: PathBuf,
    pub bundle: ApiBundleOwned,
    /// The fields of the model the file has.
    pub recorded: Recorded,
}

/// Reads a snapshot file.
pub fn parse(path: &Path) -> Result<ApiBundleOwned, String> {
    read(path).map(|s| s.bundle)
}

/// Reads a snapshot file, with what it recorded.
pub fn read(path: &Path) -> Result<Snapshot, String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let (bundle, recorded) =
        Recorded::parse(&contents).map_err(|e| format!("parsing {}: {e}", path.display()))?;
    Ok(Snapshot {
        path: path.to_path_buf(),
        bundle,
        recorded,
    })
}

/// Newest snapshot of the same crate as `bundle` saved in `dir`, with a version older than `version`, or the same
/// one if `or_same` is true. `None` if there is none, also when `dir` doesn't exist.
pub fn latest_snapshot(
    dir: &Path,
    bundle: &ApiBundleOwned,
    version: &VersionOwned,
    or_same: bool,
) -> Result<Option<Snapshot>, String> {
    let crate_id = &bundle.crate_version(0).map_err(|e| e.to_string())?.crate_id;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading {}: {e}", dir.display())),
    };
    let mut latest: Option<Snapshot> = None;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("reading {}: {e}", dir.display()))?
            .path();
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let snapshot = read(&path)?;
        let snapshot_version = snapshot
            .bundle
            .crate_version(0)
            .map_err(|e| e.to_string())?;
        let v = key(&snapshot_version.version);
        if &snapshot_version.crate_id != crate_id
            || v > key(version)
            || (v == key(version) && !or_same)
        {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|l| key(&l.bundle.ext_crates[0].version) < v)
        {
            latest = Some(snapshot);
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

/// File name of the snapshot of the API root `root` (`wire_v2::Thermo`) of crate version `version` at wire version
/// `wire`: `multi_root_api_0_1_0.wire_v2.Thermo@w2.1.ron`. A crate version saves a root's file only when the root
/// changed since its previous one, and a saved file of an older crate version is never written again.
pub fn root_file_name(version: &FullVersionOwned, root: &str, wire: WireVersion) -> String {
    format!(
        "{}.{}@{wire}.ron",
        version.filename_friendly(),
        root.replace("::", ".")
    )
}

/// What a per-root snapshot file is named after ([root_file_name]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootFileName {
    pub crate_id: String,
    /// The crate version, `(major, minor, patch)`.
    pub version: (u32, u32, u32),
    /// `wire_v2::Thermo`.
    pub root: String,
    pub wire: WireVersion,
}

/// The crate version, root and wire version a per-root snapshot file is named after ([root_file_name]); `None` for
/// any other file, a whole-crate one too.
pub fn root_of_file_name(file_name: &str) -> Option<RootFileName> {
    let stem = file_name.strip_suffix(".ron")?;
    let (path, wire) = stem.rsplit_once('@')?;
    let wire = wire.parse().ok()?;
    let (crate_version, root) = path.split_once('.')?;
    let mut parts = crate_version.rsplitn(4, '_');
    let patch = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let major = parts.next()?.parse().ok()?;
    let crate_id = parts.next()?;
    if crate_id.is_empty() || root.is_empty() {
        return None;
    }
    Some(RootFileName {
        crate_id: crate_id.to_string(),
        version: (major, minor, patch),
        root: root.replace('.', "::"),
        wire,
    })
}

/// An earlier snapshot of an API root, with the wire and crate versions it was saved at.
#[cfg(feature = "source")]
#[derive(Clone, Debug)]
pub struct RootSnapshot {
    pub snapshot: Snapshot,
    pub wire: WireVersion,
    pub version: VersionOwned,
}

/// Every snapshot in `dir` of the API root `root` (`wire_v2::Thermo`) of crate `crate_id`, oldest first by wire
/// version, then crate version: its per-root files, of any line, and the whole-crate files (`<crate>_0_1_0.ron`, from
/// before the crate had several roots) whose wire version is on `line` and that have a root trait of the same name,
/// cut down to that root ([wire_weaver_core::root_of_crate_snapshot]). Empty when `dir` doesn't exist.
#[cfg(feature = "source")]
pub fn root_snapshots(
    dir: &Path,
    crate_id: &str,
    root: &str,
    line: u32,
) -> Result<Vec<RootSnapshot>, String> {
    let trait_name = root.rsplit("::").next().unwrap_or(root);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("reading {}: {e}", dir.display())),
    };
    let mut found = vec![];
    for entry in entries {
        let path = entry
            .map_err(|e| format!("reading {}: {e}", dir.display()))?
            .path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        match root_of_file_name(file_name) {
            Some(name) => {
                if name.crate_id != crate_id || name.root != root {
                    continue;
                }
                let wire = name.wire;
                let snapshot = read(&path)?;
                if snapshot.bundle.wire != Some(wire) {
                    return Err(format!(
                        "{} is named after wire version {wire}, and has {}",
                        path.display(),
                        snapshot
                            .bundle
                            .wire
                            .map_or("none".to_string(), |w| w.to_string())
                    ));
                }
                let version = snapshot
                    .bundle
                    .crate_version(0)
                    .map_err(|e| e.to_string())?
                    .version
                    .clone();
                if key(&version) != name.version {
                    return Err(format!(
                        "{} is named after crate version {}.{}.{}, and has {}",
                        path.display(),
                        name.version.0,
                        name.version.1,
                        name.version.2,
                        triplet(&version)
                    ));
                }
                found.push(RootSnapshot {
                    snapshot,
                    wire,
                    version,
                });
            }
            None => {
                let mut snapshot = read(&path)?;
                let own = snapshot
                    .bundle
                    .crate_version(0)
                    .map_err(|e| e.to_string())?;
                if own.crate_id != crate_id {
                    continue;
                }
                let version = own.version.clone();
                let Some(wire) = snapshot.bundle.wire.filter(|w| w.line.0 == line) else {
                    continue;
                };
                let Some(bundle) =
                    wire_weaver_core::root_of_crate_snapshot(&snapshot.bundle, trait_name)
                else {
                    continue;
                };
                snapshot.bundle = bundle;
                found.push(RootSnapshot {
                    snapshot,
                    wire,
                    version,
                });
            }
        }
    }
    found.sort_by_key(|s| (wire_key(s.wire), key(&s.version)));
    Ok(found)
}

#[cfg(feature = "source")]
fn wire_key(w: WireVersion) -> (u32, u32) {
    (w.line.0, w.revision.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_file_names_round_trip() {
        let wire: WireVersion = "w2.1".parse().unwrap();
        let version: FullVersionOwned =
            ww_version::FullVersion::new("thermo_api", ww_version::Version::new(0, 12, 3)).into();
        let name = root_file_name(&version, "wire_v2::Thermo", wire);
        assert_eq!(name, "thermo_api_0_12_3.wire_v2.Thermo@w2.1.ron");
        assert_eq!(
            root_of_file_name(&name),
            Some(RootFileName {
                crate_id: "thermo_api".into(),
                version: (0, 12, 3),
                root: "wire_v2::Thermo".into(),
                wire
            })
        );
        let nested = root_file_name(&version, "lines::v3::Api", wire);
        assert_eq!(root_of_file_name(&nested).unwrap().root, "lines::v3::Api");
        for other in [
            "thermo_api_0_1_0.ron",
            "thermo_api_0_1_0@w2.1.ron",
            "thermo_api.Thermo@w2.1.ron",
            "x_0_1_0.Thermo@2.1.ron",
            "x_0_1_0.Y@w1.0.txt",
        ] {
            assert_eq!(root_of_file_name(other), None, "{other}");
        }
    }
}
