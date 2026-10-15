//! `ww settings`: tools around `ww_settings::Store` (TOOL-12). `dump` decodes a flash dump: every page and item,
//! the current record of each key, cross-checked against a real `Store` over the same bytes
//! (`ww_settings::dump`, which does the actual decoding).

use crate::cmd::info::Format;
use crate::complete;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueHint};
use clap_complete::ArgValueCandidates;
use console::style;
use std::path::PathBuf;
use ww_report::info::hex;
use ww_settings::dump::{self, Dump, ItemState};

#[derive(Subcommand)]
pub(crate) enum SettingsCommand {
    /// Decode a ww_settings::Store flash dump: pages, every item (valid, cut or garbage) and the current record
    /// of each key, cross-checked against a real Store over the same bytes
    ///
    /// The word and page size aren't in the dump itself: pass the values the device's flash uses (the datasheet
    /// figures are in the ww_settings docs page).
    Dump(DumpArgs),
}

#[derive(Args)]
pub(crate) struct DumpArgs {
    /// Flash dump file: the raw bytes of the store's flash range, as read off the device
    #[arg(value_hint = ValueHint::FilePath)]
    file: PathBuf,

    /// Flash word size in bytes: the larger of the flash's WRITE_SIZE / READ_SIZE
    #[arg(long, add = ArgValueCandidates::new(complete::settings_word_sizes))]
    word_size: usize,

    /// Flash page (erase unit) size in bytes
    #[arg(long, add = ArgValueCandidates::new(complete::settings_page_sizes))]
    page_size: usize,

    /// Absolute address of the dump's first byte, as passed to Store::new on the device (hex with 0x, or decimal)
    #[arg(long, default_value = "0", value_parser = parse_addr)]
    base: u32,

    /// Output format
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => s.parse(),
    };
    r.map_err(|e| e.to_string())
}

pub(crate) fn settings(cmd: SettingsCommand) -> Result<()> {
    match cmd {
        SettingsCommand::Dump(args) => dump(args),
    }
}

fn dump(args: DumpArgs) -> Result<()> {
    let image = std::fs::read(&args.file)
        .with_context(|| format!("Failed to read {}", args.file.display()))?;
    let result = dump::analyze(&image, args.base, args.word_size, args.page_size)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("Failed to decode {}", args.file.display()))?;
    match args.format {
        Format::Text => print!("{}", render_text(&result)),
        Format::Json => println!("{}", render_json(&result)),
    }
    if !result.store_mismatch.is_empty() {
        bail!(
            "the raw walk disagrees with a real Store for key(s) {:?} \u{2014} this is a bug in \
             ww_settings::dump, please report it",
            result.store_mismatch
        );
    }
    Ok(())
}

fn render_text(d: &Dump<'_>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(
        out,
        "{} pages, word {} bytes, page {} bytes, base {:#010x}",
        d.pages.len(),
        d.word_size,
        d.page_size,
        d.base
    )
    .unwrap();
    for page in &d.pages {
        let header = if page.valid_header {
            format!("page_seq {}", page.page_seq.unwrap())
        } else {
            style("invalid header").red().to_string()
        };
        let current = if page.is_current {
            style(" (current)").green().to_string()
        } else {
            String::new()
        };
        writeln!(
            out,
            "page {} @ {:#010x}: {header}{current}",
            page.index, page.addr
        )
        .unwrap();
        for item in d.items.iter().filter(|i| i.page == page.index) {
            let state = match item.state {
                ItemState::Valid if item.is_current => style("valid, current").green(),
                ItemState::Valid => style("valid, stale").dim(),
                ItemState::Cut => style("cut").yellow(),
                ItemState::Garbage => style("garbage").red(),
                ItemState::Erased => style("erased").dim(),
            };
            match (item.key, item.seq, item.len) {
                (Some(key), Some(seq), Some(len)) => writeln!(
                    out,
                    "  {:#010x} +{:<6} {state} key {key} seq {seq} len {len}",
                    item.addr, item.size
                )
                .unwrap(),
                _ => writeln!(out, "  {:#010x} +{:<6} {state}", item.addr, item.size).unwrap(),
            }
        }
    }
    out.push('\n');
    writeln!(out, "current record per key:").unwrap();
    if d.current.is_empty() {
        writeln!(out, "  (none)").unwrap();
    }
    for rec in &d.current {
        writeln!(
            out,
            "  key {} @ {:#010x} (page {}), seq {}, {} bytes: {}",
            rec.key,
            rec.addr,
            rec.page,
            rec.seq,
            rec.bytes.len(),
            hex(rec.bytes)
        )
        .unwrap();
    }
    if !d.store_mismatch.is_empty() {
        writeln!(
            out,
            "{}",
            style(format!(
                "store mismatch on key(s) {:?} \u{2014} report this",
                d.store_mismatch
            ))
            .red()
        )
        .unwrap();
    }
    out
}

fn render_json(d: &Dump<'_>) -> String {
    let pages: Vec<_> = d
        .pages
        .iter()
        .map(|p| {
            serde_json::json!({
                "index": p.index,
                "addr": p.addr,
                "valid_header": p.valid_header,
                "page_seq": p.page_seq,
                "is_current": p.is_current,
            })
        })
        .collect();
    let items: Vec<_> = d
        .items
        .iter()
        .map(|i| {
            let state = match i.state {
                ItemState::Valid => "valid",
                ItemState::Cut => "cut",
                ItemState::Garbage => "garbage",
                ItemState::Erased => "erased",
            };
            serde_json::json!({
                "page": i.page,
                "addr": i.addr,
                "size": i.size,
                "state": state,
                "key": i.key,
                "seq": i.seq,
                "len": i.len,
                "is_current": i.is_current,
            })
        })
        .collect();
    let current: Vec<_> = d
        .current
        .iter()
        .map(|r| {
            serde_json::json!({
                "key": r.key,
                "addr": r.addr,
                "page": r.page,
                "seq": r.seq,
                "bytes_hex": hex(r.bytes),
            })
        })
        .collect();
    let value = serde_json::json!({
        "word_size": d.word_size,
        "page_size": d.page_size,
        "base": d.base,
        "pages": pages,
        "items": items,
        "current": current,
        "store_mismatch": d.store_mismatch,
    });
    serde_json::to_string_pretty(&value).unwrap()
}
