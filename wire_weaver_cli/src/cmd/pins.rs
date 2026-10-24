//! `ww pins`: pin tests on any device serving `ww_pin_diag::PinDiag` (TOOL-15). The tests, plan files and the
//! device side are `ww_pin_probe`; this is the command line and the table.

use crate::cmd::pins_tui;
use crate::complete;
use crate::untrusted::escape;
use crate::util::StopSignal;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum, ValueHint};
use clap_complete::ArgValueCandidates;
use console::{StyledObject, measure_text_width, style};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use wire_weaver_client::DynClient;
use ww_pin_probe::bank::BankPins;
use ww_pin_probe::board::Board;
use ww_pin_probe::device::{DevicePins, Names, location_name, locations};
use ww_pin_probe::plan::{self, Plan};
use ww_pin_probe::probe::{Check, Report, Setup, Stopped, run};
use ww_pin_probe::select::select;
use ww_pin_probe::{Level, Point, PointCaps};

#[derive(Subcommand)]
pub(crate) enum PinsCommand {
    /// The device's points: id, name, group and what each can do
    List(ListArgs),

    /// Shorts to GND, to the supply and between points, and pulls, on the listed points
    ///
    /// Per point, from what it can do: pulls (read with the pull-up, then the pull-down, driving the opposite level
    /// first: external pull-up, pull-down, floating or held), short to GND (drive high weakly, read back), short to
    /// the supply (drive low, read back); then shorts between the points (each driven low alone with the others on
    /// pull-up, then high alone with the others on pull-down). What a point can't do is shown as not checked, a
    /// refusal of the firmware as refused. Every point is given back at the end, also on Ctrl-C and errors. Exit
    /// code 0 when nothing is shorted (and, with --plan, every expectation is met). Examples:
    ///
    ///   ww pins test GPIO0,GPIO5,P8           names and point ids
    ///   ww pins test GPIO2..=GPIO7 'chain.*'  a range of names, all points of a group
    ///   ww pins test --plan b200a.toml        expectations and rules from a plan
    ///   ww pins test GPIO0..=GPIO9 --save b200a.toml   write what this board shows into the plan
    #[command(verbatim_doc_comment)]
    Test(TestArgs),

    /// Every point live, laid out per connector from a board preset: drive, pull, read, release
    ///
    /// The points of PinDiag, or the pins of the device's ww_gpio banks when it has no PinDiag (or with --gpio):
    /// then each pin's mode and pull are saved when it is first set and put back on release.
    ///
    /// Reads all points every --interval-ms and shows each on its pad: ● high, ○ low, × refused; what was set
    /// next to it: ▲ ▼ driving high or low, ↑ ↓ pull-up or pull-down, → input. Keys: arrows or hjkl move, Tab the
    /// next connector, 1 / 0 drive high / low, t toggle, u / d / n input with pull-up / pull-down / none, w weak
    /// drive, r read now, x release all, p pause reading, q quit. Every point is released on quit, Ctrl-C and errors.
    /// The preset is a boardmap board description (with its geometry JSON) or a small preset for ww alone, see
    /// docs/cli_reference.md#ww-pins-tui. Without one, a panel per group. Examples:
    ///
    ///   ww pins tui --board docs/board/nucleo_g0b1re.toml   in a nucleo_g0b1re_template checkout
    ///   ww pins tui                                         a panel per group of points
    #[command(verbatim_doc_comment)]
    Tui(TuiArgs),
}

#[derive(Args)]
pub(crate) struct TuiArgs {
    /// Board preset: a boardmap board description (`[[view.connector]]`, `[aliases]`, `[pins]`, its geometry JSON)
    #[arg(long, value_hint = ValueHint::FilePath)]
    board: Option<PathBuf>,

    /// Where PinDiag is in the device's API (`diag.pins`), when it has more than one
    #[arg(long)]
    at: Option<String>,

    /// How often all points are read, ms
    #[arg(long, default_value_t = 250)]
    interval_ms: u64,

    /// The pins of the device's ww_gpio banks, also when it serves PinDiag (without PinDiag they are used anyway)
    #[arg(long, conflicts_with = "at")]
    gpio: bool,
}

#[derive(Args)]
pub(crate) struct ListArgs {
    /// Where PinDiag is in the device's API (`diag.pins`), when it has more than one
    #[arg(long)]
    at: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

#[derive(Copy, Clone, Default, ValueEnum)]
pub(crate) enum Format {
    /// For people
    #[default]
    Text,
    /// For scripts
    Json,
}

#[derive(Args)]
pub(crate) struct TestArgs {
    /// Points: names, ids (P8), ranges (GPIO2..=GPIO7), globs on names and groups ('chain.*'), comma separated.
    /// Without any: the points the plan names (also with --watch), without a plan all points (with --watch alone:
    /// none, the watched points are only read)
    #[arg(value_delimiter = ',', add = ArgValueCandidates::new(complete::pin_points))]
    points: Vec<String>,

    /// Points only read, never set: their level, and whether they follow the tested points
    #[arg(long, value_delimiter = ',', add = ArgValueCandidates::new(complete::pin_points))]
    watch: Vec<String>,

    /// Plan file: expectations per point, narrower allow lists, which points are one net
    #[arg(long, value_hint = ValueHint::FilePath)]
    plan: Option<PathBuf>,

    /// Write the pull class each tested point shows into `expect` of this plan file (created if missing), so the
    /// next run with --plan is a pass/fail test
    #[arg(long, value_hint = ValueHint::FilePath)]
    save: Option<PathBuf>,

    /// Where PinDiag is in the device's API (`diag.pins`), when it has more than one; default: the plan's target
    #[arg(long)]
    at: Option<String>,
}

/// Runs a `ww pins` command; `Ok(false)` when a test found a fault.
pub(crate) async fn pins(cmd: PinsCommand, device: &DynClient) -> Result<bool> {
    match cmd {
        PinsCommand::List(args) => list(args, device).await.map(|()| true),
        PinsCommand::Test(args) => test(args, device).await,
        PinsCommand::Tui(args) => tui(args, device).await.map(|()| true),
    }
}

async fn tui(args: TuiArgs, device: &DynClient) -> Result<()> {
    use std::io::IsTerminal;
    // the preset first: a typo in it shouldn't wait for the device
    let board = args.board.as_deref().map(Board::load).transpose()?;
    if !std::io::stdout().is_terminal() {
        bail!("ww pins tui needs a terminal; 'ww pins list' and 'ww pins test' work in a pipe");
    }
    let stop = StopSignal::install();
    let interval = std::time::Duration::from_millis(args.interval_ms.max(10));
    let with_board = |title: String| match &board {
        Some(b) => format!("{title} · {}", escape(&b.name)),
        None => title,
    };
    let pin_diag = !args.gpio && (args.at.is_some() || !locations(device)?.is_empty());
    if pin_diag {
        let (mut pins, points) = open(device, args.at.as_deref()).await?;
        let title = with_board(format!("PinDiag at {}", location_name(&pins.at)));
        let mut app = pins_tui::App::new(points, board, title, interval);
        pins_tui::run(&mut app, &mut pins, stop).await
    } else {
        let (mut pins, points) = BankPins::find(device).await?;
        let title = with_board("GPIO banks (modes put back on release, no board rules)".into());
        let mut app = pins_tui::App::new(points, board, title, interval);
        pins_tui::run(&mut app, &mut pins, stop).await
    }
}

async fn open(device: &DynClient, at: Option<&str>) -> Result<(DevicePins, Vec<Point>)> {
    let mut pins = DevicePins::find(device, at)?;
    let raw = pins.raw_points().await?;
    let names = Names::for_device(device, &raw).await;
    if let Some(note) = &names.note {
        eprintln!("{}", style(note).dim());
    }
    let points = names.points(&raw);
    Ok((pins, points))
}

async fn list(args: ListArgs, device: &DynClient) -> Result<()> {
    let (pins, points) = open(device, args.at.as_deref()).await?;
    match args.format {
        Format::Text => {
            eprintln!(
                "{} {}, {} points",
                style("PinDiag at").dim(),
                style(location_name(&pins.at)).cyan(),
                points.len()
            );
            print!("{}", list_table(&points));
        }
        Format::Json => {
            let points: Vec<_> = points
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "group": p.group,
                        "caps": caps_list(p.caps),
                    })
                })
                .collect();
            let out = serde_json::json!({ "at": pins.at.name, "points": points });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
    }
    Ok(())
}

/// What a point can do, as plan files name it (`allow`), plus `weak_drive`.
fn caps_list(c: PointCaps) -> Vec<&'static str> {
    [
        (c.read, "read"),
        (c.pull_up, "pull_up"),
        (c.pull_down, "pull_down"),
        (c.drive_low, "drive_low"),
        (c.drive_high, "drive_high"),
        (c.weak_drive, "weak_drive"),
    ]
    .into_iter()
    .filter_map(|(has, name)| has.then_some(name))
    .collect()
}

fn list_table(points: &[Point]) -> String {
    let yes = |b: bool| {
        if b {
            style("✓".to_string()).green().to_string()
        } else {
            style("·".to_string()).dim().to_string()
        }
    };
    let rows = points
        .iter()
        .map(|p| {
            let c = p.caps;
            vec![
                format!("P{}", p.id),
                escape(&p.name).into_owned(),
                escape(&p.group).into_owned(),
                yes(c.read),
                yes(c.pull_up),
                yes(c.pull_down),
                yes(c.drive_low),
                yes(c.drive_high),
                yes(c.weak_drive),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "NAME",
            "GROUP",
            "READ",
            "PULL-UP",
            "PULL-DOWN",
            "LOW",
            "HIGH",
            "WEAK",
        ],
        rows,
    )
}

async fn test(args: TestArgs, device: &DynClient) -> Result<bool> {
    let plan = args.plan.as_deref().map(Plan::load).transpose()?;
    let at = args
        .at
        .as_deref()
        .or(plan.as_ref().and_then(|p| p.target.as_deref()));
    let (mut pins, points) = open(device, at).await?;
    let selected = select(&points, &args.points)?;
    let watch = select(&points, &args.watch)?;
    let setup = Setup::new(&points, &selected, &watch, plan.as_ref())?;
    if setup.targets.is_empty() {
        bail!("no point to test");
    }

    let bar = ProgressBar::new(0).with_style(
        ProgressStyle::with_template("{spinner} testing {pos}/{len} [{bar:30}] {elapsed}")
            .expect("valid template")
            .progress_chars("=> "),
    );
    let report = run(
        &mut pins,
        &setup,
        async { tokio::signal::ctrl_c().await.ok() },
        |done, total| {
            bar.set_length(total as u64);
            bar.set_position(done as u64);
        },
    )
    .await;
    bar.finish_and_clear();
    let report = match report {
        Ok(report) => report,
        Err(Stopped::Cancelled) => {
            eprintln!("{}", style("Cancelled, all points released").yellow());
            return Ok(false);
        }
        Err(e @ Stopped::Failed { .. }) => return Err(e).context("pin test stopped"),
    };

    print!("{}", test_table(&points, &report));
    print!("{}", summary(&points, &report));
    if let Some(path) = &args.save {
        let saved = plan::save(path, &pins.at.name, &points, &report)?;
        eprintln!(
            "{} {}: {} written, {} unchanged, {} without a pull class to save",
            style("Saved").dim(),
            style(path.display()).cyan(),
            saved.written.len(),
            saved.unchanged.len(),
            saved.skipped.len()
        );
        let unexpected = report.connections.iter().filter(|c| !c.expected).count();
        if unexpected > 0 {
            eprintln!(
                "{}",
                style(format!(
                    "{unexpected} connection(s) between points not saved: if they are one net on the board, \
                     add them to [shorts] same_net"
                ))
                .yellow()
            );
        }
    }
    Ok(report.passed())
}

fn name(points: &[Point], id: u16) -> String {
    points
        .iter()
        .find(|p| p.id == id)
        .map_or_else(|| format!("P{id}"), |p| escape(&p.name).into_owned())
}

fn not_checked() -> String {
    style("not checked".to_string()).dim().to_string()
}

fn refused() -> String {
    style("refused".to_string()).yellow().to_string()
}

fn rail(c: &Check<bool>) -> String {
    match c {
        Check::Done(false) => style("ok".to_string()).green().to_string(),
        Check::Done(true) => style("SHORT".to_string()).red().bold().to_string(),
        Check::NotChecked(_) => not_checked(),
        Check::Refused(_) => refused(),
    }
}

fn level(l: Level) -> &'static str {
    match l {
        Level::High => "high",
        Level::Low => "low",
    }
}

fn test_table(points: &[Point], report: &Report) -> String {
    let mut rows = vec![];
    for p in &report.points {
        let t = &p.target;
        let id = t.point.id;
        let pulls = match (&p.level, &p.pulls) {
            (Some(Check::Done(l)), _) => format!("watch: {}", level(*l)),
            (Some(_), _) => refused(),
            (None, Check::Done(class)) => match t.expect {
                Some(e) if e == *class => style(format!("{class} ✓")).green().to_string(),
                Some(e) => style(format!("{class} ✗ want {e}")).red().to_string(),
                None => class.to_string(),
            },
            (None, Check::NotChecked(_)) if t.expect.is_some() => {
                style("not checked ✗".to_string()).red().to_string()
            }
            (None, Check::Refused(_)) if t.expect.is_some() => {
                style("refused ✗".to_string()).red().to_string()
            }
            (None, Check::NotChecked(_)) => not_checked(),
            (None, Check::Refused(_)) => refused(),
        };
        let partners: Vec<String> = report
            .connections
            .iter()
            .filter_map(|c| {
                let other = if c.a == id {
                    c.b
                } else if c.b == id {
                    c.a
                } else {
                    return None;
                };
                let n = name(points, other);
                Some(if c.expected {
                    style(format!("{n} (net)")).green().to_string()
                } else {
                    style(n).red().bold().to_string()
                })
            })
            .collect();
        let between = match &p.between {
            Check::Done(()) if partners.is_empty() => style("ok".to_string()).green().to_string(),
            Check::Done(()) => partners.join(" "),
            Check::NotChecked(_) => not_checked(),
            Check::Refused(_) => refused(),
        };
        let watched = p.level.is_some();
        let (gnd, vdd) = if watched {
            (String::new(), String::new())
        } else {
            (rail(&p.gnd), rail(&p.vdd))
        };
        let mut why: Vec<&str> = vec![];
        if let Some(Check::Refused(r)) = &p.level {
            why.extend(r.split("; "));
        }
        if !watched {
            let reasons = [
                reason(&p.pulls),
                reason(&p.between),
                reason(&p.gnd),
                reason(&p.vdd),
            ];
            for r in reasons.into_iter().flatten() {
                why.extend(r.split("; "));
            }
        }
        why.extend(p.notes.iter().map(String::as_str));
        rows.push(vec![
            format!("P{id}"),
            escape(&t.point.name).into_owned(),
            escape(&t.point.group).into_owned(),
            pulls,
            gnd,
            vdd,
            between,
            style(escape(&notes(&why)).into_owned()).dim().to_string(),
        ]);
    }
    table(
        &[
            "ID",
            "NAME",
            "GROUP",
            "PULLS",
            "GND",
            "SUPPLY",
            "SHORTED TO",
            "NOTES",
        ],
        rows,
    )
}

/// Reasons and refusals (`<step>: <refusal>`) of a point in a few words: each once, refusals by what the device
/// said, with the steps when there are only a few (`in use by indication`, `not capable: pull-up, pull-down`).
fn notes(items: &[&str]) -> String {
    let mut by_why: Vec<(&str, Vec<&str>)> = vec![];
    for item in items {
        let (step, why) = match item.split_once(": ") {
            Some((step, why)) if STEPS.contains(&step) => (Some(step), why),
            _ => (None, *item),
        };
        let at = match by_why.iter().position(|(w, _)| *w == why) {
            Some(at) => at,
            None => {
                by_why.push((why, vec![]));
                by_why.len() - 1
            }
        };
        if let Some(step) = step
            && !by_why[at].1.contains(&step)
        {
            by_why[at].1.push(step);
        }
    }
    by_why
        .iter()
        .map(|(why, steps)| match steps.len() {
            1..=2 => format!("{why}: {}", steps.join(", ")),
            _ => why.to_string(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The steps `ww_pin_probe` names in its refusals.
const STEPS: [&str; 8] = [
    "pull-up",
    "pull-down",
    "input",
    "drive low",
    "drive high",
    "drive low (weak)",
    "drive high (weak)",
    "read",
];

/// Why a check wasn't done.
fn reason<T>(c: &Check<T>) -> Option<&str> {
    match c {
        Check::NotChecked(r) | Check::Refused(r) => Some(r),
        Check::Done(_) => None,
    }
}

fn summary(points: &[Point], report: &Report) -> String {
    let mut out = String::new();
    for &(a, b) in &report.missing {
        out += &format!(
            "{}\n",
            style(format!(
                "{} and {} are one net in the plan, but not connected",
                name(points, a),
                name(points, b)
            ))
            .red()
        );
    }
    let rails: usize = report
        .points
        .iter()
        .map(|p| (p.gnd == Check::Done(true)) as usize + (p.vdd == Check::Done(true)) as usize)
        .sum();
    let between = report.connections.iter().filter(|c| !c.expected).count();
    let mismatches = report.mismatches().len();
    let mut faults = vec![];
    if rails > 0 {
        faults.push(format!("{rails} short(s) to GND or the supply"));
    }
    if between > 0 {
        faults.push(format!("{between} short(s) between points"));
    }
    if mismatches > 0 {
        faults.push(format!("{mismatches} expectation(s) not met"));
    }
    if !report.missing.is_empty() {
        faults.push(format!("{} net(s) open", report.missing.len()));
    }
    let n = report.points.len();
    let line: StyledObject<String> = if faults.is_empty() {
        style(format!("{n} points: no shorts")).green().bold()
    } else {
        style(format!("{n} points: {}", faults.join(", ")))
            .red()
            .bold()
    };
    out += &format!("{line}\n");
    out
}

/// Columns padded to their widest cell (colour codes don't count), two spaces apart.
fn table(header: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| measure_text_width(h)).collect();
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(measure_text_width(cell));
        }
    }
    let line = |cells: Vec<String>| {
        let line = cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell}{}", " ".repeat(w - measure_text_width(cell))))
            .collect::<Vec<_>>()
            .join("  ");
        format!("{}\n", line.trim_end())
    };
    let mut out = line(
        header
            .iter()
            .map(|h| style(h.to_string()).dim().to_string())
            .collect(),
    );
    for row in rows {
        out += &line(row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_pin_probe::mock::{ALL, MockBoard, Net};

    #[tokio::test]
    async fn table_and_summary() {
        console::set_colors_enabled(false);
        let mut b = MockBoard::default();
        let g0 = b.point("GPIO0", "mcu", ALL, Net::Open);
        b.shorted_to("GPIO1", "mcu", ALL, g0);
        b.point("GPIO2", "mcu", ALL, Net::Gnd);
        let read_only = PointCaps {
            pull_up: false,
            pull_down: false,
            drive_low: false,
            drive_high: false,
            weak_drive: false,
            ..ALL
        };
        b.point("DET", "mcu", read_only, Net::PullUp);
        b.point("LED", "mcu", ALL, Net::Open);
        b.points[4].held_by = Some("indication".into());
        let points = b.host_points();
        let plan = Plan::parse("[[point]]\nname = \"GPIO0\"\nexpect = \"pull_up\"").unwrap();
        let setup = Setup::new(&points, &[0, 1, 2, 3, 4], &[], Some(&plan)).unwrap();
        let report = run(&mut b, &setup, std::future::pending::<()>(), |_, _| {})
            .await
            .unwrap();
        let text = test_table(&points, &report);
        assert_eq!(
            text,
            "\
ID  NAME   GROUP  PULLS                    GND          SUPPLY       SHORTED TO  NOTES
P0  GPIO0  mcu    floating ✗ want pull-up  ok           ok           GPIO1
P1  GPIO1  mcu    floating                 ok           ok           GPIO0
P2  GPIO2  mcu    held low                 SHORT        ok           ok
P3  DET    mcu    not checked              not checked  not checked  ok          no internal pulls; can't drive high; can't drive low
P4  LED    mcu    refused                  refused      refused      refused     in use by indication
"
        );
        let s = summary(&points, &report);
        assert_eq!(
            s,
            "5 points: 1 short(s) to GND or the supply, 1 short(s) between points, 1 expectation(s) not met\n"
        );
        let list = list_table(&points);
        assert!(
            list.lines()
                .nth(4)
                .unwrap()
                .starts_with("P3  DET    mcu    ✓     ·"),
            "{list}"
        );
    }
}
