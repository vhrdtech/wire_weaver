use crate::usb_worker;
use tokio::sync::mpsc;
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client_common::{CommandSender, DeviceFilter, Error, OnError};

/// Connect to the USB device without code-generated WireWeaver client code. Intended use cases are:
///     * request API and type definitions from the device itself
///     * load API and type definitions from a file system or GitHub
/// Then communicate with the device via dynamically generated UI or through REPL.
pub async fn connect_runtime_api(filter: DeviceFilter) -> Result<CommandSender, Error> {
    let mut cmd_tx = start_worker();
    cmd_tx
        .connect(
            filter,
            FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
            OnError::ExitImmediately,
        )
        .await?;
    Ok(cmd_tx)
}

/// Connect to the USB device without code-generated WireWeaver client code. Intended use cases are:
///     * request API and type definitions from the device itself
///     * load API and type definitions from a file system or GitHub
/// Then communicate with the device via dynamically generated UI or through REPL.
pub fn connect_runtime_api_blocking(filter: DeviceFilter) -> Result<CommandSender, Error> {
    let mut cmd_tx = start_worker();
    cmd_tx.connect_blocking(
        filter,
        FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
        OnError::ExitImmediately,
    )?;
    Ok(cmd_tx)
}

fn start_worker() -> CommandSender {
    let (transport_cmd_tx, transport_cmd_rx) = mpsc::unbounded_channel();
    let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
    let cmd_tx = CommandSender::new(transport_cmd_tx, dispatcher_msg_rx);
    tokio::spawn(async move {
        usb_worker(transport_cmd_rx, dispatcher_msg_tx).await;
    });
    cmd_tx
}
