use anyhow::{Result, anyhow};
use console::style;
use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::triplet;
use wire_weaver_evolution::{Change, Check, Comparison, Outcome, Report};

pub(crate) fn check(path: PathBuf, against: Option<PathBuf>, strict: bool) -> Result<()> {
    let outcome = load(path, against)?
        .strict(strict)
        .run()
        .map_err(|e| anyhow!(e))?;
    if let Outcome::Compared(c) = &outcome {
        print_report(&c.report, &c.old_path, &c.new_label);
    }
    match outcome.verdict() {
        Ok(ok) if matches!(outcome, Outcome::NoSnapshot(_)) => println!("{ok}"),
        Ok(ok) => {
            if let Outcome::Compared(c) = &outcome
                && c.report.change != Change::None
            {
                println!("{}", style(ok).green());
            }
        }
        Err(e) => return Err(anyhow!(e)),
    }
    Ok(())
}

/// Old and new versions of a crate to compare, as `ww api check` does; `None` (with a note printed) if there is no
/// snapshot to compare with.
pub(crate) fn compared(path: PathBuf, against: Option<PathBuf>) -> Result<Option<Comparison>> {
    match load(path, against)?.run().map_err(|e| anyhow!(e))? {
        Outcome::Compared(c) => Ok(Some(*c)),
        outcome @ Outcome::NoSnapshot(_) => {
            println!("{}", outcome.verdict().map_err(|e| anyhow!(e))?);
            Ok(None)
        }
    }
}

fn load(path: PathBuf, against: Option<PathBuf>) -> Result<Check> {
    let check = Check::new(path);
    Ok(match against {
        Some(against) => check.against(against),
        None => check,
    })
}

/// Print what changed between a snapshot and a newer version.
pub(crate) fn print_report(report: &Report, old_path: &Path, new_label: &str) {
    println!(
        "{} {} ({}) -> {} ({new_label}): {}",
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
    for finding in &report.breaking {
        println!("  {} {}", style("breaking:").red(), finding.message);
        println!(
            "      {} {}",
            style("rule:").dim(),
            style(finding.rule).dim()
        );
    }
    for change in &report.compatible {
        println!("  {} {change}", style("compatible:").green());
    }
    for warning in &report.warnings {
        println!("  {} {warning}", style("warning:").yellow());
    }
}
