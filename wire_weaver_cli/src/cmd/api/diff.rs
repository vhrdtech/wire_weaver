use super::check::{Versions, triplet};
use anyhow::{Result, anyhow};
use console::style;
use std::path::PathBuf;
use wire_weaver_client::evolution::{self, Change, Difference};

pub(crate) fn diff(path: PathBuf, against: Option<PathBuf>) -> Result<()> {
    let Some(versions) = Versions::load(path, against)? else {
        return Ok(());
    };
    let differences = evolution::diff(&versions.old, &versions.new).map_err(|e| anyhow!(e))?;
    let report = evolution::compare(&versions.old, &versions.new).map_err(|e| anyhow!(e))?;
    println!(
        "{} {} ({}) -> {} ({})",
        report.new.crate_id,
        triplet(&report.old.version),
        versions.old_path.display(),
        triplet(&report.new.version),
        versions.new_label,
    );
    for difference in &differences {
        print_difference(difference);
    }
    let verdict = match report.change {
        Change::Breaking => style(report.change).red(),
        Change::None => style(report.change).green(),
        _ => style(report.change).yellow(),
    };
    if report.change == Change::None {
        println!("{verdict}");
    } else if report.check_version().is_ok() {
        println!("{verdict}, version is bumped enough");
    } else {
        // `ww api check` tells why, a diff is only informational
        let minimal = style(format!("{} or later", triplet(&report.minimal_version()))).red();
        println!("{verdict}, version has to be {minimal} (run `ww api check` for details)");
    }
    Ok(())
}

fn print_difference(difference: &Difference) {
    match difference {
        Difference::Added { path, definition } => {
            println!("  {} {path}: {definition}", style("+").green());
        }
        Difference::Removed { path, definition } => {
            println!("  {} {path}: {definition}", style("-").red());
        }
        Difference::Changed {
            path,
            what,
            old,
            new,
        } => {
            println!(
                "  {} {path}: {what} {} -> {}",
                style("~").yellow(),
                style(old).red(),
                style(new).green()
            );
        }
        Difference::Docs { path, old, new } => {
            println!("  {} {path}: docs", style("~").yellow());
            let doc = |line: &str| format!("/// {line}").trim_end().to_string();
            for line in diff_lines(old, new) {
                match line {
                    Line::Same(l) => println!("      {}", style(doc(l)).dim()),
                    Line::Removed(l) => {
                        println!("    {} {}", style("-").red(), style(doc(l)).red())
                    }
                    Line::Added(l) => {
                        println!("    {} {}", style("+").green(), style(doc(l)).green())
                    }
                }
            }
        }
    }
}

enum Line<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

/// Line diff by the longest common subsequence, doc comments are short.
fn diff_lines<'a>(old: &'a [String], new: &'a [String]) -> Vec<Line<'a>> {
    // lcs[i][j]: longest common subsequence of old[i..] and new[j..]
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = vec![];
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            lines.push(Line::Same(&old[i]));
            (i, j) = (i + 1, j + 1);
        } else if j == new.len() || (i < old.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            lines.push(Line::Removed(&old[i]));
            i += 1;
        } else {
            lines.push(Line::Added(&new[j]));
            j += 1;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_line_diff() {
        let old = ["a", "b", "c"].map(String::from);
        let new = ["a", "x", "c", "d"].map(String::from);
        let rendered: Vec<String> = diff_lines(&old, &new)
            .into_iter()
            .map(|l| match l {
                Line::Same(l) => format!(" {l}"),
                Line::Removed(l) => format!("-{l}"),
                Line::Added(l) => format!("+{l}"),
            })
            .collect();
        assert_eq!(rendered, [" a", "-b", "+x", " c", "+d"]);
    }
}
