//! `ww dfu`: firmware update steps through `ww_dfu` global traits, on any device whatever its API and version
//! (API-20, CLT-10).

use anyhow::{Result, anyhow};
use clap::{Subcommand, ValueEnum};
use wire_weaver_client::ww_client_server::ErrorKindOwned;
use wire_weaver_client::{Commander, DynClient, Error};
use ww_dfu::BootTarget;

#[derive(Subcommand)]
pub(crate) enum DfuCommand {
    /// Reboot the device into its updater (default) or its app, through `ww_dfu::BootControl`
    ///
    /// Works with any device implementing `ww_dfu::BootControl` anywhere in its API, whatever the API and its
    /// version: the call carries a global trait path the device resolves itself.
    Reboot {
        /// What to reboot into
        #[arg(long, value_enum, default_value_t = Into::Updater)]
        into: Into,
    },
}

impl DfuCommand {
    /// Commands that reset the device, the connection drops right after
    pub(crate) fn resets_device(&self) -> bool {
        matches!(self, DfuCommand::Reboot { .. })
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum Into {
    /// The updater (bootloader with a transport or recovery app), to receive an update there
    Updater,
    /// The app, the normal reset: switches to a staged image or applies a stored plan if there is one
    App,
}

pub(crate) async fn dfu(cmd: DfuCommand, device: &DynClient) -> Result<()> {
    match cmd {
        DfuCommand::Reboot { into } => {
            let mut boot = BootControl {
                cmd: device
                    .global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl")
                    .cmd_tx_take(),
            };
            let (target, name) = match into {
                Into::Updater => (BootTarget::Updater, "updater"),
                Into::App => (BootTarget::App, "app"),
            };
            boot.reboot(target)
                .call()
                .await
                .map_err(not_found)?
                .map_err(|e| anyhow!("device refused to reboot: {e:?}"))?;
            let api = &device.device_api_info().user_api_version;
            println!("{api:?} reboots into the {name}");
        }
    }
    Ok(())
}

fn not_found(e: Error) -> anyhow::Error {
    match &e {
        Error::RemoteError(r) if matches!(r.kind, ErrorKindOwned::TraitNotFound) => anyhow!(
            "the device doesn't implement {:?}::BootControl (or not in a compatible version)",
            ww_dfu::BOOT_CONTROL_FULL_GID
        ),
        _ => e.into(),
    }
}

/// `ww_dfu::BootControl` trait client, calls carry a global trait path
struct BootControl {
    cmd: Commander,
}

mod boot_control_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_dfu :: BootControl for BootControl,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}
