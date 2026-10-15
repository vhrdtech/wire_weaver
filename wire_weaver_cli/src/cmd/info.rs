//! `ww info` and `ww report`: what a device is and a bug report of it, through the global info traits (CLT-25).

use anyhow::{Context, Result};
use clap::{Args, ValueEnum, ValueHint};
use std::path::PathBuf;
use wire_weaver_client::DynClient;
use ww_report::clients::Contract;
use ww_report::report::{ReportOptions, collect_report, render_info, render_report};

#[derive(Copy, Clone, Default, ValueEnum)]
pub(crate) enum Format {
    /// For people
    #[default]
    Text,
    /// For scripts
    Json,
}

#[derive(Args)]
pub(crate) struct InfoArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

#[derive(Args)]
pub(crate) struct ReportArgs {
    /// Firmware ELF to decode counters, log and crash with. By default the one with the build id the device
    /// reports: `target/` of the cargo project here, WW_ELF_PATH or ~/.cache/ww/elf (`ww elf add` fills it)
    #[arg(long, value_hint = ValueHint::FilePath)]
    elf: Option<PathBuf>,

    /// Use the ELF although the device runs other firmware (its build id differs)
    #[arg(long)]
    force: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t)]
    format: Format,

    /// Write the report to this file instead of stdout
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
}

pub(crate) async fn info(args: InfoArgs, device: &DynClient) -> Result<()> {
    let info = ww_report::info::collect_info(&Contract::new(device)).await;
    match args.format {
        Format::Text => print!("{}", render_info(&info)),
        Format::Json => println!("{}", serde_json::to_string_pretty(&info)?),
    }
    Ok(())
}

pub(crate) async fn report(args: ReportArgs, device: &DynClient) -> Result<()> {
    let options = ReportOptions {
        elf: args.elf,
        force: args.force,
        defmt_table: None,
        tool: format!("ww {}", env!("WW_VERSION")),
    };
    let report = collect_report(&Contract::new(device), &options).await;
    let text = match args.format {
        Format::Text => render_report(&report),
        Format::Json => serde_json::to_string_pretty(&report)? + "\n",
    };
    match args.output {
        Some(path) => {
            std::fs::write(&path, text)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            eprintln!("Report written to {}", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}
