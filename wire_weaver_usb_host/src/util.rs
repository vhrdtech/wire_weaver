use crate::usb_worker;
use tokio::sync::mpsc;
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client::{ClientConfig, CommandSender, DeviceFilter, Error, OnError};

/// Connect to the USB device without code-generated WireWeaver client code. Intended use cases are:
///     * request API and type definitions from the device itself
///     * load API and type definitions from a file system or GitHub
/// Then communicate with the device via dynamically generated UI or through REPL.
pub async fn connect_runtime_api(
    filter: DeviceFilter,
    config: ClientConfig,
) -> Result<CommandSender, Error> {
    let mut cmd_tx = start_worker(config.cmd_queue_size);
    cmd_tx
        .connect(
            filter,
            FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
            config.on_error,
        )
        .await?;
    Ok(cmd_tx)
}

/// Connect to the USB device without code-generated WireWeaver client code. Intended use cases are:
///     * request API and type definitions from the device itself
///     * load API and type definitions from a file system or GitHub
/// Then communicate with the device via dynamically generated UI or through REPL.
pub fn connect_runtime_api_blocking(
    filter: DeviceFilter,
    config: ClientConfig,
) -> Result<CommandSender, Error> {
    let mut cmd_tx = start_worker(config.cmd_queue_size);
    cmd_tx.connect_blocking(
        filter,
        FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
        OnError::ExitImmediately,
    )?;
    Ok(cmd_tx)
}

fn start_worker(cmd_queue_size: usize) -> CommandSender {
    let (transport_cmd_tx, transport_cmd_rx) = mpsc::channel(cmd_queue_size);
    let cmd_tx = CommandSender::new(transport_cmd_tx);
    tokio::spawn(async move {
        usb_worker(transport_cmd_rx).await;
    });
    cmd_tx
}
