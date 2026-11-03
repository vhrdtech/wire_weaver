//! `ww info` and `ww report`: what a device is and a bug report of it, through the global info traits (CLT-25).
//! `ww info` also works on a device already in its boot ROM ([info_rom], TOOL-30).

use anyhow::{Context, Result};
use clap::{Args, ValueEnum, ValueHint};
use serde::Serialize;
use std::path::PathBuf;
use wire_weaver_client::DynClient;
use ww_board_info::guard::Guard;
use ww_report::clients::Contract;
use ww_report::report::{ReportOptions, collect_report, render_info, render_report};
use ww_report::rework_check::{
    BoardReworks, ImageReworks, RomFlash, board_guard, guard_line, read_board, scan_installed_image,
};

use crate::cmd::rom::{self, DfuseRom, PicobootRom, RP_SCAN, STM32_FLASH, STM32_SCAN};

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
    /// Firmware ELF to decode counters, log and crash with: the global `--elf` given on the command line. By default
    /// the one with the build id the device reports: `target/` of the cargo project here, WW_ELF_PATH or
    /// ~/.cache/ww/elf (`ww elf add` fills it)
    #[arg(skip)]
    pub(crate) elf: Option<PathBuf>,

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

/// `ww info` on a device already in its boot ROM: the image in flash, the board's bench record and the verdict a
/// reboot would reach right now ([ww_report::rework_check], TOOL-30).
pub(crate) async fn info_rom(args: InfoArgs, target: &rom::RomTarget) -> Result<()> {
    let (image, board) = match target {
        rom::RomTarget::Rp { serial } => {
            let mut pb = rom::open_rp(serial).await?;
            pb.exclusive_access(true, false).await?;
            pb.exit_xip().await?;
            let r = read_rom(
                &mut PicobootRom(&mut pb),
                wire_weaver_client::ww_picoboot::proto::FLASH_BASE,
                RP_SCAN,
            )
            .await;
            pb.exclusive_access(false, false).await?;
            r?
        }
        rom::RomTarget::Stm32 { serial } => {
            let mut dfu = rom::open_stm32(serial).await?;
            if !dfu.layout().readable(STM32_FLASH, 1) {
                println!(
                    "the STM32 bootloader doesn't read its flash (readout protection?): the board's reworks can't be checked"
                );
                return Ok(());
            }
            read_rom(&mut DfuseRom(&mut dfu), STM32_FLASH, STM32_SCAN).await?
        }
    };
    match args.format {
        Format::Text => print!(
            "{}",
            ww_report::rework_check::rom_info_text(&image, board.as_ref())
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&RomInfoJson::new(&image, board.as_ref()))?
        ),
    }
    Ok(())
}

async fn read_rom(
    rom: &mut impl RomFlash,
    flash_base: u32,
    scan_len: u32,
) -> Result<(ImageReworks, Option<BoardReworks>)> {
    let image = scan_installed_image(rom, flash_base, scan_len).await?;
    let board = read_board(rom, &image, None, None).await?;
    Ok((image, board))
}

/// [info_rom]'s `--format json`.
#[derive(Debug, Serialize)]
struct RomInfoJson {
    guards: bool,
    build_id: Option<String>,
    boards: String,
    board: Option<RomBoardJson>,
}

#[derive(Debug, Serialize)]
struct RomBoardJson {
    name: String,
    revision: String,
    applied: Vec<u16>,
    allowance_image: Option<String>,
    allowance_reworks: Vec<u16>,
    runs: bool,
    unknown: Vec<u16>,
    verdict: String,
}

impl RomInfoJson {
    fn new(image: &ImageReworks, board: Option<&BoardReworks>) -> Self {
        RomInfoJson {
            guards: image.guards(),
            build_id: image.id.map(|id| id.to_string()),
            boards: image.boards(),
            board: board.map(|b| {
                let guard = board_guard(image, b);
                let unknown = guard.unknown().iter().map(|id| id.0).collect();
                RomBoardJson {
                    name: b.name.clone(),
                    revision: b.revision.clone(),
                    applied: b.applied.iter().map(|id| id.0).collect(),
                    allowance_image: b.allowance.image_id().map(|id| id.to_string()),
                    allowance_reworks: b.allowance.reworks.iter().map(|id| id.0).collect(),
                    runs: matches!(guard, Guard::Run | Guard::RunAllowed { .. }),
                    unknown,
                    verdict: guard_line(&guard),
                }
            }),
        }
    }
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
