use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, oneshot};
use wire_weaver_usb_host::wire_weaver_client_common::ww_version::{
    FullVersion, FullVersionOwned, Version, VersionOwned,
};
use wire_weaver_usb_host::wire_weaver_client_common::{Command, DeviceFilter, OnError};
use wire_weaver_usb_host::{ConnectionInfo, usb_worker};

pub async fn connect_usb_dyn_api(filter: DeviceFilter) -> Result<mpsc::UnboundedSender<Command>> {
    let (connected_tx, connected_rx) = oneshot::channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let conn_state = Arc::new(RwLock::new(ConnectionInfo::default()));
    tokio::spawn(async move {
        usb_worker(
            cmd_rx,
            conn_state,
            FullVersion::new("", Version::new(0, 1, 0)),
            64,
        )
        .await;
    });
    cmd_tx
        .send(Command::Connect {
            filter,
            user_protocol_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 1, 0)),
            on_error: OnError::ExitImmediately,
            connected_tx: Some(connected_tx),
        })
        .map_err(|_| anyhow::anyhow!("event loop not running"))?;
    let connection_result = connected_rx
        .await
        .map_err(|_| anyhow::anyhow!("event loop not running"))?;
    connection_result?;
    Ok(cmd_tx)
}
