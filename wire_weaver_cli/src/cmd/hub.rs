//! `ww hub`: the per-user hub that holds devices and serves every program (SRV-13). `serve` runs it in the
//! foreground, `take` has it hold a device (CLT-39); status, stop, who, keep and lend come with TOOL-17.

use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum HubCommand {
    /// Run the hub in the foreground until it is idle or stopped (programs start it by themselves when it pays off;
    /// this is for watching its log or debugging)
    Serve {
        /// Exit after this many seconds without connections (default 300, or WW_HUB_IDLE_EXIT)
        #[arg(long)]
        idle_exit: Option<u64>,
    },
    /// Have the hub open a device now and keep it, so every program reaches it through the hub from its first call
    /// (starts the hub if none runs)
    Take {
        /// Device key as the hub lists it (usb:<vid>:<pid>:<serial>), or the serial number of a WireWeaver USB device
        device: String,
    },
}

pub(crate) async fn hub(cmd: HubCommand) -> Result<()> {
    match cmd {
        HubCommand::Serve { idle_exit } => {
            let mut config = ww_hub::HubConfig::from_env()?;
            if let Some(s) = idle_exit {
                config.idle_exit = Duration::from_secs(s);
            }
            match ww_hub::serve(config).await? {
                ww_hub::Exit::AlreadyRunning => {
                    anyhow::bail!("a hub runs for this user already")
                }
                exit => eprintln!("ww hub: {exit:?}"),
            }
        }
        HubCommand::Take { device } => {
            let key = device_key(&device).await?;
            wire_weaver_client::hub::take(&key).await?;
            eprintln!("the ww hub holds {key}");
        }
    }
    Ok(())
}

/// A key as given, or the key of the one WireWeaver USB device with this serial number.
async fn device_key(device: &str) -> Result<String> {
    if device.contains(':') {
        return Ok(device.to_string());
    }
    let devices = wire_weaver_client::list_usb_devices().await?;
    let mut keys = devices
        .iter()
        .filter(|d| d.serials.iter().any(|s| s == device))
        .filter_map(wire_weaver_client::hub::key_of);
    match (keys.next(), keys.next()) {
        (Some(key), None) => Ok(key),
        (None, _) => anyhow::bail!("no WireWeaver USB device with serial {device}"),
        (Some(_), Some(_)) => {
            anyhow::bail!(
                "several devices with serial {device}, give the key (usb:<vid>:<pid>:<serial>)"
            )
        }
    }
}
