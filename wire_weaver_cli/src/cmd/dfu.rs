//! `ww dfu`: firmware update steps through `ww_dfu` global traits, on any device whatever its API and version
//! (API-20, CLT-10); whole updates of a chunky device (CLT-33) are the `ww-chunky` plugin's `flash`.

use std::ffi::OsString;

use anyhow::{Result, anyhow};
use clap::{Subcommand, ValueEnum};
use console::style;
use wire_weaver_client::ww_client_server::ErrorKindOwned;
use wire_weaver_client::{CallError, ClientMeta, DynClient, Error};
use ww_dfu::{BootState, BootTarget, NodeInfoOwned, Outcome, Profile};

use crate::cmd::bootsel::BootselCommand;
use crate::cmd::stm32dfu::Stm32DfuCommand;
use crate::device::Selection;
use crate::dfu_client::Dfu;
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
        matches!(self, DfuCommand::Device(d) if !matches!(d, DeviceDfu::Flash { .. }))
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
    /// Runs `ww-chunky flash` (the `ww-chunky` plugin, from the chunky_mcu_updater repository, found on PATH) with
    /// these arguments and the device selection; `ww dfu flash --help` shows its help. It reads what the device holds,
    /// plans a delta against it (chunks the device has are copied on the device, only the rest goes over the wire),
    /// sends the steps, has the device verify and stage the image, reboots it into a trial and waits until the new
    /// firmware confirms itself, or the device reverts to the old one.
    ///
    /// Example: ww dfu flash target/thumbv7em-none-eabihf/release/b135_fw -s 0B135A01
    #[command(disable_help_flag = true)]
    Flash {
        /// Arguments of `ww-chunky flash`
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },

    /// Show what the device reports about updates: firmware, version, boot state, last update
    Status,

    /// Keep the trial image (what the firmware normally does itself after its self-test)
    Confirm,

    /// Give up the trial image (the old one runs after the next reset), or withdraw a staged one
    Revert,
}

impl DeviceDfu {
    /// Commands that reset the device, the connection drops right after
    pub(crate) fn resets_device(&self) -> bool {
        matches!(self, DeviceDfu::Reboot { .. })
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum Into {
    /// The updater (bootloader with a transport or recovery app), to receive an update there
    Updater,
    /// The app, the normal reset: switches to a staged image or applies a stored plan if there is one
    App,
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
        DeviceDfu::Flash { .. } => unreachable!("flash runs the ww-chunky plugin"),
        DeviceDfu::Confirm => Some("ww dfu confirm".to_string()),
        DeviceDfu::Revert => Some("ww dfu revert".to_string()),
    };
    let exclusive = match &purpose {
        Some(purpose) => crate::plugin::take_exclusive(selection, meta, purpose).await?,
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
    let r = run(cmd, device).await;
    if let Some(exclusive) = exclusive
        && let Err(e) = exclusive.release().await
    {
        // the hub drops it by itself once this program exits
        eprintln!("{} {e:#}", style("note").yellow());
    }
    r
}

async fn run(cmd: DeviceDfu, device: DynClient) -> Result<()> {
    match cmd {
        DeviceDfu::Reboot { into } => reboot(into, &device).await,
        DeviceDfu::Flash { .. } => unreachable!("flash runs the ww-chunky plugin"),
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
    let layout = format!("{:#010x}", n.geometry_id);
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
    fn versions_format() {
        assert_eq!(version(0x01_0203), "66051 (1.2.3)");
    }
}
