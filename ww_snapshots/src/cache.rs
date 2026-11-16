//! The per-user cache, `~/.wire_weaver/`, as far as snapshots are concerned.
//!
//! ```text
//! ~/.wire_weaver/
//!   <crate>_<x>_<y>_<z>-<hash>[+docs].ron   device API bundles by API hash (wire_weaver_client)
//!   definitions/<...>.ww                     definition cache by signature (ww_self::definition_cache)
//!   snapshots/<crate>_<x>_<y>_<z>.ron        whole-crate snapshots, byte for byte as `ww api save` wrote them
//!   snapshots/superseded/<...>.ww            kept definitions, named as in a crate's api_snapshots/superseded/
//!   snapshots/missing/<crate>_<x>_<y>_<z>    a fetch found nothing; its time says when to try again
//!   sources.toml                             offline, and the git repository of a crate the fetch may use
//! ```
//!
//! `snapshots/` is filled by codegen (the snapshots it skipped against), `ww api save` (the crate's own), `ww api
//! cache` (every snapshot under a path) and a fetch that passed the checks. Every write is a whole file renamed into
//! place, so a reader never sees a partial one; a file with the same bytes is left alone. What is read from here is
//! untrusted and goes through the same checks as any other file ([crate::file]).

use crate::dir::Dir;
use crate::file::{self, Snapshot};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use ww_version::FullVersionOwned;

/// `snapshots/` under the cache root.
pub const SNAPSHOTS_SUBDIR: &str = "snapshots";

/// `snapshots/missing/`.
pub const MISSING_SUBDIR: &str = "missing";

/// `sources.toml` under the cache root.
pub const SOURCES_FILE: &str = "sources.toml";

/// Environment variable that turns the fetch off when set to anything but empty or `0`.
pub const OFFLINE_ENV: &str = "WW_OFFLINE";

/// `~/.wire_weaver`, `None` without a home directory.
pub fn default_root() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join(".wire_weaver"))
}

/// The snapshots directory of the cache rooted at `root`.
pub fn snapshots_dir(root: &Path) -> Dir {
    Dir::new(root.join(SNAPSHOTS_SUBDIR))
}

/// Writes `bytes` as `file_name` in `dir`, whole and renamed into place. `Ok(false)` when the file was already
/// there with the same bytes.
pub fn store_file(dir: &Path, file_name: &str, bytes: &[u8]) -> Result<bool> {
    let path = dir.join(file_name);
    if fs::read(&path).is_ok_and(|have| have == bytes) {
        return Ok(false);
    }
    fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let tmp = dir.join(format!("{file_name}.tmp{}", std::process::id()));
    fs::write(&tmp, bytes).with_context(|| tmp.display().to_string())?;
    fs::rename(&tmp, &path).with_context(|| path.display().to_string())?;
    Ok(true)
}

/// Stores a checked snapshot in `dir` under its file name.
pub fn store_snapshot(dir: &Dir, snapshot: &Snapshot) -> Result<bool> {
    store_file(
        dir.path(),
        &file::file_name(snapshot.version()),
        snapshot.text.as_bytes(),
    )
}

/// What [store_crate] did.
#[derive(Debug, Default)]
pub struct Stored {
    /// Snapshots written (not those already there).
    pub snapshots: Vec<FullVersionOwned>,
    /// Superseded definitions written.
    pub superseded: usize,
    /// Files refused, with why.
    pub refused: Vec<(PathBuf, String)>,
}

/// Copies every whole-crate snapshot of `from` (a crate's `api_snapshots/` or any directory in that layout) and the
/// definitions in its `superseded/` into `into`, each read and checked first.
pub fn store_crate(into: &Dir, from: &Dir) -> Result<Stored> {
    let mut stored = Stored::default();
    for (version, path) in from.files()? {
        if same_bytes(&path, &into.file_path(&version)) {
            continue;
        }
        match file::read(&path) {
            Ok(snapshot) => {
                if store_snapshot(into, &snapshot)? {
                    stored.snapshots.push(version);
                }
            }
            Err(e) => stored.refused.push((path, format!("{e:#}"))),
        }
    }
    let superseded_into = into.superseded_dir();
    for path in from.superseded_files()? {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if same_bytes(&path, &superseded_into.join(name)) {
            continue;
        }
        let bytes = match checked_superseded(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                stored.refused.push((path, format!("{e:#}")));
                continue;
            }
        };
        if store_file(&superseded_into, name, &bytes)? {
            stored.superseded += 1;
        }
    }
    Ok(stored)
}

/// Whether the file at `b` is there with the same bytes as `a`: already stored, nothing to check again. Sizes are
/// compared first, so a file past the limits is never read whole.
fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(size_a), Ok(size_b)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    size_a.len() == size_b.len()
        && size_a.len() <= crate::limits::SNAPSHOT_FILE_BYTES_MAX
        && fs::read(a).ok() == fs::read(b).ok()
}

/// The bytes of a superseded definition file, checked to decode and to be named by one of its own signatures.
fn checked_superseded(path: &Path) -> Result<Vec<u8>> {
    use ww_self::definition_cache::Kind;
    let len = fs::metadata(path)
        .with_context(|| path.display().to_string())?
        .len();
    if len > crate::limits::SUPERSEDED_FILE_BYTES_MAX {
        bail!("{}: {len} bytes, refused", path.display());
    }
    let bytes = fs::read(path).with_context(|| path.display().to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("file name")?;
    let kind = if name.contains("-trait-") {
        Kind::Trait
    } else {
        Kind::Type
    };
    let stem = name.strip_suffix(".ww").context("not a .ww file")?;
    let (rest, signature) = stem.rsplit_once('-').context("no signature in the name")?;
    let (_, def_name) = rest
        .split_once(&format!("-{}-", kind.name()))
        .context("no kind in the name")?;
    let def_name = def_name.replace('.', "::");
    let (_, signatures) =
        ww_self::definition_cache::decode_with_signatures(kind, &def_name, &bytes)
            .with_context(|| path.display().to_string())?;
    let hex = |s: &[u8]| s.iter().map(|b| format!("{b:02x}")).collect::<String>();
    if hex(&signatures.shape) != signature && hex(&signatures.with_docs) != signature {
        bail!(
            "{}: the bytes have neither signature the name says",
            path.display()
        );
    }
    Ok(bytes)
}

/// Remembers that a fetch of `version` found nothing, so it isn't tried again before [crate::limits::FETCH_RETRY_AFTER].
pub fn mark_missing(dir: &Dir, version: &FullVersionOwned) -> Result<()> {
    let missing = dir.path().join(MISSING_SUBDIR);
    fs::create_dir_all(&missing).with_context(|| missing.display().to_string())?;
    let path = missing.join(version.filename_friendly());
    fs::write(&path, b"").with_context(|| path.display().to_string())?;
    Ok(())
}

/// Whether a fetch of `version` found nothing less than `retry_after` ago.
pub fn missing_recently(dir: &Dir, version: &FullVersionOwned, retry_after: Duration) -> bool {
    let path = dir
        .path()
        .join(MISSING_SUBDIR)
        .join(version.filename_friendly());
    let Ok(modified) = fs::metadata(&path).and_then(|m| m.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age < retry_after)
}

/// Forgets a miss (a snapshot of `version` arrived another way).
pub fn clear_missing(dir: &Dir, version: &FullVersionOwned) {
    let path = dir
        .path()
        .join(MISSING_SUBDIR)
        .join(version.filename_friendly());
    let _ = fs::remove_file(path);
}

/// `sources.toml`: what the fetch may do.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    /// No fetch at all. [OFFLINE_ENV] set turns it on too.
    #[serde(default)]
    pub offline: bool,
    /// The git repository of a crate, by crate name, for crates not on crates.io.
    #[serde(default)]
    pub repositories: BTreeMap<String, Repository>,
}

/// Where a crate's source lives, for the fetch.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// `https://github.com/owner/repo`: only GitHub is fetched from today.
    pub url: String,
    /// Branch, tag or commit whose archive is fetched; the default branch when absent. Old versions' snapshots never
    /// leave a crate's `api_snapshots/`, so the default branch has every version's.
    #[serde(rename = "ref", default)]
    pub git_ref: Option<String>,
}

impl Sources {
    /// Reads `root/sources.toml`; a missing file is the default. [OFFLINE_ENV] overrides `offline`.
    pub fn read(root: &Path) -> Result<Sources> {
        let path = root.join(SOURCES_FILE);
        let mut sources = match fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str::<Sources>(&text).with_context(|| path.display().to_string())?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Sources::default(),
            Err(e) => bail!("{}: {e}", path.display()),
        };
        if std::env::var(OFFLINE_ENV).is_ok_and(|v| !v.is_empty() && v != "0") {
            sources.offline = true;
        }
        Ok(sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_file_reads_and_refuses_unknown_keys() {
        let text = "offline = true\n[repositories]\nmy_api = { url = \"https://github.com/o/r\", ref = \"main\" }\n";
        let sources: Sources = toml::from_str(text).unwrap();
        assert!(sources.offline);
        assert_eq!(
            sources.repositories["my_api"],
            Repository {
                url: "https://github.com/o/r".into(),
                git_ref: Some("main".into()),
            }
        );
        assert!(toml::from_str::<Sources>("online = true\n").is_err());
    }

    #[test]
    fn missing_markers_age() {
        let dir =
            std::env::temp_dir().join(format!("ww_snapshots_cache_test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let snapshots = snapshots_dir(&dir);
        let version = FullVersionOwned::new("x".into(), ww_version::VersionOwned::new(0, 1, 0));
        assert!(!missing_recently(
            &snapshots,
            &version,
            Duration::from_secs(60)
        ));
        mark_missing(&snapshots, &version).unwrap();
        assert!(missing_recently(
            &snapshots,
            &version,
            Duration::from_secs(60)
        ));
        assert!(!missing_recently(&snapshots, &version, Duration::ZERO));
        clear_missing(&snapshots, &version);
        assert!(!missing_recently(
            &snapshots,
            &version,
            Duration::from_secs(60)
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
