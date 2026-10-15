//! `ww report`: [DeviceInfo] plus counters, the stored log and the last reset decoded with the firmware's ELF, in
//! one text a tester sends along with a bug report.

use crate::clients::Contract;
use crate::counters::{self, CountersReport};
use crate::info::{DeviceInfo, LastReset, Part, collect_info};
use crate::log::{self, LogReport};
use crate::symbols::{Located, Symbols};
use defmt_decoder::Table;
use serde::Serialize;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How to find the ELF and what to put in the report.
#[derive(Default, Clone)]
pub struct ReportOptions {
    /// The ELF to use for everything. By default each build id the device reports is looked up (`target/`,
    /// `WW_ELF_PATH`, the ELF directory `ww elf add` fills).
    pub elf: Option<PathBuf>,
    /// Use an ELF although its build id differs from the device's
    pub force: bool,
    /// `defmt` table to decode the log with, instead of the one in the ELF
    pub defmt_table: Option<Arc<Table>>,
    /// Tool name and version, printed in the header
    pub tool: String,
}

/// A part of the report that needed an ELF, and which one it used.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Elf {
    pub path: String,
    pub build_id: String,
}

/// Everything `ww report` collects.
#[derive(Debug, Serialize)]
pub struct Report {
    pub tool: String,
    pub info: DeviceInfo,
    /// ELF used for the last reset, with the crash's addresses named
    pub reset_symbols: Part<Vec<Located>>,
    pub counters: Part<CountersReport>,
    pub log: Part<LogReport>,
    /// Things worth knowing: an ELF that could not be checked, one that was not found
    pub notes: Vec<String>,
}

/// Collect the report. Each part that fails says why and the rest is still there.
pub async fn collect_report(contract: &Contract, options: &ReportOptions) -> Report {
    let info = collect_info(contract).await;
    let mut notes = vec![];
    let fw_id = info
        .firmware
        .value()
        .map(|f| f.build_id.clone())
        .unwrap_or_default();

    let reset_symbols = match info.last_reset.value() {
        Some(reset) => reset_symbols(reset, &fw_id, options, &mut notes),
        None => Part::NotImplemented,
    };

    let counters = match counters::read_info(&contract.counters).await {
        Part::Value(counters_info) => {
            let id = counters_info.build_id.as_slice().to_vec();
            match resolve(options, &id, "counters", &mut notes) {
                Ok(elf) => match counters::read(&contract.counters, counters_info, &elf).await {
                    Ok(c) => Part::Value(c),
                    Err(e) => Part::Error(format!("{e:#}")),
                },
                Err(e) => Part::Error(e),
            }
        }
        Part::NotImplemented => Part::NotImplemented,
        Part::Error(e) => Part::Error(e),
    };

    let log = log_part(contract, options, &mut notes).await;

    Report {
        tool: options.tool.clone(),
        info,
        reset_symbols,
        counters,
        log,
        notes,
    }
}

/// The ELF for build id `id` (an empty one: only an explicit `--elf` is used), pushing what is worth knowing to
/// `notes`.
fn resolve(
    options: &ReportOptions,
    id: &[u8],
    what: &str,
    notes: &mut Vec<String>,
) -> Result<PathBuf, String> {
    if id.is_empty() && options.elf.is_none() {
        return Err(format!(
            "the device reports no build id for its {what}, so its ELF can't be found: pass --elf"
        ));
    }
    match cnt_core::elf::resolve(options.elf.as_deref(), Some(id), options.force) {
        Ok(resolved) => {
            if let Some(w) = resolved.warning
                && !notes.contains(&w)
            {
                notes.push(w);
            }
            Ok(resolved.path)
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

fn reset_symbols(
    reset: &LastReset,
    fw_id: &str,
    options: &ReportOptions,
    notes: &mut Vec<String>,
) -> Part<Vec<Located>> {
    let addresses: Vec<u32> = reset
        .fault_addresses
        .iter()
        .chain(&reset.stack)
        .copied()
        .collect();
    if addresses.is_empty() {
        return Part::NotImplemented;
    }
    // the record names the firmware that crashed, which may be older than the one running now
    let id_hex = if reset.build_id.is_empty() {
        fw_id
    } else {
        &reset.build_id
    };
    let id: Vec<u8> = (0..id_hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&id_hex[2 * i..2 * i + 2], 16).ok())
        .collect();
    match resolve(options, &id, "crash record", notes) {
        Ok(path) => match Symbols::load(&path) {
            Ok(symbols) => Part::Value(addresses.iter().map(|a| symbols.locate(*a)).collect()),
            Err(e) => Part::Error(format!("{e:#}")),
        },
        Err(e) => Part::Error(e),
    }
}

async fn log_part(
    contract: &Contract,
    options: &ReportOptions,
    notes: &mut Vec<String>,
) -> Part<LogReport> {
    let info = match Part::from_result(contract.defmt.read_DEFMT().read().await) {
        Part::Value(i) => i,
        Part::NotImplemented => return Part::NotImplemented,
        Part::Error(e) => return Part::Error(e),
    };
    let table = match &options.defmt_table {
        Some(t) => t.clone(),
        None => {
            let path = match resolve(options, info.build_id.as_slice(), "log", notes) {
                Ok(p) => p,
                Err(e) => return Part::Error(e),
            };
            match load_table(&path) {
                Ok(t) => Arc::new(t),
                Err(e) => return Part::Error(e),
            }
        }
    };
    if !contract.defmt_absolute {
        return Part::Error(
            "the stored log is a stream, which is opened on the device's own API path: connect without \
             traits-only so its introspection data is read"
                .to_string(),
        );
    }
    match log::read(&contract.defmt, &table).await {
        Ok(l) => Part::Value(l),
        Err(e) => Part::Error(format!("{e:#}")),
    }
}

fn load_table(path: &Path) -> Result<Table, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match Table::parse(&bytes) {
        Ok(Some(t)) => Ok(t),
        Ok(None) => Err(format!("{} has no defmt table", path.display())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn part<T>(out: &mut String, part: &Part<T>, show: impl FnOnce(&mut String, &T)) {
    match part {
        Part::Value(v) => show(out, v),
        Part::NotImplemented => out.push_str("not implemented by this device\n"),
        Part::Error(e) => {
            let _ = writeln!(out, "unavailable: {e}");
        }
    }
}

/// `ww info` as text.
pub fn render_info(info: &DeviceInfo) -> String {
    let mut out = String::new();
    if let Some(api) = &info.api {
        let _ = writeln!(out, "API       {api}");
    }
    if let Part::Value(f) = &info.firmware {
        let _ = write!(out, "Firmware  {} {}", f.name, f.version);
        if let Some(git) = &f.git {
            let _ = write!(out, " ({git}");
            if let Some(built) = &f.built {
                let _ = write!(out, ", built {built}");
            }
            out.push(')');
        } else if let Some(built) = &f.built {
            let _ = write!(out, " (built {built})");
        }
        let _ = writeln!(out, ", {}", f.profile);
        let id = if f.build_id.is_empty() {
            "none (the firmware keeps no build id note)"
        } else {
            &f.build_id
        };
        let _ = writeln!(out, "Build id  {id}");
    } else {
        out.push_str("Firmware  ");
        part(&mut out, &info.firmware, |_, _| {});
    }
    out.push_str("Board     ");
    part(&mut out, &info.board, |o, b| {
        let _ = write!(o, "{} rev {} ({})", b.name, b.revision, b.revision_source);
        for (label, v) in [("variant", &b.variant), ("BOM", &b.bom_variant)] {
            if !v.is_empty() {
                let _ = write!(o, ", {label} {v}");
            }
        }
        if !b.serial.is_empty() {
            let _ = write!(o, ", serial {}", b.serial);
        }
        if let Some(d) = &b.build_date {
            let _ = write!(o, ", built {d}");
        }
        o.push('\n');
    });
    match &info.ids {
        Part::Value(ids) if ids.is_empty() => out.push_str("Ids       none\n"),
        Part::Value(ids) => {
            for (i, id) in ids.iter().enumerate() {
                let label = if i == 0 { "Ids      " } else { "         " };
                let primary = if i == 0 { ", primary" } else { "" };
                let _ = writeln!(
                    out,
                    "{label} {} {} ({}{primary})",
                    id.kind, id.value, id.source
                );
            }
        }
        other => {
            out.push_str("Ids       ");
            part(&mut out, other, |_, _| {});
        }
    }
    out.push_str("Label     ");
    part(&mut out, &info.label, |o, l| {
        let _ = write!(o, "{:?}", l.name);
        if let Part::Value(n) = &l.notes
            && !n.is_empty()
        {
            let _ = write!(o, ", notes {n:?}");
        }
        o.push_str(if l.persistent {
            " (kept over power cycles)\n"
        } else {
            " (lost at power off)\n"
        });
    });
    out.push_str("Last reset ");
    part(&mut out, &info.last_reset, |o, r| {
        let _ = write!(o, "{}", r.cause);
        if let Part::Value(n) = &r.boot_count {
            let _ = write!(o, ", boot {n} since cleared");
        }
        if let Some(up) = r.uptime_s {
            let _ = write!(o, ", ran {up:.1} s");
        }
        o.push('\n');
        if !r.message.is_empty() {
            let _ = writeln!(o, "           message: {}", r.message);
        }
        if let Some(f) = &r.fault {
            let _ = writeln!(o, "           {f}");
        }
    });
    out
}

/// The report as one text file.
pub fn render_report(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Device report ({})\n", report.tool);
    let _ = writeln!(
        out,
        "## Device\n\n```text\n{}```\n",
        render_info(&report.info)
    );
    for note in &report.notes {
        let _ = writeln!(out, "Note: {note}\n");
    }

    out.push_str("## Last reset\n\n");
    match &report.info.last_reset {
        Part::Value(r) => {
            let _ = writeln!(out, "Cause: {}", r.cause);
            if !r.build_id.is_empty() {
                let _ = writeln!(
                    out,
                    "Firmware that was running: build id {}",
                    hex_or(&r.build_id)
                );
            }
            if let Part::Value(n) = &r.boot_count {
                let _ = writeln!(out, "Boots since cleared: {n}");
            }
            if !r.message.is_empty() {
                let _ = writeln!(out, "Message: {}", r.message);
            }
            match &report.reset_symbols {
                Part::Value(located) => {
                    out.push_str("\n```text\n");
                    let n_fault = r.fault_addresses.len();
                    for (i, l) in located.iter().enumerate() {
                        let what = if i < n_fault { "fault" } else { "stack" };
                        match &l.function {
                            Some(f) => {
                                let _ = writeln!(
                                    out,
                                    "{what} {:#010x}  {f}+{:#x}",
                                    l.address, l.offset
                                );
                            }
                            None => {
                                let _ = writeln!(out, "{what} {:#010x}", l.address);
                            }
                        }
                    }
                    out.push_str("```\n");
                }
                Part::Error(e) => {
                    let _ = writeln!(out, "\nAddresses not named: {e}");
                    if !r.stack.is_empty() {
                        let _ = writeln!(out, "Stack: {}", words(&r.stack));
                    }
                }
                Part::NotImplemented => {}
            }
        }
        other => part(&mut out, other, |_, _| {}),
    }

    out.push_str("\n## Counters\n\n");
    part(&mut out, &report.counters, |o, c| {
        let _ = writeln!(o, "Decoded with {}\n\n```text", c.elf);
        let width = c.counters.iter().map(|c| c.name.len()).max().unwrap_or(0);
        for v in &c.counters {
            let unit = if v.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", v.unit)
            };
            let _ = writeln!(
                o,
                "{} {:7} {:width$} {}{unit}",
                v.buffer, v.severity, v.name, v.value
            );
        }
        o.push_str("```\n");
    });

    out.push_str("\n## Log\n\n");
    part(&mut out, &report.log, |o, l| {
        if l.lines.is_empty() {
            o.push_str("The device's log is empty.\n");
        } else {
            o.push_str("```text\n");
            for line in &l.lines {
                let _ = writeln!(o, "{line}");
            }
            o.push_str("```\n");
        }
    });
    out
}

fn hex_or(id_hex: &str) -> &str {
    if id_hex.is_empty() { "unknown" } else { id_hex }
}

fn words(w: &[u32]) -> String {
    w.iter()
        .map(|w| format!("{w:#010x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
