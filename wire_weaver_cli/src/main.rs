use crate::cli::Commands;
use crate::util::connect_usb_dyn_api;
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

    match cli.command {
        Commands::USBLoopback {
            duration_sec,
            packet_size,
        } => {
            let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
            let mut device = connect_usb_dyn_api(filter.clone()).await.context(format!(
                "Connecting to USB device with filter: {filter:02x?}"
            ))?;

            cmd::usb_loopback::usb_loopback(&mut device, duration_sec, packet_size).await?;

            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            _ = device.send(Command::DisconnectAndExit {
                disconnected_tx: Some(tx),
            });
            _ = rx.await;
        }

        Commands::Api(api_cmd) => cmd::api::api(api_cmd)?,

        #[cfg(target_os = "linux")]
        Commands::Udev => {
            todo!()
        }
    }

    Ok(())
}
