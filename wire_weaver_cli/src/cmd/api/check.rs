use anyhow::{Result, anyhow};
use console::style;
use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::triplet;
use wire_weaver_evolution::{Change, Check, Comparison, Outcome, Report, RootsOutcome};

pub(crate) fn check(
    path: PathBuf,
    against: Option<PathBuf>,
    strict: bool,
    require_current: bool,
) -> Result<()> {
    print_missing_test_warning(&path);
    let outcome = load(path, against)?
        .strict(strict)
        .require_current(require_current)
        .run()
        .map_err(|e| anyhow!(e))?;
    match &outcome {
        Outcome::Compared(c) => print_report(&c.report, &c.old_path, &c.new_label),
        Outcome::Roots(r) => print_roots(r),
        Outcome::NoSnapshot(_) => {}
    }
    match outcome.verdict() {
        Ok(ok) if matches!(outcome, Outcome::NoSnapshot(_)) => println!("{ok}"),
        Ok(ok) => {
            let changed = match &outcome {
                Outcome::Compared(c) => c.report.change != Change::None || c.design_line,
                Outcome::Roots(r) => r.changed(),
                Outcome::NoSnapshot(_) => false,
            };
            if changed {
                println!("{}", style(ok).green());
            }
        }
        Err(e) => return Err(anyhow!(e)),
    }
    for warning in outcome.warnings() {
        println!("{} {warning}", style("warning:").yellow());
    }
    print_hints(outcome.hints());
    Ok(())
}

/// A crate directory (not a snapshot file) without its own `tests/evolution.rs` gets a warning: `ww api check` is
/// run by hand, a missing test means `cargo test` never catches a wire break.
pub(crate) fn print_missing_test_warning(path: &Path) {
    if path.is_dir()
        && let Some(warning) = wire_weaver_evolution::missing_test_warning(path)
    {
        println!("{} {warning}", style("warning:").yellow());
    }
}

/// TOOL-22: unit-suffix hints, never affecting the verdict.
fn print_hints(hints: &[wire_weaver_evolution::UnitHint]) {
    for hint in hints {
        println!("  {} {hint}", style("note:").cyan());
    }
}

/// Old and new versions of a crate to compare, as `ww api check` does, one per API root of a crate with several
/// (named, EVO-39); empty (with a note printed) if there is no snapshot to compare with.
pub(crate) fn compared(
    path: PathBuf,
    against: Option<PathBuf>,
) -> Result<Vec<(Option<String>, Comparison)>> {
    match load(path, against)?.run().map_err(|e| anyhow!(e))? {
        Outcome::Compared(c) => Ok(vec![(None, *c)]),
        outcome @ Outcome::NoSnapshot(_) => {
            println!("{}", outcome.verdict().map_err(|e| anyhow!(e))?);
            Ok(vec![])
        }
        Outcome::Roots(roots) => {
            let mut compared = vec![];
            for root in roots.roots {
                let label = format!("API root `{}` ({})", root.root, root.wire);
                match root.outcome {
                    Outcome::Compared(c) => compared.push((Some(label), *c)),
                    outcome => println!("{label}: {outcome}"),
                }
            }
            Ok(compared)
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

/// Print each API root's report under its name, then the removed roots, the shared items that changed and the
/// crate version all of it needs (EVO-39).
fn print_roots(roots: &RootsOutcome) {
    for root in &roots.roots {
        println!(
            "{} {}",
            style(format!("API root `{}`", root.root)).bold(),
            style(format!("({})", root.wire)).dim()
        );
        match &root.outcome {
            Outcome::Compared(c) => print_report(&c.report, &c.old_path, &c.new_label),
            outcome => println!("  {outcome}"),
        }
    }
    for removed in &roots.removed {
        match &removed.recorded {
            Some(record) => println!(
                "{} `{}`: recorded in {}",
                style("removed API root").bold(),
                removed.root,
                record.display()
            ),
            None => println!(
                "{} `{}`: last snapshot {}, wire {}, needs crate {} or later",
                style("removed API root").red(),
                removed.root,
                removed.last.display(),
                removed.wire,
                removed
                    .required()
                    .map_or("(no bump)".to_string(), |v| triplet(&v))
            ),
        }
    }
    for shared in &roots.shared {
        println!("{} {shared}", style("note:").cyan());
    }
    if roots.changed()
        && let Some(min) = roots.minimal_version()
    {
        println!(
            "{} {} or later, the largest its roots need",
            style("crate:").bold(),
            triplet(&min)
        );
    }
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
    // both bumps the changes need, the wire version's and the crate's (EVO-27)
    println!("  {} {}", style("needs:").bold(), report.required());
    for finding in &report.breaking {
        // a break of Rust code only, devices and hosts of both versions still talk (EVO-22)
        let label = match finding.rule.keeps_wire() {
            true => "breaking (Rust only, the wire is compatible):",
            false => "breaking:",
        };
        println!("  {} {}", style(label).red(), finding.message);
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
    // across a wire break, resources of the same name elsewhere are never taken for the same ones (EVO-40)
    for proposal in &report.proposals {
        println!("  {} {proposal}", style("proposal:").cyan());
    }
    if report.across_line {
        println!(
            "  {}",
            style(
                "resources matched by path and #[moved_from] across the wire break, not by position"
            )
            .dim()
        );
    }
}
