//! `ww api check`: compares an API crate with its snapshots and says which version bumps its changes need.

use anyhow::{Result, anyhow};
use console::style;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::triplet;
use wire_weaver_evolution::{
    Change, Check, Comparison, Outcome, Report, RootsOutcome, unreasoned_message,
};

pub(crate) fn check(path: PathBuf, options: CheckOptions) -> Result<()> {
    let checked = render(path, options)?;
    print!("{}", checked.text);
    checked.verdict.map_err(|e| anyhow!(e))
}

/// What `ww api check` compares a crate with and how strictly, its flags besides the crate's path.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// The snapshot or crate to compare with instead of the crate's newest older snapshot (`--against`).
    pub against: Option<PathBuf>,
    /// Fail on a change that has no reason written next to it (`--strict`).
    pub strict: bool,
    /// Fail when the current version's snapshot is missing or differs from the source (`--require-current`).
    pub require_current: bool,
}

/// What `ww api check` printed and how it ended.
#[derive(Debug)]
pub struct Checked {
    /// The text it prints on stdout.
    pub text: String,
    /// `Err` with the message it fails with after the text, `Ok` when it exits with 0.
    pub verdict: Result<(), String>,
}

/// The text `ww api check` prints for the crate at `path`, and its verdict. Colours follow
/// [console::colors_enabled]. For a program or a test that shows the same text, such as a book's transcript; `Err`
/// only when the crate or the snapshot can't be loaded.
pub fn render(path: PathBuf, options: CheckOptions) -> Result<Checked> {
    let mut out = String::new();
    write_missing_test_warning(&mut out, &path)?;
    let outcome = load(path, options.against)?
        .strict(options.strict)
        .require_current(options.require_current)
        .run()
        .map_err(|e| anyhow!(e))?;
    // TOOL-35: --require-current alone doesn't need a reason (the documented tests/evolution.rs template uses
    // it, and external users aren't forced into writing one); only --strict does.
    if options.strict
        && let Some(message) = unreasoned_message(outcome.unreasoned())
    {
        return Ok(Checked {
            text: out,
            verdict: Err(message),
        });
    }
    match &outcome {
        Outcome::Compared(c) => write_report(&mut out, &c.report, &c.old_path, &c.new_label)?,
        Outcome::Roots(r) => write_roots(&mut out, r)?,
        Outcome::NoSnapshot(_) => {}
    }
    match outcome.verdict() {
        Ok(ok) if matches!(outcome, Outcome::NoSnapshot(_)) => writeln!(out, "{ok}")?,
        Ok(ok) => {
            let changed = match &outcome {
                Outcome::Compared(c) => c.report.change != Change::None || c.design_line,
                Outcome::Roots(r) => r.changed(),
                Outcome::NoSnapshot(_) => false,
            };
            if changed {
                writeln!(out, "{}", style(ok).green())?;
            }
        }
        Err(e) => {
            return Ok(Checked {
                text: out,
                verdict: Err(e.to_string()),
            });
        }
    }
    for warning in outcome.warnings() {
        writeln!(out, "{} {warning}", style("warning:").yellow())?;
    }
    write_hints(&mut out, outcome.hints())?;
    Ok(Checked {
        text: out,
        verdict: Ok(()),
    })
}

/// A crate directory (not a snapshot file) without its own `tests/evolution.rs` gets a warning: `ww api check` is
/// run by hand, a missing test means `cargo test` never catches a wire break.
pub(crate) fn print_missing_test_warning(path: &Path) {
    let mut out = String::new();
    write_missing_test_warning(&mut out, path).expect("writing to a String never fails");
    print!("{out}");
}

fn write_missing_test_warning(out: &mut String, path: &Path) -> std::fmt::Result {
    if path.is_dir()
        && let Some(warning) = wire_weaver_evolution::missing_test_warning(path)
    {
        writeln!(out, "{} {warning}", style("warning:").yellow())?;
    }
    Ok(())
}

/// TOOL-22: unit-suffix hints, never affecting the verdict.
fn write_hints(out: &mut String, hints: &[wire_weaver_evolution::UnitHint]) -> std::fmt::Result {
    for hint in hints {
        writeln!(out, "  {} {hint}", style("note:").cyan())?;
    }
    Ok(())
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
fn write_roots(out: &mut String, roots: &RootsOutcome) -> std::fmt::Result {
    for root in &roots.roots {
        writeln!(
            out,
            "{} {}",
            style(format!("API root `{}`", root.root)).bold(),
            style(format!("({})", root.wire)).dim()
        )?;
        match &root.outcome {
            Outcome::Compared(c) => write_report(out, &c.report, &c.old_path, &c.new_label)?,
            outcome => writeln!(out, "  {outcome}")?,
        }
    }
    for removed in &roots.removed {
        match &removed.recorded {
            Some(record) => writeln!(
                out,
                "{} `{}`: recorded in {}",
                style("removed API root").bold(),
                removed.root,
                record.display()
            )?,
            None => writeln!(
                out,
                "{} `{}`: last snapshot {}, wire {}, needs crate {} or later",
                style("removed API root").red(),
                removed.root,
                removed.last.display(),
                removed.wire,
                removed
                    .required()
                    .map_or("(no bump)".to_string(), |v| triplet(&v))
            )?,
        }
    }
    for shared in &roots.shared {
        writeln!(out, "{} {shared}", style("note:").cyan())?;
    }
    if roots.changed()
        && let Some(min) = roots.minimal_version()
    {
        writeln!(
            out,
            "{} {} or later, the largest its roots need",
            style("crate:").bold(),
            triplet(&min)
        )?;
    }
    Ok(())
}

/// Print what changed between a snapshot and a newer version.
pub(crate) fn print_report(report: &Report, old_path: &Path, new_label: &str) {
    let mut out = String::new();
    write_report(&mut out, report, old_path, new_label).expect("writing to a String never fails");
    print!("{out}");
}

/// Write what changed between a snapshot and a newer version.
fn write_report(
    out: &mut String,
    report: &Report,
    old_path: &Path,
    new_label: &str,
) -> std::fmt::Result {
    writeln!(
        out,
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
    )?;
    // both bumps the changes need, the wire version's and the crate's (EVO-27)
    writeln!(out, "  {} {}", style("needs:").bold(), report.required())?;
    for finding in &report.breaking {
        // a break of Rust code only, devices and hosts of both versions still talk (EVO-22)
        let label = match finding.rule.keeps_wire() {
            true => "breaking (Rust only, the wire is compatible):",
            false => "breaking:",
        };
        writeln!(out, "  {} {}", style(label).red(), finding.message)?;
        writeln!(
            out,
            "      {} {}",
            style("rule:").dim(),
            style(finding.rule).dim()
        )?;
    }
    for change in &report.compatible {
        writeln!(out, "  {} {change}", style("compatible:").green())?;
    }
    for warning in &report.warnings {
        writeln!(out, "  {} {warning}", style("warning:").yellow())?;
    }
    // a dependency's doc comments changed, its definition did not (EVO-69)
    for note in &report.notes {
        writeln!(out, "  {} {note}", style("note:").dim())?;
    }
    // across a wire break, resources of the same name elsewhere are never taken for the same ones (EVO-40)
    for proposal in &report.proposals {
        writeln!(out, "  {} {proposal}", style("proposal:").cyan())?;
    }
    if report.across_line {
        writeln!(
            out,
            "  {}",
            style(
                "resources matched by path and #[moved_from] across the wire break, not by position"
            )
            .dim()
        )?;
    }
    Ok(())
}
