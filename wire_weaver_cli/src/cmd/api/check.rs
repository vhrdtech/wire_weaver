use anyhow::{Context, Result, anyhow};
use console::style;
use std::fs;
use std::path::{Path, PathBuf};
use wire_weaver_client::evolution::{self, Change, Report};
use wire_weaver_client::ww_self::ApiBundleOwned;
use wire_weaver_client::ww_version::VersionOwned;

/// Directory inside a crate where its snapshots are kept.
pub(crate) const SNAPSHOTS_DIR: &str = "api_snapshots";

pub(crate) fn check(crate_path: PathBuf, against: Option<PathBuf>) -> Result<()> {
    let bundle = wire_weaver_core::load_crate(&crate_path)?;
    let version = &bundle.crate_version(0)?.version;
    let (old_path, old) = match against {
        Some(path) => {
            let old = parse(&path)?;
            (path, old)
        }
        None => {
            let dir = crate_path.join(SNAPSHOTS_DIR);
            let Some(previous) = latest_snapshot(&dir, &bundle, version, true)? else {
                println!(
                    "no snapshot of {} {} or earlier in {}, nothing to compare with",
                    bundle.ext_crates[0].crate_id,
                    triplet(version),
                    dir.display()
                );
                return Ok(());
            };
            previous
        }
    };
    let report = evolution::compare(&old, &bundle).map_err(|e| anyhow!(e))?;
    print_report(&report, &old_path);
    report.check_version().map_err(|e| anyhow!(e))?;
    if report.change != Change::None {
        println!("{}", style("version is bumped enough").green());
    }
    Ok(())
}

/// Print what changed between a snapshot and the crate source.
pub(crate) fn print_report(report: &Report, old_path: &Path) {
    println!(
        "{} {} ({}) -> {} (source): {}",
        report.new.crate_id,
        triplet(&report.old.version),
        old_path.display(),
        triplet(&report.new.version),
        match report.change {
            Change::Breaking => style(report.change).red(),
            Change::None => style(report.change).green(),
            _ => style(report.change).yellow(),
        }
    );
    for reason in &report.breaking {
        println!("  {} {reason}", style("breaking:").red());
    }
    for change in &report.compatible {
        println!("  {} {change}", style("compatible:").green());
    }
    for warning in &report.warnings {
        println!("  {} {warning}", style("warning:").yellow());
    }
}

/// Newest snapshot of the same crate saved in `dir`, with a version older than `version`, or the same one if
/// `or_same` is true.
pub(crate) fn latest_snapshot(
    dir: &Path,
    bundle: &ApiBundleOwned,
    version: &VersionOwned,
    or_same: bool,
) -> Result<Option<(PathBuf, ApiBundleOwned)>> {
    let crate_id = &bundle.crate_version(0)?.crate_id;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("reading {}", dir.display())),
    };
    let mut latest: Option<(PathBuf, ApiBundleOwned)> = None;
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let snapshot = parse(&path)?;
        let snapshot_version = &snapshot.crate_version(0)?;
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

fn parse(path: &Path) -> Result<ApiBundleOwned> {
    let contents = fs::read_to_string(path).context(format!("reading {}", path.display()))?;
    ron::from_str(&contents).context(format!("parsing {}", path.display()))
}

fn key(v: &VersionOwned) -> (u32, u32, u32) {
    (v.major.0, v.minor.0, v.patch.0)
}

pub(crate) fn triplet(v: &VersionOwned) -> String {
    format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0)
}
