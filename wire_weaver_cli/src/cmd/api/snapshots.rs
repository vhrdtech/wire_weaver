//! `ww api cache` and `ww api fetch`: filling this user's snapshot cache by hand (EVO-80), for a device whose API
//! refers to a crate no source on this machine has a snapshot of.

use anyhow::{Context, Result, anyhow, bail};
use std::path::PathBuf;
use ww_snapshots::cargo::packages_under;
use ww_snapshots::ww_version::{FullVersionOwned, VersionOwned};
use ww_snapshots::{Dir, Origin, Resolver, cache, limits};

/// Copies the `api_snapshots/` of every crate under each of `paths` into `~/.wire_weaver/snapshots/`, each file
/// checked first.
pub(crate) fn cache(paths: Vec<PathBuf>) -> Result<()> {
    let root = cache::default_root().context("no home directory: nowhere to cache into")?;
    let into = cache::snapshots_dir(&root);
    let mut found = 0;
    for path in paths {
        let mut dirs: Vec<Dir> = vec![];
        if path
            .file_name()
            .is_some_and(|n| n == ww_snapshots::SNAPSHOTS_DIR)
            && path.is_dir()
        {
            // the `api_snapshots/` directory itself
            dirs.push(Dir::new(&path));
        }
        let mut budget = limits::CARGO_TREE_DIRS_MAX;
        dirs.extend(
            packages_under(&path, &mut budget)
                .with_context(|| path.display().to_string())?
                .into_iter()
                .map(|p| Dir::of_crate(&p.dir))
                .filter(Dir::exists),
        );
        for dir in dirs {
            let stored = cache::store_crate(&into, &dir)?;
            found += 1;
            println!(
                "{}: {} snapshots and {} superseded definitions new in the cache",
                dir.path().display(),
                stored.snapshots.len(),
                stored.superseded
            );
            for (path, why) in &stored.refused {
                eprintln!("  refused {}: {why}", path.display());
            }
        }
    }
    if found == 0 {
        bail!("no `api_snapshots/` under the given paths");
    }
    println!("into {}", into.path().display());
    Ok(())
}

/// The snapshot of `crate_name` `version` through the host's chain: what this machine has first, then the network;
/// what is found outside the cache is stored in it.
pub(crate) fn fetch(crate_name: String, version: String) -> Result<()> {
    let numbers: Vec<u32> = version
        .split('.')
        .map(|n| n.parse::<u32>().ok())
        .collect::<Option<_>>()
        .filter(|n: &Vec<u32>| n.len() == 3)
        .ok_or_else(|| anyhow!("version `{version}`: expected `MAJOR.MINOR.PATCH`"))?;
    let wanted = FullVersionOwned::new(
        crate_name.clone(),
        VersionOwned::new(numbers[0], numbers[1], numbers[2]),
    );
    if let Some(root) = cache::default_root() {
        cache::clear_missing(&cache::snapshots_dir(&root), &wanted);
    }
    let resolver = Resolver::global();
    resolver.forget_misses();
    if resolver.snapshot(&wanted).is_none() {
        bail!(
            "{crate_name} {version}: no snapshot found: none embedded, none in the cache or in Cargo's sources, \
             nothing fetched (the log names each URL tried; WW_OFFLINE or `offline` in sources.toml turns the \
             fetch off)"
        );
    }
    let from = match resolver.origin(&wanted) {
        Some(Origin::Set(_)) => "embedded in ww".to_owned(),
        Some(Origin::Dir(dir)) => format!("in {}", dir.display()),
        Some(Origin::Cache) => "already in the cache".to_owned(),
        Some(Origin::Cargo(dir)) => {
            format!(
                "in Cargo's sources at {}, copied into the cache",
                dir.display()
            )
        }
        Some(Origin::Fetched) => "fetched and stored in the cache".to_owned(),
        None => "found".to_owned(),
    };
    println!("{crate_name} {version}: {from}");
    Ok(())
}
