//! `ww dfu stm32`: the STM32 system bootloader in DFU mode (ROM DFU, DfuSe), over `ww_stm32_dfu`, and following a device
//! into it and back (CLT-34).

use crate::cmd::rework_check::{ReworkCheckArgs, Rom, Titles, before_rom_flash, in_rom};
use crate::device::Selection;
use crate::untrusted::escape;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueHint};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wire_weaver_client::follow::{Follow, Found, Timeouts, UsbBus, open_retrying};
use wire_weaver_client::ww_stm32_dfu::layout::MemoryLayout;
use wire_weaver_client::ww_stm32_dfu::proto::ROM_ID_ADDR;
use wire_weaver_client::ww_stm32_dfu::usb::{self, NusbIo};
use wire_weaver_client::ww_stm32_dfu::{Dfuse, FlashOptions, Image, Stage};

use crate::cmd::rom::{DfuseRom, STM32_FLASH, STM32_SCAN};

#[derive(Subcommand)]
pub(crate) enum Stm32DfuCommand {
    /// List STM32 system bootloaders in DFU mode (0483:df11)
    List,

    /// Show the memory layout of the alternate settings of a device in DFU mode
    Info,

    /// Write an ELF file or a binary into flash, verify it and jump to flash
    ///
    /// The device is chosen with --serial (part of it is enough when only one matches), or the only one connected.
    /// Given the serial of a running WireWeaver device (part of it is enough), the board's reworks are checked
    /// against the image first, then the device is rebooted into the bootloader (ww_dfu BootControl::reboot(Updater))
    /// and waited for (--wait, 10 s by default); --follow waits for the firmware to come back. When the image lacks a
    /// rework applied on the board, it is written only if you agree (or --allow-unknown-reworks names those numbers):
    /// the allowance is stored for the image first. A device already in the bootloader can't be checked: give its
    /// full serial number with --wait to wait for it. Only the sectors the image touches are erased.
    ///
    /// Example: ww dfu stm32 flash target/thumbv7em-none-eabihf/release/bootloader --wait 10 --follow -s 3A0B1C2D4E5F
    Flash(FlashArgs),

    /// Read memory: a hexdump to the terminal, or raw bytes into a file with -o
    ///
    /// Example: ww dfu stm32 read 0x08060000 0x80 -s 3A0B
    Read(ReadArgs),

    /// Erase the flash sector containing ADDR, or sectors by index (see 'ww dfu stm32 info' for the layout)
    ///
    /// Sector indices count from the start of the first memory region; A..B is A up to but not including B, A..=B
    /// includes B. Example: ww dfu stm32 erase --sectors 3..=5 -s 3A0B
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

    /// Wait this many seconds for the bootloader with the --serial to appear (10 after rebooting a running device)
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

    #[command(flatten)]
    reworks: ReworkCheckArgs,
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
        Stm32DfuCommand::Flash(args) => flash(args, selection, serial).await,
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
            escape(d.product_string().unwrap_or_default()),
            escape(d.serial_number().unwrap_or_default()),
        );
    }
    Ok(())
}

async fn info(serial: Option<&str>) -> Result<()> {
    let device = usb::find(serial).await?;
    println!("serial {}", escape(device.serial_number().unwrap_or("?")));
    match Dfuse::new(NusbIo::open(&device, 0).await?) {
        Ok(mut dfu) => match dfu.rom_id().await {
            Some(v) => println!(
                "bootloader version {} (byte at {ROM_ID_ADDR:#010x}){}",
                version_name(v),
                if locks_pwr_cr3(v) {
                    ", locks PWR_CR3 on STM32H72x/73x (a power cycle after it)"
                } else {
                    ""
                }
            ),
            None => println!("bootloader version: not readable"),
        },
        Err(e) => println!("{e}"),
    }
    for (alt, name) in usb::alt_settings(&device).await? {
        println!("alt {alt}: {}", escape(&name));
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

/// Whether a ROM with version byte `id` locks PWR_CR3 with its own supply configuration until power-off: V9.1 and
/// V9.2 on STM32H72x/73x (AN2606 Table 134, fixed in V9.3). The id doesn't say the family; other families with
/// these versions get the note too, which says it is about H72x/73x.
fn locks_pwr_cr3(id: u8) -> bool {
    matches!(id, 0x91 | 0x92)
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

async fn flash(args: FlashArgs, selection: &Selection, serial: Option<String>) -> Result<()> {
    let image = load(&args)?;
    if serial.is_none() && (args.wait.is_some() || args.follow) {
        bail!("--wait and --follow need the device's full --serial");
    }
    let base = image.segments.first().map(|s| s.addr).unwrap_or_default();
    eprintln!(
        "{} {} bytes in {} segment(s) from {:#010x}",
        style("image").bold(),
        image.len(),
        image.segments.len(),
        base
    );
    // a running device of the serial is rebooted into the boot ROM; the board's reworks are checked there
    let contents: Vec<u8> = image
        .segments
        .iter()
        .flat_map(|s| s.data.iter().copied())
        .collect();
    let rebooted = before_rom_flash(Rom::Stm32, selection, serial.as_deref()).await?;
    let wait = match &rebooted {
        Some(_) => Some(args.wait.unwrap_or(10)),
        None => args.wait,
    };
    let titles = rebooted
        .as_ref()
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    let follow = match (&rebooted, &serial) {
        (Some((full, _)), _) => Some(Follow::new(full.clone())),
        (None, Some(s)) if wait.is_some() || args.follow => Some(Follow::new(s.clone())),
        _ => None,
    };
    let info = match (&follow, wait) {
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
            eprintln!("{} {}", style("found").bold(), escape(&found.to_string()));
            usb::find(found.serials.first().map(String::as_str)).await?
        }
        _ => usb::find(serial.as_deref()).await?,
    };
    let full_serial = info.serial_number().unwrap_or_default().to_string();
    let io = open_retrying(
        async || NusbIo::open(&info, 0).await,
        Timeouts::default().open,
    )
    .await?;
    let mut dfu = Dfuse::new(io)?;
    let leave_addr = args
        .leave_addr
        .unwrap_or_else(|| dfu.layout().regions[0].start);

    // the board's reworks against the image, the allowance stored or cleared: before anything of the image
    check_reworks(&mut dfu, &contents, &args.reworks, &titles).await?;
    // best effort, before anything is erased: the ROM's version byte (AN2606 Table 3, 0x93 is V9.3)
    let rom_id = dfu.rom_id().await;
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
        "{} {}: written{} in {:.1?}",
        style("✔").green(),
        escape(&full_serial),
        if args.no_verify { "" } else { " and verified" },
        started.elapsed()
    );
    if args.no_leave {
        return Ok(());
    }
    dfu.leave(leave_addr).await?;
    drop(dfu);
    eprintln!("{} jumped to {leave_addr:#010x}", style("✔").green());
    let locks_supply = rom_id.is_some_and(locks_pwr_cr3);
    if let Some(id) = rom_id.filter(|_| locks_supply) {
        eprintln!(
            "{} bootloader {}: on an STM32H72x/73x it locks PWR_CR3 until power-off (AN2606 Table 134), unplug and replug the board when you can (a firmware that does not handle it comes back only then)",
            style("note").yellow().bold(),
            version_name(id)
        );
    }
    if let Some(follow) = follow.filter(|_| args.follow) {
        let (msg, secs) = if locks_supply {
            ("waiting (replug it if it does not come) for", 120)
        } else {
            // about 10 s to blink the error code and reboot into the bootloader (STD-101), if it refuses to boot
            ("waiting for", 12)
        };
        let spinner = spinner(format!("{msg} {} to come back", follow.serial));
        // it may come back as WireWeaver, or, if it refuses to boot on an unknown rework (STD-101), blink the error
        // code and land back in the bootloader instead (TOOL-30)
        let found = follow
            .back(&mut UsbBus, Duration::from_secs(secs), "no device", |f| {
                matches!(f, Found::WireWeaver(_) | Found::Stm32Dfu(_))
            })
            .await;
        spinner.finish_and_clear();
        match found.map_err(|e| anyhow!(e))? {
            Found::WireWeaver(info) => {
                eprintln!("{} {}", style("back").bold(), escape(&info.to_string()))
            }
            Found::Stm32Dfu(_) => eprintln!(
                "{} it refused to boot (blink code 1: an applied rework this image doesn't know) and is back in \
                 the bootloader; `ww info -s {}` shows it, `ww board rework allow` fixes it",
                style("note").yellow().bold(),
                escape(&full_serial)
            ),
            _ => unreachable!("back() only returns what `want` accepted"),
        }
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

/// The rework check through the ROM (TOOL-29). A ROM that refuses to read (readout protection) can't be checked:
/// said, and the image is written only with `--yes`.
async fn check_reworks(
    dfu: &mut Dfuse<NusbIo>,
    contents: &[u8],
    args: &ReworkCheckArgs,
    titles: &Titles,
) -> Result<()> {
    let readable = dfu.layout().readable(STM32_FLASH, 1);
    if !readable {
        if !args.yes {
            bail!(
                "the STM32 bootloader doesn't read its flash (readout protection?): the board's reworks can't be checked, pass --yes to write anyway; nothing written"
            );
        }
        eprintln!(
            "{} the board's reworks are not checked: the bootloader doesn't read its flash",
            style("note").yellow().bold()
        );
        return Ok(());
    }
    in_rom(
        &mut DfuseRom(dfu),
        STM32_FLASH,
        STM32_SCAN,
        contents,
        args,
        titles,
    )
    .await
}
