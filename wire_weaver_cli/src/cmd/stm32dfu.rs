//! `ww stm32dfu`: the STM32 system bootloader in DFU mode (ROM DFU, DfuSe), over `ww_stm32_dfu`, and following a device
//! into it and back (CLT-34).

use crate::device::Selection;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueHint};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wire_weaver_client::follow::{Follow, UsbBus};
use wire_weaver_client::ww_stm32_dfu::layout::MemoryLayout;
use wire_weaver_client::ww_stm32_dfu::proto::ROM_ID_ADDR;
use wire_weaver_client::ww_stm32_dfu::usb::{self, NusbIo};
use wire_weaver_client::ww_stm32_dfu::{Dfuse, FlashOptions, Image, Stage};

#[derive(Subcommand)]
pub(crate) enum Stm32DfuCommand {
    /// List STM32 system bootloaders in DFU mode (0483:df11)
    List,

    /// Show the memory layout of the alternate settings of a device in DFU mode
    Info,

    /// Write an ELF file or a binary into flash, verify it and jump to flash
    ///
    /// The device is chosen with --serial (part of it is enough when only one matches), or the only one connected.
    /// To update a WireWeaver device that rebooted itself into the bootloader (ww_dfu BootControl::reboot(Updater)),
    /// give its full serial number with --wait to wait for the bootloader and --follow to wait for the firmware to
    /// come back. Only the sectors the image touches are erased.
    ///
    /// Example: ww stm32dfu flash target/thumbv7em-none-eabihf/release/bootloader --wait 10 --follow -s 3A0B1C2D4E5F
    Flash(FlashArgs),

    /// Read memory: a hexdump to the terminal, or raw bytes into a file with -o
    ///
    /// Example: ww stm32dfu read 0x08060000 0x80 -s 3A0B
    Read(ReadArgs),

    /// Erase the flash sector containing ADDR, or sectors by index (see 'ww stm32dfu info' for the layout)
    ///
    /// Sector indices count from the start of the first memory region; A..B is A up to but not including B, A..=B
    /// includes B. Example: ww stm32dfu erase --sectors 3..=5 -s 3A0B
    Erase(EraseArgs),
}

#[derive(Args)]
pub(crate) struct ReadArgs {
    /// Start address
    #[arg(value_parser = parse_addr)]
    addr: u32,

    /// Number of bytes
    #[arg(value_parser = parse_addr)]
    len: u32,

    /// Write the raw bytes into this file instead of printing a hexdump
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct EraseArgs {
    /// Erase the sector containing this address
    #[arg(value_parser = parse_addr, required_unless_present = "sectors", conflicts_with = "sectors")]
    addr: Option<u32>,

    /// Sector indices A..B or A..=B
    #[arg(long, value_name = "A..B")]
    sectors: Option<String>,
}

#[derive(Args)]
pub(crate) struct FlashArgs {
    /// ELF file (detected by its magic bytes, any extension, taken by the physical address of its load segments), or a
    /// raw binary (any other extension) written at --addr
    #[arg(value_hint = ValueHint::FilePath)]
    file: PathBuf,

    /// Address of a raw binary
    #[arg(long, default_value = "0x08000000", value_parser = parse_addr)]
    addr: u32,

    /// Jump to this address when leaving DFU (default: the start of flash, where the bootloader lives)
    #[arg(long, value_parser = parse_addr)]
    leave_addr: Option<u32>,

    /// Wait this many seconds for the bootloader with the --serial to appear
    #[arg(long, value_name = "SECONDS")]
    wait: Option<u64>,

    /// After leaving DFU, wait for a WireWeaver device with the same serial number and show it
    #[arg(long)]
    follow: bool,

    /// Stay in DFU mode after writing
    #[arg(long, conflicts_with = "follow")]
    no_leave: bool,

    /// Do not read the memory back to compare
    #[arg(long)]
    no_verify: bool,
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => s.parse(),
    };
    r.map_err(|e| e.to_string())
}

pub(crate) async fn stm32dfu(cmd: Stm32DfuCommand, selection: &Selection) -> Result<()> {
    let serial = selection
        .settings
        .iter()
        .find(|s| s.key == "serial")
        .map(|s| s.value.clone());
    match cmd {
        Stm32DfuCommand::List => list().await,
        Stm32DfuCommand::Info => info(serial.as_deref()).await,
        Stm32DfuCommand::Flash(args) => flash(args, serial).await,
        Stm32DfuCommand::Read(args) => read(args, serial.as_deref()).await,
        Stm32DfuCommand::Erase(args) => erase(args, serial.as_deref()).await,
    }
}

async fn list() -> Result<()> {
    let devices = usb::list().await?;
    if devices.is_empty() {
        eprintln!("No STM32 DFU devices found");
    }
    for d in devices {
        println!(
            "usb {}-{} {:04x}:{:04x} \"{}\" serial={}",
            d.bus_id(),
            d.port_chain()
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join("."),
            d.vendor_id(),
            d.product_id(),
            d.product_string().unwrap_or_default(),
            d.serial_number().unwrap_or_default(),
        );
    }
    Ok(())
}

async fn info(serial: Option<&str>) -> Result<()> {
    let device = usb::find(serial).await?;
    println!("serial {}", device.serial_number().unwrap_or("?"));
    match Dfuse::new(NusbIo::open(&device, 0).await?) {
        Ok(mut dfu) => {
            match dfu.bootloader_version().await {
                Ok(v) => println!("bootloader version {}", version_name(v)),
                Err(e) => println!("bootloader version: {e}"),
            }
            if let Some(id) = dfu.rom_id().await {
                println!("bootloader id {id:#04x} (byte at {ROM_ID_ADDR:#010x})");
            }
        }
        Err(e) => println!("{e}"),
    }
    for (alt, name) in usb::alt_settings(&device).await? {
        println!("alt {alt}: {name}");
        match MemoryLayout::parse(&name) {
            Ok(layout) => {
                for r in layout.regions {
                    println!(
                        "    {:#010x}..{:#010x}  {} x {} KiB  {}{}{}",
                        r.start,
                        r.end(),
                        r.sectors,
                        r.sector_size / 1024,
                        if r.readable { "r" } else { "-" },
                        if r.erasable { "e" } else { "-" },
                        if r.writable { "w" } else { "-" },
                    );
                }
            }
            Err(e) => println!("    {e}"),
        }
    }
    Ok(())
}

async fn read(args: ReadArgs, serial: Option<&str>) -> Result<()> {
    let mut dfu = usb::open(serial).await?;
    let bar = progress_bar("read")?;
    bar.set_length(args.len as u64);
    let data = dfu
        .read(args.addr, args.len as usize, &mut |n| bar.inc(n as u64))
        .await?;
    bar.finish_and_clear();
    match args.output {
        Some(path) => {
            std::fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;
            eprintln!(
                "{} {} bytes into {}",
                style("✔").green(),
                data.len(),
                path.display()
            );
        }
        None => {
            for (i, line) in data.chunks(16).enumerate() {
                let hex: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
                let text: String = line
                    .iter()
                    .map(|b| {
                        if b.is_ascii_graphic() || *b == b' ' {
                            *b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();
                println!(
                    "{:08x}  {:<47}  |{text}|",
                    args.addr as usize + i * 16,
                    hex.join(" ")
                );
            }
        }
    }
    Ok(())
}

/// `A..B` or `A..=B` as a range of sector indices.
fn parse_sectors(s: &str) -> Result<std::ops::Range<usize>> {
    let (a, b, inclusive) = match s.split_once("..=") {
        Some((a, b)) => (a, b, true),
        None => {
            let (a, b) = s
                .split_once("..")
                .ok_or_else(|| anyhow!("expected A..B or A..=B"))?;
            (a, b, false)
        }
    };
    let a: usize = a.trim().parse().context("first sector index")?;
    let b: usize = b.trim().parse().context("last sector index")?;
    Ok(a..b + usize::from(inclusive))
}

async fn erase(args: EraseArgs, serial: Option<&str>) -> Result<()> {
    let mut dfu = usb::open(serial).await?;
    let sectors: Vec<u32> = match (args.addr, &args.sectors) {
        (Some(addr), _) => {
            let (base, _, region) = dfu
                .layout()
                .sector_at(addr)
                .ok_or_else(|| anyhow!("{addr:#010x} is not in the memory layout"))?;
            if !region.erasable {
                bail!("{addr:#010x} is not erasable memory");
            }
            vec![base]
        }
        (None, Some(spec)) => {
            let range = parse_sectors(spec)?;
            let all: Vec<u32> = dfu
                .layout()
                .regions
                .iter()
                .filter(|r| r.erasable)
                .flat_map(|r| (0..r.sectors).map(move |i| r.start + i * r.sector_size))
                .collect();
            if range.is_empty() || range.end > all.len() {
                bail!(
                    "sectors {spec}: the layout has {} erasable sectors",
                    all.len()
                );
            }
            all[range].to_vec()
        }
        (None, None) => unreachable!("clap requires one"),
    };
    let bar = progress_bar("erase")?;
    bar.set_style(
        ProgressStyle::with_template("{msg:7} [{bar:40.cyan/blue}] {pos}/{len} sectors")?
            .progress_chars("#>-"),
    );
    bar.set_length(sectors.len() as u64);
    for addr in &sectors {
        dfu.erase_sector(*addr).await?;
        bar.inc(1);
    }
    bar.finish_and_clear();
    eprintln!(
        "{} erased {} sector(s) from {:#010x}",
        style("✔").green(),
        sectors.len(),
        sectors[0]
    );
    Ok(())
}

fn progress_bar(msg: &'static str) -> Result<ProgressBar> {
    let bar = ProgressBar::new(0).with_message(msg);
    bar.set_style(
        ProgressStyle::with_template(
            "{msg:7} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )?
        .progress_chars("#>-"),
    );
    if !console::Term::stderr().is_term() {
        bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    }
    Ok(bar)
}

/// `0x93` as `V9.3`.
fn version_name(v: u8) -> String {
    format!("{v:#04x} (V{}.{})", v >> 4, v & 0xF)
}

/// ELF magic (`\x7fELF`): detected by content, not extension.
const ELF_MAGIC: &[u8] = &[0x7f, b'E', b'L', b'F'];

fn load(args: &FlashArgs) -> Result<Image> {
    let bytes =
        std::fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    Ok(if bytes.starts_with(ELF_MAGIC) {
        Image::from_elf(&bytes)?
    } else {
        Image::from_bin(args.addr, bytes)?
    })
}

async fn flash(args: FlashArgs, serial: Option<String>) -> Result<()> {
    let image = load(&args)?;
    let follow = match (&serial, args.wait.is_some() || args.follow) {
        (Some(s), true) => Some(Follow::new(s.clone())),
        (None, true) => bail!("--wait and --follow need the device's full --serial"),
        _ => None,
    };
    let base = image.segments.first().map(|s| s.addr).unwrap_or_default();
    eprintln!(
        "{} {} bytes in {} segment(s) from {:#010x}",
        style("image").bold(),
        image.len(),
        image.segments.len(),
        base
    );
    let info = match (&follow, args.wait) {
        (Some(follow), Some(secs)) => {
            let spinner = spinner(format!(
                "waiting for {} in the STM32 bootloader",
                follow.serial
            ));
            let found = follow
                .back_in_stm32_dfu(&mut UsbBus, Duration::from_secs(secs))
                .await;
            spinner.finish_and_clear();
            let found = found?;
            eprintln!("{} {found}", style("found").bold());
            usb::find(found.serials.first().map(String::as_str)).await?
        }
        _ => usb::find(serial.as_deref()).await?,
    };
    let full_serial = info.serial_number().unwrap_or_default().to_string();
    let mut dfu = Dfuse::new(NusbIo::open(&info, 0).await?)?;
    let leave_addr = args
        .leave_addr
        .unwrap_or_else(|| dfu.layout().regions[0].start);

    // best effort, before anything is erased
    let version = dfu.bootloader_version().await.ok();
    let started = Instant::now();
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template(
            "{msg:7} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
        )?
        .progress_chars("#>-"),
    );
    if !console::Term::stderr().is_term() {
        bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    }
    let options = FlashOptions {
        no_verify: args.no_verify,
        ..FlashOptions::default()
    };
    dfu.flash(
        &image,
        &options,
        &mut |stage: Stage, done: u64, total: u64| {
            let msg = match stage {
                Stage::Erase => "erase",
                Stage::Write => "write",
                Stage::Verify => "verify",
            };
            if bar.message() != msg {
                bar.set_message(msg);
                bar.reset();
            }
            bar.set_length(total);
            bar.set_position(done);
        },
    )
    .await?;
    bar.finish_and_clear();
    eprintln!(
        "{} {full_serial}: written{} in {:.1?}",
        style("✔").green(),
        if args.no_verify { "" } else { " and verified" },
        started.elapsed()
    );
    if args.no_leave {
        return Ok(());
    }
    dfu.leave(leave_addr).await?;
    drop(dfu);
    eprintln!("{} jumped to {leave_addr:#010x}", style("✔").green());
    if let Some(v) = version.filter(|v| v >> 4 == 9) {
        eprintln!(
            "{} bootloader {} locks PWR_CR3: power-cycle the board before the firmware can set up PWR",
            style("note").yellow().bold(),
            version_name(v)
        );
    }
    if let Some(follow) = follow.filter(|_| args.follow) {
        let spinner = spinner(format!("waiting for {} to come back", follow.serial));
        let back = follow
            .back_in_wire_weaver(&mut UsbBus, Duration::from_secs(10))
            .await;
        spinner.finish_and_clear();
        eprintln!("{} {}", style("back").bold(), back.map_err(|e| anyhow!(e))?);
    }
    Ok(())
}

fn spinner(msg: String) -> ProgressBar {
    let s = ProgressBar::new_spinner().with_message(msg);
    if console::Term::stderr().is_term() {
        s.enable_steady_tick(Duration::from_millis(100));
    } else {
        s.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    }
    s
}
