use std::{any::Any, marker::PhantomData, sync::Arc};

#[cfg(feature = "usb")]
use crate::config::InterfaceKind;
use crate::{
    Commander, WwClient,
    config::{ClientConfig, IntrospectBundle, ValidatedConfig},
    device_info::DeviceApiInfo,
    event_loop::command::Command,
};
use anyhow::{Error, Result, anyhow, bail};
use tokio::sync::{mpsc, oneshot};
use tracing::error;
use ww_version::FullVersionOwned;

pub struct PreparedConnection<T> {
    config: ClientConfig,
    _phantom: PhantomData<T>,
}

impl<T: WwClient> PreparedConnection<T> {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            _phantom: PhantomData,
        }
    }

    pub async fn connect(self) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);

        for iface_kind in config.interfaces() {
            #[cfg(feature = "usb")]
            if iface_kind == InterfaceKind::Usb {
                match crate::usb::try_connect(&config, &mut cmd_rx).await {
                    Ok(Some(h)) => {
                        match try_connect(&cmd_tx, h, config.client_version.clone()).await {
                            Ok(device_api_info) => {
                                return Ok(T::from_cmd(
                                    create_commander(config, cmd_tx, device_api_info).await,
                                ));
                            }
                            Err(e) => {
                                error!(
                                    "Connecting to device failed: '{e}', will try other devices if any"
                                );
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => bail_on_ambiguous(e)?,
                }
            }
        }

        Err(anyhow!("No devices found to connect to"))
    }

    pub fn connect_blocking(self) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);

        for iface_kind in config.interfaces() {
            #[cfg(feature = "usb")]
            if iface_kind == InterfaceKind::Usb {
                match crate::usb::try_connect_blocking(&config, &mut cmd_rx) {
                    Ok(Some(h)) => {
                        match try_connect_blocking(&cmd_tx, h, config.client_version.clone()) {
                            Ok(device_api_info) => {
                                return Ok(T::from_cmd(create_commander_blocking(
                                    config,
                                    cmd_tx,
                                    device_api_info,
                                )));
                            }
                            Err(e) => {
                                error!(
                                    "Connecting to device failed: '{e}', will try other devices if any"
                                );
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => bail_on_ambiguous(e)?,
                }
            }
        }

        Err(anyhow!("No devices found to connect to"))
    }

    pub fn connect_promise(self) {
        todo!()
    }

    /// Try to connect to a selected device immediately, but if it fails or there are no devices, keep trying.
    /// Even when connected and then disconnected or errored out, try again, until commanded to stop and exit.
    /// Commander and all open streams will be kept active.
    ///
    /// NOTE: not implemented yet
    pub fn connect_keep_trying(self) {
        // figure out how to handle Commander.connected_device if connected to a different device, reject by serial?
        todo!()
    }
}

async fn try_connect(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    client_version: Box<FullVersionOwned>,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle,
            client_version,
            connected_tx: Some(connected_tx),
            failed_tx: None,
        })
        .await
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    let info = connected_rx.await?;
    let device_api_info = info.result?;
    Ok(device_api_info)
}

fn try_connect_blocking(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    client_version: Box<FullVersionOwned>,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .blocking_send(Command::Connect {
            handle,
            client_version,
            connected_tx: Some(connected_tx),
            failed_tx: None,
        })
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    let info = connected_rx.blocking_recv()?;
    let device_api_info = info.result?;
    Ok(device_api_info)
}

fn create_commander_inner(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let mut commander = Commander::new(cmd_tx);
    commander.set_local_timeout(config.default_timeout);
    if let Some(introspect_client) = config.introspect_client {
        if introspect_client.api_hash.no_docs == device_api_info.user_api_hash.no_docs {
            // connected device API is exactly the same as client, no need to download or look for cached one
            commander.set_device_introspect(introspect_client.clone());
        }
        commander.set_client_introspect(introspect_client);
    }
    commander.connected_device = device_api_info;
    commander
}

/// NOTE: keep in sync with [create_commander_blocking]
async fn create_commander(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let device_api_hash = device_api_info.user_api_hash.clone();
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    if commander.device_introspect().is_none() {
        if let Ok(api_bundle) = commander.introspect().get().await {
            commander.set_device_introspect(IntrospectBundle {
                api_bundle: Arc::new(api_bundle),
                api_hash: device_api_hash,
            });
        }
    }
    commander
}

/// NOTE: keep in sync with [create_commander]
fn create_commander_blocking(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let device_api_hash = device_api_info.user_api_hash.clone();
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    if commander.device_introspect().is_none() {
        if let Ok(api_bundle) = commander.introspect().get_blocking() {
            commander.set_device_introspect(IntrospectBundle {
                api_bundle: Arc::new(api_bundle),
                api_hash: device_api_hash,
            });
        }
    }
    commander
}

fn bail_on_ambiguous(e: Error) -> Result<()> {
    if let Some(e) = e.downcast_ref::<crate::Error>() {
        if let crate::Error::AmbiguousDeviceChoice(devices) = e {
            println!("Matched devices:");
            for dev in devices {
                println!("{dev:?}");
            }
            bail!("Ambiguous device choice");
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}
