//! A directory of snapshot files: a crate's `api_snapshots/`, the cache's `snapshots/`, or the same layout wherever
//! else it is kept. Whole-crate snapshots at the top (`<crate>_<x>_<y>_<z>.ron`), the definitions in-place refreshes
//! replaced in `superseded/` (EVO-68), per-root snapshots (`..@w2.1.ron`, EVO-38) ignored here: they serve a crate's
//! own evolution checks, while bundles name skipped items by crate version.

use crate::file::{self, Snapshot};
use crate::limits::{SUPERSEDED_FILE_BYTES_MAX, SUPERSEDED_FILES_MAX};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use ww_self::ApiBundleOwned;
use ww_self::definition_cache::{self, Kind};
use ww_version::FullVersionOwned;

/// The directory in a crate where `ww api save` keeps its snapshots.
pub const SNAPSHOTS_DIR: &str = "api_snapshots";

/// The directory inside one that keeps superseded definitions.
pub const SUPERSEDED_DIR: &str = "superseded";

/// A directory in the `api_snapshots/` layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dir {
    path: PathBuf,
}

impl Dir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Dir { path: path.into() }
    }

    /// `<crate_dir>/api_snapshots`.
    pub fn of_crate(crate_dir: &Path) -> Self {
        Dir::new(crate_dir.join(SNAPSHOTS_DIR))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the directory exists.
    pub fn exists(&self) -> bool {
        self.path.is_dir()
    }

    /// The path the snapshot of `version` has here, whether or not the file exists.
    pub fn file_path(&self, version: &FullVersionOwned) -> PathBuf {
        self.path.join(file::file_name(version))
    }

    /// `superseded/`.
    pub fn superseded_dir(&self) -> PathBuf {
        self.path.join(SUPERSEDED_DIR)
    }

    /// The checked snapshot of exactly `version`, `Ok(None)` when the directory has none.
    pub fn snapshot(&self, version: &FullVersionOwned) -> Result<Option<Snapshot>> {
        if !file::is_plain_crate_name(&version.crate_id) {
            bail!("`{}` is not a crate name, refused", version.crate_id);
        }
        file::read_if_exists(&self.file_path(version))
    }

    /// The canonical bundle of the `kind` named `name` of `version` that `reference` names, kept in `superseded/`
    /// (EVO-68), `Ok(None)` when there is none. Files are checked against the limits before they are read.
    pub fn superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Result<Option<ApiBundleOwned>> {
        let dir = self.superseded_dir();
        if !dir.is_dir() {
            return Ok(None);
        }
        check_superseded_dir(&dir)?;
        definition_cache::load(&dir, kind, version, name, reference)
    }

    /// Every whole-crate snapshot file here, with the version its name says, sorted by name.
    pub fn files(&self) -> Result<Vec<(FullVersionOwned, PathBuf)>> {
        let entries = match fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => bail!("{}: {e}", self.path.display()),
        };
        let mut files = vec![];
        for entry in entries {
            let entry = entry.with_context(|| self.path.display().to_string())?;
            let name = entry.file_name();
            let Some(version) = name.to_str().and_then(file::parse_file_name) else {
                continue;
            };
            if entry.path().is_file() {
                files.push((version, entry.path()));
            }
        }
        files.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(files)
    }

    /// The versions of [Self::files].
    pub fn versions(&self) -> Result<Vec<FullVersionOwned>> {
        Ok(self.files()?.into_iter().map(|(v, _)| v).collect())
    }

    /// Every superseded definition file here (`superseded/*.ww`), sorted by name.
    pub fn superseded_files(&self) -> Result<Vec<PathBuf>> {
        let dir = self.superseded_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => bail!("{}: {e}", dir.display()),
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "ww") && p.is_file())
            .collect();
        files.sort();
        Ok(files)
    }
}

/// Refuses a `superseded/` directory past the limits, before anything in it is read.
fn check_superseded_dir(dir: &Path) -> Result<()> {
    let mut count = 0;
    for entry in fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry.with_context(|| dir.display().to_string())?;
        count += 1;
        if count > SUPERSEDED_FILES_MAX {
            bail!(
                "{}: more than {SUPERSEDED_FILES_MAX} entries, refused",
                dir.display()
            );
        }
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if len > SUPERSEDED_FILE_BYTES_MAX {
            bail!(
                "{}: {len} bytes, more than a superseded definition can be ({SUPERSEDED_FILE_BYTES_MAX}), \
                 refused",
                entry.path().display()
            );
        }
    }
    Ok(())
}
