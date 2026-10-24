use super::check::print_report;
use anyhow::{Result, anyhow};
use console::style;
use std::path::PathBuf;
use wire_weaver_evolution::{Change, Check, SaveStatus};

pub(crate) fn save(crate_path: PathBuf, force: bool) -> Result<()> {
    // checked against the previous version's snapshot, only the current version's is written or refreshed
    let saved = Check::new(&crate_path).save(force).map_err(|e| {
        let hint = if e.contains("Not saved") {
            " Or pass --force to save anyway."
        } else {
            ""
        };
        anyhow!("{e}{hint}")
    })?;
    if let Some((old_path, report)) = &saved.previous
        && report.change != Change::None
        && saved.status != SaveStatus::UpToDate
    {
        print_report(report, old_path, "source");
    }
    if saved.status == SaveStatus::UpToDate {
        println!("{} is up to date", saved.path.display());
        return Ok(());
    }
    println!(
        "{} {}, {}",
        style(saved.status).green(),
        saved.path.display(),
        crate::api_tree::summary(&saved.bundle)
    );
    Ok(())
}
