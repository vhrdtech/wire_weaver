use crate::cli::{Cli, Commands};
use crate::cmd::config::ConfigCommand;
use crate::device::Selection;
use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches};
use wire_weaver_client::DynClient;

mod cli;
pub(crate) mod cmd;
mod device;
mod util;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let selection = match &cli.command {
        Commands::List(_) | Commands::Config(_) => Some(Selection::resolve(
            &matches,
            matches!(cli.command, Commands::Config(ConfigCommand::Save)),
        )?),
        _ if cli.need_device() => Some(Selection::resolve(&matches, false)?),
        _ => None,
    };
    let mut device = match &selection {
        Some(selection) if cli.need_device() => Some(
            DynClient::from_config(selection.client_config())
                .connect()
                .await
                .with_context(|| {
                    format!(
                        "connecting to device ({selection}), run 'ww list' to see connected devices"
                    )
                })?,
        ),
        _ => None,
    };

    match cli.command {
        Commands::List(args) => cmd::list::list(args, selection.as_ref().unwrap()).await?,
        Commands::USBLoopback {
            duration_sec,
            packet_size,
        } => {
            cmd::usb_loopback::usb_loopback(device.as_mut().unwrap(), duration_sec, packet_size)
                .await?
        }
        Commands::Api(api_cmd) => cmd::api::api(api_cmd)?,
        Commands::Introspect => cmd::introspect::introspect(device.as_mut().unwrap()).await?,
        Commands::Config(config_cmd) => {
            cmd::config::config(config_cmd, selection.as_ref().unwrap())?
        }

        #[cfg(target_os = "linux")]
        Commands::Udev => {
            todo!()
        }
    }

    if let Some(device) = device {
        device.disconnect().asynch().await?;
    }

    Ok(())
}
