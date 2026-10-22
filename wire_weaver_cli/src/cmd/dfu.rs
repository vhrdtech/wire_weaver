//! `ww dfu`: firmware update steps through `ww_dfu` global traits, on any device whatever its API and version
//! (API-20, CLT-10), and whole updates of a chunky device (CLT-33, `chunky_ww_host` in chunky_mcu_updater).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use chunky_ww_host::chunky::chips::PRESETS;
use chunky_ww_host::chunky::geometry::FlashGeometry;
use chunky_ww_host::chunky_server::{self, Signer, keys};
use chunky_ww_host::{
    Dfu, Event, Node, Options, Reconnect, SealFor, UpdateError, check_image, reboot_into_trial,
    seal_for_node, wait_confirmed,
};
use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand, ValueEnum, ValueHint};
use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use wire_weaver_client::follow::{Follow, UsbBus};
use wire_weaver_client::hub::Exclusive;
use wire_weaver_client::ww_client_server::ErrorKindOwned;
use wire_weaver_client::{CallError, ClientConfig, ClientMeta, DynClient, Error};
use ww_dfu::{BootState, BootTarget, NodeInfoOwned, Outcome, Profile, Seal};

use crate::cmd::bootsel::BootselCommand;
use crate::cmd::stm32dfu::Stm32DfuCommand;
use crate::device::Selection;
use crate::untrusted::escape;

#[derive(Subcommand)]
pub(crate) enum DfuCommand {
    #[command(flatten)]
    Device(DeviceDfu),

    /// RP2040 / RP2350 boot ROM in BOOTSEL mode: list, flash over PICOBOOT, reboot
    ///
    /// --serial selects the device, as for other commands (the RP2350 boot ROM reports the chip id).
    #[command(subcommand)]
    Rp(BootselCommand),

    /// STM32 system bootloader in DFU mode (ROM DFU, DfuSe): list, show the memory layout, flash
    ///
    /// --serial selects the device, as for other commands (the bootloader reports a serial number made from the chip's
    /// unique id, see 'ww dfu stm32 list').
    #[command(subcommand)]
    Stm32(Stm32DfuCommand),
}

impl DfuCommand {
    /// The steps on a running device connect to it; the boot ROMs are found by their own USB ids.
    pub(crate) fn needs_device(&self) -> bool {
        matches!(self, DfuCommand::Device(_))
    }
}

/// Update steps on a running device, through `ww_dfu` global traits.
#[derive(Subcommand)]
pub(crate) enum DeviceDfu {
    /// Reboot the device into its updater (default) or its app, through `ww_dfu::BootControl`
    ///
    /// Works with any device implementing `ww_dfu::BootControl` anywhere in its API, whatever the API and its
    /// version: the call carries a global trait path the device resolves itself. An A/B device whose app takes
    /// updates itself (Profile::AB) has no updater of its own: it reboots into the chip's boot ROM (STM32 ROM DFU,
    /// RP2350 BOOTSEL), which `ww dfu stm32 flash` or `ww dfu rp flash` write.
    Reboot {
        /// What to reboot into
        #[arg(long, value_enum, default_value_t = Into::Updater)]
        into: Into,
    },

    /// Update a chunky device (`chunky_ww`): send what changed, reboot into the trial, wait for its confirm
    ///
    /// Reads what the device holds, plans a delta against it (chunks the device has are copied on the device, only
    /// the rest goes over the wire), sends the steps, has the device verify and stage the image, reboots it into a
    /// trial and waits until the new firmware confirms itself, or the device reverts to the old one. An ELF or a raw
    /// binary is sealed for the device first: its product, its seal scheme, the next version.
    ///
    /// Example: ww dfu flash target/thumbv7em-none-eabihf/release/b135_fw -s 0B135A01
    Flash(FlashArgs),

    /// Show what the device reports about updates: firmware, version, boot state, last update
    Status,

    /// Keep the trial image (what the firmware normally does itself after its self-test)
    Confirm,

    /// Give up the trial image (the old one runs after the next reset), or withdraw a staged one
    Revert,
}

#[derive(Args)]
pub(crate) struct FlashArgs {
    /// ELF file (detected by its magic bytes), an image sealed for the device (as long as ACTIVE, with its
    /// descriptor), or a raw binary payload
    #[arg(value_hint = ValueHint::FilePath)]
    file: PathBuf,

    /// Release version: a number or x.y.z (packed major << 16 | minor << 8 | patch); default the running one + 1
    #[arg(long, value_parser = parse_version)]
    fw_version: Option<u32>,

    /// Product id; default the device's (another one is refused)
    #[arg(long)]
    product: Option<u32>,

    /// Anti-rollback counter; default the device's version floor
    #[arg(long)]
    counter: Option<u32>,

    /// Release key (32-byte seed, `chunky keygen`) for a device that checks Ed25519 signatures
    #[arg(long, value_hint = ValueHint::FilePath, conflicts_with = "unsigned")]
    key: Option<PathBuf>,

    /// Seal without a signature: the default for a device built `unsigned`, refused by one that checks signatures
    #[arg(long)]
    unsigned: bool,

    /// Flash layout preset, when the device's geometry id is not one this ww knows
    #[arg(long, value_parser = chip_parser())]
    chip: Option<String>,

    /// Steps outstanding at most (default: as many as the device takes)
    #[arg(long)]
    in_flight: Option<u8>,

    /// Stop after staging: no reboot, no trial (`ww dfu reboot --into app` starts it)
    #[arg(long)]
    no_reboot: bool,
}

impl DeviceDfu {
    /// Commands that reset the device, the connection drops right after
    pub(crate) fn resets_device(&self) -> bool {
        matches!(self, DeviceDfu::Reboot { .. } | DeviceDfu::Flash(_))
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum Into {
    /// The updater (bootloader with a transport or recovery app), to receive an update there
    Updater,
    /// The app, the normal reset: switches to a staged image or applies a stored plan if there is one
    App,
}

fn parse_version(s: &str) -> Result<u32, String> {
    if let Ok(n) = s.parse() {
        return Ok(n);
    }
    let parts = s
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u32>().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    match parts[..] {
        [major, minor, patch] if major <= 0xFFFF && minor <= 0xFF && patch <= 0xFF => {
            Ok(major << 16 | minor << 8 | patch)
        }
        _ => Err("a number, or x.y.z with minor and patch up to 255".into()),
    }
}

fn chip_parser() -> impl TypedValueParser<Value = String> {
    PossibleValuesParser::new(PRESETS.iter().map(|(name, g)| {
        PossibleValue::new(*name).help(format!(
            "{} KB chunks, {} KB erase blocks, ACTIVE {} KB",
            g.chunk / 1024,
            g.erase / 1024,
            g.active.len / 1024
        ))
    }))
}

fn preset_name(id: u32) -> Option<&'static str> {
    PRESETS.iter().find(|(_, g)| g.id() == id).map(|(n, _)| *n)
}

/// `meta` is what `device` was connected with: a command that changes the device takes it whole while a ww hub serves
/// it to other programs too (SRV-32), and the claim is this client instance's.
pub(crate) async fn dfu(
    cmd: DeviceDfu,
    device: DynClient,
    selection: &Selection,
    meta: &ClientMeta,
) -> Result<()> {
    let purpose = match &cmd {
        DeviceDfu::Status => None,
        DeviceDfu::Reboot { .. } => Some("ww dfu reboot".to_string()),
        DeviceDfu::Flash(args) => Some(format!("ww dfu flash {}", args.file.display())),
        DeviceDfu::Confirm => Some("ww dfu confirm".to_string()),
        DeviceDfu::Revert => Some("ww dfu revert".to_string()),
    };
    let exclusive = match &purpose {
        Some(purpose) => take_exclusive(selection, meta, purpose).await?,
        None => None,
    };
    // held directly: this connection's own clock steering (CLT-46) leaves the device alone meanwhile, as the hub's
    // does under the exclusive claim
    if purpose.is_some()
        && exclusive.is_none()
        && let Some(steering) = device.cmd().clock_sync()
    {
        steering.set_paused(true);
    }
    let r = run(cmd, device, selection, meta).await;
    if let Some(exclusive) = exclusive
        && let Err(e) = exclusive.release().await
    {
        // the hub drops it by itself once this program exits
        eprintln!("{} {e:#}", style("note").yellow());
    }
    r
}

/// The whole device for this command when a ww hub of this user serves it (the hub refuses other programs' calls
/// and writes meanwhile and doesn't steer its clock). `None` when no hub holds it: this program then holds it directly.
async fn take_exclusive(
    selection: &Selection,
    meta: &ClientMeta,
    purpose: &str,
) -> Result<Option<Exclusive>> {
    if !wire_weaver_client::hub::hub_running() {
        return Ok(None);
    }
    let config = selection.client_config();
    let devices = wire_weaver_client::list_usb_devices()
        .await
        .unwrap_or_default();
    let mut keys = devices
        .iter()
        .filter(|d| config.matches(d))
        .filter_map(wire_weaver_client::hub::key_of);
    let (Some(key), None) = (keys.next(), keys.next()) else {
        // several match (connect took one) or none on USB: nothing to tell the hub
        return Ok(None);
    };
    let exclusive = Exclusive::take(&key, meta, purpose).await?;
    if exclusive.is_some() {
        eprintln!(
            "{} {key} to this command alone while it runs (other programs on the ww hub wait)",
            style("exclusive").bold()
        );
    }
    Ok(exclusive)
}

async fn run(
    cmd: DeviceDfu,
    device: DynClient,
    selection: &Selection,
    meta: &ClientMeta,
) -> Result<()> {
    match cmd {
        DeviceDfu::Reboot { into } => reboot(into, &device).await,
        DeviceDfu::Flash(args) => flash(args, device, selection, meta).await,
        DeviceDfu::Status => status(&device).await,
        DeviceDfu::Confirm => {
            let dfu = Dfu::new(&device);
            dfu.boot.confirm().call().await.map_err(call_error)?;
            let state = dfu.boot.boot_state().call().await.map_err(not_found)?;
            println!("{} boot state {}", style("✔").green(), state_style(state));
            Ok(())
        }
        DeviceDfu::Revert => {
            let dfu = Dfu::new(&device);
            dfu.boot.revert().call().await.map_err(call_error)?;
            let state = dfu.boot.boot_state().call().await.map_err(not_found)?;
            println!(
                "{} boot state {}: the previous image runs after the next reset (`ww dfu reboot --into app`)",
                style("✔").green(),
                state_style(state)
            );
            Ok(())
        }
    }
}

async fn reboot(into: Into, device: &DynClient) -> Result<()> {
    let mut dfu = Dfu::new(device);
    let (target, name) = match into {
        Into::Updater => (BootTarget::Updater, "updater"),
        Into::App => (BootTarget::App, "app"),
    };
    // an A/B app that takes updates itself: what it calls its updater is the chip's boot ROM
    let rom = target == BootTarget::Updater
        && dfu
            .node
            .node()
            .call()
            .await
            .is_ok_and(|n| matches!(n.profile, Profile::AB(_)) && n.offers.stage);
    dfu.boot.reboot(target).call().await.map_err(|e| match e {
        CallError::Device(e) => anyhow!("device refused to reboot: {e:?}"),
        CallError::Unimplemented { resource } => anyhow!("the device doesn't implement {resource}"),
        CallError::Client(e) => not_found(e),
    })?;
    let api = &device.device_api_info().user_api_version;
    if rom {
        println!(
            "{api:?} reboots into its boot ROM (an A/B device has no updater of its own): write it with `ww dfu stm32 flash` (STM32 ROM DFU) or `ww dfu rp flash` (RP2350); the app itself updates with `ww dfu flash`"
        );
    } else {
        println!("{api:?} reboots into the {name}");
    }
    Ok(())
}

async fn status(device: &DynClient) -> Result<()> {
    let dfu = Dfu::new(device);
    let n = dfu.node.node().call().await.map_err(not_found)?;
    print_node(&n);
    if n.offers.stage
        && let Ok(l) = dfu.stage.limits().call().await
    {
        println!(
            "{:12} pieces of {} B, {} in flight{}",
            style("stage").bold(),
            l.max_piece,
            l.in_flight,
            if l.writes_in_background {
                ""
            } else {
                ", stalls while it writes flash"
            }
        );
    }
    Ok(())
}

fn print_node(n: &NodeInfoOwned) {
    let layout = match preset_name(n.geometry_id) {
        Some(name) => format!("{name} ({:#010x})", n.geometry_id),
        None => format!("{:#010x}", n.geometry_id),
    };
    println!("{:12} {:?}, {:?}", style("node").bold(), n.role, n.profile);
    println!(
        "{:12} {} (id {})",
        style("product").bold(),
        escape(n.product.as_str()),
        n.product_id
    );
    println!("{:12} {layout}", style("layout").bold());
    println!(
        "{:12} {}, floor {}, seal {:?}",
        style("version").bold(),
        n.app_version.map_or("no valid image".into(), version),
        n.version_floor,
        n.seal
    );
    println!(
        "{:12} {}",
        style("boot state").bold(),
        state_style(n.boot_state)
    );
    let last = n.last_result;
    let outcome = format!("{:?}", last.outcome);
    let outcome = match last.outcome {
        Outcome::Confirmed => style(outcome).green(),
        Outcome::Reverted | Outcome::Rejected | Outcome::Recovery => style(outcome).red(),
        Outcome::Nothing => style(outcome).dim(),
    };
    match last.version {
        0 => println!("{:12} {outcome}", style("last update").bold()),
        v => println!(
            "{:12} {outcome} ({})",
            style("last update").bold(),
            version(v)
        ),
    }
}

/// A version, and how `--fw-version x.y.z` packs it.
fn version(v: u32) -> String {
    format!("{v} ({}.{}.{})", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

fn state_style(state: BootState) -> console::StyledObject<String> {
    let text = format!("{state:?}");
    match state {
        BootState::Confirmed => style(text).green(),
        BootState::Trial | BootState::Staged | BootState::PlanStored | BootState::Updating => {
            style(text).yellow()
        }
        BootState::Reverted => style(text).red(),
    }
}

/// ELF magic (`\x7fELF`), detected by content.
const ELF_MAGIC: &[u8] = &[0x7f, b'E', b'L', b'F'];

async fn flash(
    args: FlashArgs,
    device: DynClient,
    selection: &Selection,
    meta: &ClientMeta,
) -> Result<()> {
    let bytes =
        std::fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let geometry: Option<FlashGeometry> = args.chip.as_deref().map(|name| {
        *PRESETS
            .iter()
            .find(|(n, _)| *n == name)
            .expect("clap accepted only preset names")
            .1
    });
    let mut node = match Node::open(&device, geometry, None).await {
        Err(UpdateError::UnknownGeometry(id)) => bail!(
            "the device's flash layout {id:#010x} is not a chip preset this ww knows: pass --chip"
        ),
        node => node?,
    };
    print_node(&node.info);
    if !node.info.offers.stage {
        return Err(UpdateError::NoStage {
            profile: node.info.profile,
        }
        .into());
    }
    let sealed =
        !bytes.starts_with(ELF_MAGIC) && check_image(&bytes, &node.info, &node.geometry).is_ok();
    let image = if sealed {
        bytes
    } else {
        let payload = if bytes.starts_with(ELF_MAGIC) {
            chunky_server::payload_from_elf(&bytes, &node.geometry)?
        } else {
            bytes
        };
        let signer = match (&args.key, args.unsigned, node.info.seal) {
            (Some(key), _, _) => Signer::from_seed(&keys::read_seed(key)?),
            (None, true, _) | (None, false, Seal::Unsigned) => Signer::Unsigned,
            (None, false, Seal::Ed25519) => {
                bail!("the device checks Ed25519 signatures: give the release key with --key")
            }
        };
        if args.fw_version.is_none() {
            eprintln!(
                "{} no --fw-version: sealed as {}, the running version + 1",
                style("note").yellow(),
                version(node.info.app_version.unwrap_or(0).wrapping_add(1))
            );
        }
        let meta = SealFor {
            product_id: args.product,
            version: args.fw_version,
            security_counter: args.counter,
        };
        seal_for_node(&payload, &node.info, &node.geometry, meta, &signer)?
    };
    let info = check_image(&image, &node.info, &node.geometry)?;
    eprintln!(
        "{} version {}, {} KB for {}",
        style("image").bold(),
        version(info.version),
        image.len() / 1024,
        preset_name(node.info.geometry_id).unwrap_or("this layout")
    );

    let options = Options {
        geometry: Some(node.geometry),
        in_flight: args.in_flight,
        ..Options::default()
    };
    let started = Instant::now();
    let mut ui = Ui::default();
    let report = node.stage(&image, &options, &mut |e| ui.on(e)).await;
    ui.done();
    let report = report?;
    let s = report.stats;
    eprintln!(
        "{} staged in {:.1?}: {} KB over the wire for a {} KB image{}",
        style("✔").green(),
        started.elapsed(),
        s.wire_bytes.div_ceil(1024),
        s.image_bytes / 1024,
        match report.plans {
            1 => String::new(),
            n => format!(", planned {n} times"),
        }
    );
    if args.no_reboot {
        println!("staged: `ww dfu reboot --into app` starts the trial");
        return Ok(());
    }
    let mut reconnect = UsbReconnect::new(selection, meta).await;
    drop(device);
    let back = reboot_into_trial(node, &mut reconnect, &mut |e| ui.on(e)).await;
    let (node, device) = back.inspect_err(|_| ui.done())?;
    let done = if node.info.boot_state == BootState::Trial {
        wait_confirmed(node, device, &mut reconnect, &mut |e| ui.on(e)).await
    } else {
        Ok((node, device))
    };
    ui.done();
    let (node, _device) = done?;
    let n = &node.info;
    if n.boot_state == BootState::Confirmed && n.app_version == Some(info.version) {
        println!(
            "{} {} runs and confirmed itself, {:.1?} in all",
            style("✔").green(),
            version(info.version),
            started.elapsed()
        );
        return Ok(());
    }
    print_node(n);
    bail!(
        "the new image did not stay: the device runs {} ({:?}, last update {:?})",
        n.app_version.map_or("nothing".into(), version),
        n.boot_state,
        n.last_result.outcome
    )
}

/// The progress bar of the steps and a spinner for the phases around it, on stderr when it is a terminal.
#[derive(Default)]
struct Ui {
    bar: Option<ProgressBar>,
    spinner: Option<ProgressBar>,
}

impl Ui {
    fn spin(&mut self, msg: String) {
        self.done();
        let s = ProgressBar::new_spinner().with_message(msg);
        if console::Term::stderr().is_term() {
            s.enable_steady_tick(Duration::from_millis(100));
        } else {
            s.set_draw_target(ProgressDrawTarget::hidden());
        }
        self.spinner = Some(s);
    }

    fn done(&mut self) {
        for bar in [self.bar.take(), self.spinner.take()].into_iter().flatten() {
            bar.finish_and_clear();
        }
    }

    fn on(&mut self, event: Event<'_>) {
        match event {
            Event::Inventory { done, total } => {
                if self.spinner.is_none() {
                    self.spin(String::new());
                }
                if let Some(s) = &self.spinner {
                    s.set_message(format!(
                        "reading what the device holds: {done}/{total} chunks"
                    ));
                }
            }
            Event::Planned {
                stats,
                steps,
                attempt,
            } => {
                self.done();
                eprintln!(
                    "{} {steps} steps{}: {} chunks copied on the device, {} sent, {} kept, {} blocks erased",
                    style("plan").bold(),
                    if attempt > 1 { " (again)" } else { "" },
                    stats.copied,
                    stats.written,
                    stats.skipped,
                    stats.erased_blocks
                );
            }
            Event::Resumed { from } => eprintln!(
                "{} the device had this plan up to step {from}",
                style("resume").bold()
            ),
            Event::Sent { done, total } => {
                let bar = self.bar.get_or_insert_with(|| {
                    let bar = ProgressBar::new(total).with_message("send");
                    bar.set_style(
                        ProgressStyle::with_template(
                            "{msg:7} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                        )
                        .expect("a valid template")
                        .progress_chars("#>-"),
                    );
                    if !console::Term::stderr().is_term() {
                        bar.set_draw_target(ProgressDrawTarget::hidden());
                    }
                    bar
                });
                bar.set_length(total);
                bar.set_position(done);
            }
            Event::Replanning(e) => {
                self.done();
                eprintln!(
                    "{} the device answered {e:?}: reading it again",
                    style("replan").yellow()
                );
            }
            Event::Verifying => self.spin("the device checks the image".into()),
            Event::Staged => self.done(),
            Event::Rebooting => {
                self.spin("rebooting into the trial (the bootloader swaps the images)".into())
            }
            Event::Back(n) => {
                self.done();
                eprintln!(
                    "{} running {}, {}",
                    style("back").bold(),
                    n.app_version.map_or("nothing".into(), version),
                    state_style(n.boot_state)
                );
            }
            Event::WaitingForConfirm => {
                self.spin("waiting for the firmware to confirm itself".into())
            }
            Event::Node(_) | Event::KnownActive => {}
        }
    }
}

/// Finds the device again after it reset: gone from the bus and back under its serial number, then connected with
/// the same selection, as the same client instance (its exclusive claim in a hub holds across the reset).
struct UsbReconnect {
    config: ClientConfig,
    follow: Option<Follow>,
}

impl UsbReconnect {
    /// Before the reset: the full serial number of the device, given with --serial or of the one WireWeaver device
    /// the selection matches.
    async fn new(selection: &Selection, meta: &ClientMeta) -> Self {
        let given = selection
            .settings
            .iter()
            .find(|s| s.key == "serial")
            .map(|s| s.value.to_lowercase());
        let devices = wire_weaver_client::list_all_usb_devices()
            .await
            .unwrap_or_default();
        let matching: Vec<&String> = devices
            .iter()
            .filter(|d| d.api.is_some())
            .filter_map(|d| d.serials.first())
            .filter(|s| given.as_ref().is_none_or(|g| s.to_lowercase().contains(g)))
            .collect();
        let follow = match matching[..] {
            [serial] => Some(Follow::new(serial.clone())),
            _ => None,
        };
        Self {
            // the rest of the command runs on this connection: no clock steering on it (CLT-46)
            config: selection
                .client_config()
                .traits_only()
                .clock_sync(false)
                .client_meta(meta.clone()),
            follow,
        }
    }
}

impl Reconnect for UsbReconnect {
    async fn reconnect(&mut self, timeout: Duration) -> Result<DynClient, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        match &self.follow {
            Some(follow) => {
                // seen gone first: until then, what answers is the firmware about to reset
                _ = follow.gone(&mut UsbBus, Duration::from_secs(5)).await;
                follow
                    .back_in_wire_weaver(&mut UsbBus, timeout)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            None => tokio::time::sleep(Duration::from_secs(1)).await,
        }
        loop {
            match DynClient::from_config(self.config.clone()).connect().await {
                Ok(device) => return Ok(device),
                Err(e) if tokio::time::Instant::now() >= deadline => return Err(format!("{e:#}")),
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }
}

fn call_error(e: CallError<ww_dfu::Error>) -> anyhow::Error {
    match e {
        CallError::Device(e) => anyhow!("the device refused: {e:?}"),
        CallError::Unimplemented { resource } => anyhow!("the device doesn't implement {resource}"),
        CallError::Client(e) => not_found(e),
    }
}

fn not_found(e: Error) -> anyhow::Error {
    match &e {
        Error::RemoteError(r) if matches!(r.kind, ErrorKindOwned::TraitNotFound) => anyhow!(
            "the device doesn't implement this ww_dfu trait (or not in a compatible version)"
        ),
        _ => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_pack() {
        assert_eq!(parse_version("7"), Ok(7));
        assert_eq!(parse_version("0.2.1"), Ok(0x0201));
        assert_eq!(parse_version("v1.2.3"), Ok(0x01_0203));
        assert!(parse_version("1.256.0").is_err());
        assert!(parse_version("1.2").is_err());
        assert_eq!(version(0x01_0203), "66051 (1.2.3)");
    }
}
