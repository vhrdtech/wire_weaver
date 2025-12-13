use anyhow::Result;
use tokio::sync::mpsc;
use wire_weaver_usb_host::usb_worker;
use wire_weaver_usb_host::wire_weaver_client_common::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_usb_host::wire_weaver_client_common::{CommandSender, DeviceFilter, OnError};

pub async fn connect_usb_dyn_api(filter: DeviceFilter) -> Result<CommandSender> {
    let (transport_cmd_tx, transport_cmd_rx) = mpsc::unbounded_channel();
    let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
    let mut cmd_tx = CommandSender::new(transport_cmd_tx, dispatcher_msg_rx);
    tokio::spawn(async move {
        usb_worker(transport_cmd_rx, dispatcher_msg_tx).await;
    });
    cmd_tx
        .connect(
            filter,
            FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
            OnError::ExitImmediately,
        )
        .await?;
    Ok(cmd_tx)
}
