use crate::cli::Commands;
use anyhow::{Context, Result};
use clap::Parser;
use wire_weaver_usb_host::wire_weaver_client_common::{Command, DeviceFilter};

mod cli;
pub(crate) mod cmd;
mod util;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = cli::Cli::parse();
    let mut device = if cli.need_device() {
        let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
        let device = wire_weaver_usb_host::util::connect_runtime_api(filter.clone())
            .await
            .context(format!(
                "Connecting to USB device with filter: {filter:02x?}"
            ))?;
        Some(device)
    } else {
        None
    };

    match cli.command {
        Commands::USBLoopback {
            duration_sec,
            packet_size,
        } => {
            cmd::usb_loopback::usb_loopback(device.as_mut().unwrap(), duration_sec, packet_size)
                .await?
        }
        Commands::Api(api_cmd) => cmd::api::api(api_cmd)?,
        Commands::Introspect => cmd::introspect::introspect(device.as_mut().unwrap()).await?,

        #[cfg(target_os = "linux")]
        Commands::Udev => {
            todo!()
        }
    }

    if let Some(device) = device {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        _ = device.send(Command::DisconnectAndExit {
            disconnected_tx: Some(tx),
        });
        _ = rx.await;
    }

    Ok(())
}
