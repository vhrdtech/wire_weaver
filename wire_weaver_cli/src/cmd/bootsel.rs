//! `ww bootsel`: RP2040 / RP2350 boot ROMs in BOOTSEL mode, over PICOBOOT (`ww_picoboot`), and following a device
//! into BOOTSEL and back (CLT-21).

use crate::device::Selection;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum, ValueHint};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wire_weaver_client::follow::{Follow, UsbBus};
use wire_weaver_client::ww_picoboot::image::family;
use wire_weaver_client::ww_picoboot::proto::{FLASH_BASE, REBOOT_DELAY, Reboot2};
use wire_weaver_client::ww_picoboot::usb::{self, NusbIo};
use wire_weaver_client::ww_picoboot::{Image, Picoboot, Stage};

#[derive(Subcommand)]
pub(crate) enum BootselCommand {
    /// List RP2040 / RP2350 boot ROMs in BOOTSEL mode
    List,

    /// Show the chip id of a BOOTSEL device (RP2350)
    Info,

    /// Write a UF2 file, an ELF file or a binary into flash, verify it and reboot
    ///
    /// The device is chosen with --serial (part of it is enough when only one matches), or the only one connected.
    /// To update a WireWeaver device that rebooted itself into BOOTSEL (ww_dfu BootControl::reboot(Updater)), give
    /// its full serial number with --wait to wait for the boot ROM and --follow to wait for the firmware to come back.
    ///
    /// Example: ww bootsel flash target/thumbv8m.main-none-eabihf/release/app --wait 10 --follow -s E661640843522C2F
    Flash(FlashArgs),

    /// Reboot a BOOTSEL device (RP2350)
    Reboot {
        /// Into BOOTSEL again instead of the image in flash
        #[arg(long)]
        bootsel: bool,
    },
}

#[derive(Args)]
pub(crate) struct FlashArgs {
    /// UF2 file, ELF file (detected by its magic bytes, any extension), or a raw binary (any other extension)
    /// written at --addr
    #[arg(value_hint = ValueHint::FilePath)]
    file: PathBuf,

    /// Flash address of a raw binary
    #[arg(long, default_value = "0x10000000", value_parser = parse_addr)]
    addr: u32,

    /// UF2 family to take blocks of
    #[arg(long, value_enum, default_value_t = Family::Rp2350)]
    family: Family,

    /// Wait this many seconds for the BOOTSEL device with the --serial to appear
    #[arg(long, value_name = "SECONDS")]
    wait: Option<u64>,

    /// After the reboot, wait for a WireWeaver device with the same serial number and show it
    #[arg(long)]
    follow: bool,

    /// Do not reboot after writing
    #[arg(long, conflicts_with = "follow")]
    no_reboot: bool,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum Family {
    /// RP2350, ARM secure, ARM non-secure or RISC-V
    Rp2350,
    Rp2040,
    /// Every block meant for flash
    Any,
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => s.parse(),
    };
    r.map_err(|e| e.to_string())
}

pub(crate) async fn bootsel(cmd: BootselCommand, selection: &Selection) -> Result<()> {
    let serial = selection
        .settings
        .iter()
        .find(|s| s.key == "serial")
        .map(|s| s.value.clone());
    match cmd {
        BootselCommand::List => list().await,
        BootselCommand::Info => {
            let mut pb = open(serial.as_deref()).await?;
            let id = pb.chip_id().await?;
            println!("chip id: {id:016X}");
            Ok(())
        }
        BootselCommand::Flash(args) => flash(args, serial).await,
        BootselCommand::Reboot { bootsel } => {
            let mut pb = open(serial.as_deref()).await?;
            let to = if bootsel {
                Reboot2::Bootsel
            } else {
                Reboot2::Normal
            };
            pb.reboot2(to, REBOOT_DELAY).await?;
            Ok(())
        }
    }
}

async fn list() -> Result<()> {
    let devices = usb::list().await?;
    if devices.is_empty() {
        eprintln!("No BOOTSEL devices found");
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

/// The BOOTSEL device whose serial number contains `serial`, or the only one.
async fn find(serial: Option<&str>) -> Result<nusb::DeviceInfo> {
    let devices = usb::list().await?;
    let matching: Vec<_> = devices
        .into_iter()
        .filter(|d| {
            serial.is_none_or(|s| {
                d.serial_number()
                    .is_some_and(|ds| ds.to_lowercase().contains(&s.to_lowercase()))
            })
        })
        .collect();
    match matching.len() {
        1 => Ok(matching.into_iter().next().unwrap()),
        0 => Err(anyhow!(
            "no BOOTSEL device{} found, run 'ww bootsel list'",
            serial
                .map(|s| format!(" with serial {s}"))
                .unwrap_or_default()
        )),
        n => Err(anyhow!(
            "{n} BOOTSEL devices match, choose one with --serial: {}",
            matching
                .iter()
                .map(|d| d.serial_number().unwrap_or("?"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

async fn open(serial: Option<&str>) -> Result<Picoboot<NusbIo>> {
    let info = find(serial).await?;
    Ok(Picoboot::new(NusbIo::open(&info).await?).await?)
}

/// ELF magic (`\x7fELF`): detected by content, not extension — unlike pico-sdk's `.elf`, other toolchains name
/// their ELF output `.axf` or leave it without an extension.
const ELF_MAGIC: &[u8] = &[0x7f, b'E', b'L', b'F'];

fn load(args: &FlashArgs) -> Result<Image> {
    let bytes =
        std::fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let is_uf2 = args
        .file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("uf2"));
    let image = if bytes.starts_with(ELF_MAGIC) {
        Image::from_elf(&bytes)?
    } else if is_uf2 {
        let families: &[u32] = match args.family {
            Family::Rp2350 => &[
                family::RP2350_ARM_S,
                family::RP2350_ARM_NS,
                family::RP2350_RISCV,
                family::ABSOLUTE,
            ],
            Family::Rp2040 => &[family::RP2040],
            Family::Any => &[],
        };
        Image::from_uf2(&bytes, families)?
    } else {
        Image::from_bin(args.addr, bytes)?
    };
    Ok(image)
}

async fn flash(args: FlashArgs, serial: Option<String>) -> Result<()> {
    let image = load(&args)?;
    let follow = match (&serial, args.wait.is_some() || args.follow) {
        (Some(s), true) => Some(Follow::new(s.clone())),
        (None, true) => bail!("--wait and --follow need the device's full --serial"),
        _ => None,
    };
    let base = image.segments.first().map(|s| s.addr).unwrap_or(FLASH_BASE);
    eprintln!(
        "{} {} bytes in {} segment(s) from {:#010x}",
        style("image").bold(),
        image.len(),
        image.segments.len(),
        base
    );
    let info = match (&follow, args.wait) {
        (Some(follow), Some(secs)) => {
            let spinner = spinner(format!("waiting for {} in BOOTSEL", follow.serial));
            let found = follow
                .back_in_bootsel(&mut UsbBus, Duration::from_secs(secs))
                .await;
            spinner.finish_and_clear();
            let found = found?;
            eprintln!("{} {found}", style("found").bold());
            find(found.serials.first().map(String::as_str)).await?
        }
        _ => find(serial.as_deref()).await?,
    };
    let full_serial = info.serial_number().unwrap_or_default().to_string();
    let mut pb = Picoboot::new(NusbIo::open(&info).await?).await?;

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
    pb.flash(&image, &mut |stage: Stage, done: usize, total: usize| {
        let msg = match stage {
            Stage::Erase => "erase",
            Stage::Write => "write",
            Stage::Verify => "verify",
        };
        if bar.message() != msg {
            bar.set_message(msg);
            bar.reset();
        }
        bar.set_length(total as u64);
        bar.set_position(done as u64);
    })
    .await?;
    bar.finish_and_clear();
    eprintln!(
        "{} {full_serial}: written and verified in {:.1?}",
        style("✔").green(),
        started.elapsed()
    );
    if args.no_reboot {
        return Ok(());
    }
    // as `picotool load -x`: boot the image just written
    pb.reboot2(Reboot2::FlashUpdate { base }, REBOOT_DELAY)
        .await?;
    drop(pb);
    if let Some(follow) = follow.filter(|_| args.follow) {
        let spinner = spinner(format!("waiting for {} to come back", follow.serial));
        let back = follow
            .back_in_wire_weaver(&mut UsbBus, Duration::from_secs(10))
            .await;
        spinner.finish_and_clear();
        eprintln!("{} {}", style("back").bold(), back?);
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
