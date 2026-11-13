use super::check::{print_missing_test_warning, print_report};
use anyhow::{Result, anyhow};
use console::style;
use std::path::PathBuf;
use wire_weaver_evolution::{Change, Check, SaveStatus};

pub(crate) fn save(
    crate_path: PathBuf,
    force: bool,
    dep: Option<String>,
    snapshots_dir: Option<PathBuf>,
) -> Result<()> {
    let crate_path = match dep {
        Some(dep) => wire_weaver_core::dependency_dir(&crate_path, &dep)?,
        None => crate_path,
    };
    print_missing_test_warning(&crate_path);
    let mut check = Check::new(&crate_path);
    if let Some(dir) = snapshots_dir {
        check = check.snapshots_dir(dir);
    }
    // checked against the previous version's snapshot, only the current version's is written or refreshed
    let saved = check.save(force).map_err(|e| {
        let hint = if e.contains("Not saved") {
            " Or pass --force to save anyway."
        } else {
            ""
        };
        anyhow!("{e}{hint}")
    })?;
    // one per API root for a crate with several (EVO-38)
    for saved in &saved {
        let root = match &saved.root {
            Some(root) => format!("{root}: "),
            None => String::new(),
        };
        if let Some((old_path, report)) = &saved.previous
            && report.change != Change::None
            && saved.status != SaveStatus::UpToDate
        {
            print_report(report, old_path, "source");
        }
        for warning in &saved.warnings {
            println!("{} {warning}", style("warning:").yellow());
        }
        if saved.status == SaveStatus::DesignLine {
            let wire = saved
                .bundle
                .wire
                .map_or("w0".to_string(), |w| w.to_string());
            println!(
                "{root}not saved: {wire} is the design line, which keeps no snapshots (docs/evolution/rules.md#w0-the-design-line); \
                 the first is saved once the API moves to w1.0 at its first release"
            );
            continue;
        }
        if saved.status == SaveStatus::Removed {
            // a root the `roots` table no longer lists (EVO-39)
            println!(
                "{root}{}, recorded in {}",
                style(saved.status).green(),
                saved.path.display()
            );
            continue;
        }
        if saved.status == SaveStatus::UpToDate {
            println!("{root}{} is up to date", saved.path.display());
            continue;
        }
        println!(
            "{root}{} {}, {}",
            style(saved.status).green(),
            saved.path.display(),
            crate::api_tree::summary(&saved.bundle)
        );
        // what other snapshots and devices still name by its old signature (EVO-68)
        for kept in &saved.superseded {
            println!(
                "{root}kept the {} {} it replaced in {}",
                kept.kind.name(),
                kept.name,
                kept.path.display()
            );
        }
    }
    Ok(())
}

/// `ww api keep-superseded`: see [wire_weaver_evolution::superseded::keep_superseded].
pub(crate) fn keep_superseded(old: PathBuf, current: PathBuf, into: PathBuf) -> Result<()> {
    let read = |path: &PathBuf| {
        wire_weaver_evolution::snapshot_files::parse(path)
            .map_err(|e| anyhow!("{}: {e}", path.display()))
    };
    let (old, current) = (read(&old)?, read(&current)?);
    let (kept, warnings) =
        wire_weaver_evolution::superseded::keep_superseded(&into, &old, &current)
            .map_err(|e| anyhow!(e))?;
    for warning in &warnings {
        println!("{} {warning}", style("warning:").yellow());
    }
    for kept in &kept {
        println!(
            "kept {} {} in {}",
            kept.kind.name(),
            kept.name,
            kept.path.display()
        );
    }
    if kept.is_empty() {
        println!(
            "nothing to keep: the current snapshot has every definition of the earlier one, or they are kept"
        );
    }
    Ok(())
}

pub(crate) fn refresh_pins(
    path: PathBuf,
    snapshot: Option<PathBuf>,
    accept_breaking: bool,
    dry_run: bool,
) -> Result<()> {
    let refreshed = wire_weaver_evolution::pins::refresh(
        &path,
        snapshot.as_deref(),
        accept_breaking,
        dry_run,
        &Default::default(),
    )
    .map_err(|e| anyhow!(e))?;
    for pin in &refreshed.refreshed {
        println!("{}: {} -> {}", pin.what, pin.from, pin.to);
    }
    let file = refreshed.path.display();
    match (refreshed.refreshed.is_empty(), dry_run) {
        (true, _) => println!("{file}: pins are current"),
        (false, true) => println!("{file}: would refresh {} pins", refreshed.refreshed.len()),
        (false, false) => println!("{file}: refreshed {} pins", refreshed.refreshed.len()),
    }
    Ok(())
}
