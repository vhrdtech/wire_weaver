//! Writing a [Set](crate::Set) at build time, for a binary or extension module that embeds the snapshots of the crates its
//! workspace uses (`ww`, the Python wheel): a device's skipped definitions then resolve with nothing on disk and no
//! network. A build script calls [workspace_set]; the crate includes the written file into a `static` and registers
//! it at start:
//!
//! ```text
//! // build.rs
//! ww_snapshots::build::workspace_set("stdlib_set.rs").unwrap_or_else(|e| panic!("{e:#}"));
//! // lib.rs
//! static STDLIB: ww_snapshots::Set = include!(concat!(env!("OUT_DIR"), "/stdlib_set.rs"));
//! ww_snapshots::embed(&STDLIB);
//! ```
//!
//! The RON of every snapshot is read and checked here ([crate::file]), the binary gets the shrink_wrap bytes only; a
//! superseded definition is checked ([crate::cache]) and embedded as it is. A file that fails its check fails the
//! build and is named. `cargo:rerun-if-changed` is printed for the lock and every `api_snapshots/` used.

use crate::cache;
use crate::cargo::CargoSources;
use crate::dir::Dir;
use crate::file;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use ww_version::FullVersionOwned;

/// What [write_set] wrote.
#[derive(Debug, Default)]
pub struct Written {
    /// The crate versions embedded, in order.
    pub snapshots: Vec<FullVersionOwned>,
    /// Superseded definitions embedded.
    pub superseded: usize,
    /// The `.rs` file holding the `Set::new(..)` expression.
    pub path: PathBuf,
}

/// Every crate the `Cargo.lock` above `CARGO_MANIFEST_DIR` lists, that Cargo has on this machine and whose directory
/// has an `api_snapshots/`, written as a [Set](crate::Set) to `OUT_DIR/<file_name>`. Called from a build script.
pub fn workspace_set(file_name: &str) -> Result<Written> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR is not set")?,
    );
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").context("OUT_DIR is not set")?);
    let mut sources = CargoSources::default();
    sources.entry_dir = Some(manifest_dir);
    println!("cargo:rerun-if-changed={}", sources.lock_path()?.display());
    let mut packages = sources.locked_package_dirs()?;
    packages.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let mut crate_dirs: Vec<PathBuf> = packages
        .into_iter()
        .map(|(_, _, dir)| dir)
        .filter(|dir| Dir::of_crate(dir).exists())
        .collect();
    crate_dirs.dedup();
    write_set(&out_dir, file_name, &crate_dirs)
}

/// The snapshots and superseded definitions in the `api_snapshots/` of every crate at `crate_dirs`, as a
/// [Set](crate::Set) in `out_dir/file_name` with its `.bin` and `.ww` files next to it. A version met twice is
/// embedded from the first directory.
pub fn write_set(out_dir: &Path, file_name: &str, crate_dirs: &[PathBuf]) -> Result<Written> {
    fs::create_dir_all(out_dir).with_context(|| out_dir.display().to_string())?;
    let mut written = Written {
        path: out_dir.join(file_name),
        ..Written::default()
    };
    let mut snapshots = String::new();
    let mut superseded = String::new();
    let mut seen_superseded = HashSet::new();
    for crate_dir in crate_dirs {
        let dir = Dir::of_crate(crate_dir);
        println!("cargo:rerun-if-changed={}", dir.path().display());
        for (version, path) in dir.files()? {
            if written.snapshots.contains(&version) {
                continue;
            }
            let snapshot = file::read(&path)?;
            if snapshot.version() != &version {
                bail!(
                    "{}: a snapshot of {} {}",
                    path.display(),
                    snapshot.version().crate_id,
                    file::triplet(&snapshot.version().version)
                );
            }
            let name = file::file_name(&version);
            let bin = out_dir.join(format!("{name}.bin"));
            fs::write(&bin, &snapshot.bytes).with_context(|| bin.display().to_string())?;
            writeln!(
                snapshots,
                "        ({name:?}, include_bytes!({:?})),",
                bin.display()
            )?;
            written.snapshots.push(version);
        }
        let superseded_dir = dir.superseded_dir();
        if superseded_dir.is_dir() {
            println!("cargo:rerun-if-changed={}", superseded_dir.display());
        }
        for path in dir.superseded_files()? {
            let bytes = fs::read(&path).with_context(|| path.display().to_string())?;
            let given = path
                .file_name()
                .and_then(|n| n.to_str())
                .context("file name")?;
            let name = cache::check_superseded(given, &bytes)
                .with_context(|| path.display().to_string())?;
            if !seen_superseded.insert(name.clone()) {
                continue;
            }
            let copy = out_dir.join(&name);
            fs::write(&copy, &bytes).with_context(|| copy.display().to_string())?;
            writeln!(
                superseded,
                "        ({name:?}, include_bytes!({:?})),",
                copy.display()
            )?;
            written.superseded += 1;
        }
    }
    let text = format!(
        "ww_snapshots::Set::new(\n    &[\n{snapshots}    ],\n    &[\n{superseded}    ],\n)\n"
    );
    fs::write(&written.path, text).with_context(|| written.path.display().to_string())?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ww_global's own snapshots, as `ww` embeds them: every version, its superseded definitions, the files next to
    /// the written expression.
    #[test]
    fn a_crates_snapshots_become_a_set() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let out = std::env::temp_dir().join(format!("ww_snapshots_build_{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        let written = write_set(
            &out,
            "set.rs",
            &[root.join("ww_global"), root.join("ww_global")],
        )
        .unwrap();
        let dir = Dir::of_crate(&root.join("ww_global"));
        assert_eq!(written.snapshots.len(), dir.versions().unwrap().len());
        assert_eq!(written.superseded, dir.superseded_files().unwrap().len());
        let text = fs::read_to_string(&written.path).unwrap();
        assert!(text.starts_with("ww_snapshots::Set::new(\n"), "{text}");
        for version in &written.snapshots {
            let name = file::file_name(version);
            assert!(
                text.contains(&format!("({name:?}, include_bytes!(")),
                "{text}"
            );
            assert!(out.join(format!("{name}.bin")).is_file());
        }
        fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn the_workspace_lock_places_its_crates() {
        let mut sources = CargoSources::default();
        sources.entry_dir = Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let dirs = sources.locked_package_dirs().unwrap();
        let of = |name: &str| {
            dirs.iter()
                .find(|(n, _, _)| n == name)
                .map(|(_, _, d)| d.clone())
        };
        assert!(of("ww_global").is_some_and(|d| Dir::of_crate(&d).exists()));
        assert!(of("exact_units").is_some_and(|d| Dir::of_crate(&d).exists()));
        assert!(of("semver").is_some_and(|d| d.join("Cargo.toml").is_file()));
    }
}
