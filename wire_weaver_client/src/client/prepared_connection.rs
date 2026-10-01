use std::{any::Any, marker::PhantomData};

use crate::config::InterfaceKind;
use crate::event_loop::transport::Selected;
use crate::{
    Commander, WwClient,
    config::{ClientConfig, IntrospectBundle, ValidatedConfig},
    device_info::DeviceApiInfo,
    event_loop::command::Command,
};
use anyhow::{Error, Result};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

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
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);
        let mut unmatched = vec![];

        for iface_kind in check_interfaces(&config)? {
            #[cfg(feature = "usb")]
            if iface_kind == InterfaceKind::Usb {
                match crate::usb::try_connect(&config, &mut cmd_rx).await? {
                    Selected::Device { handle, info } => {
                        let device_api_info = try_connect(&cmd_tx, handle, &config)
                            .await
                            .map_err(|e| connect_failed(info, e))?;
                        return Ok(T::from_cmd(
                            create_commander(config, cmd_tx, device_api_info).await,
                        ));
                    }
                    Selected::NotFound { unmatched: u } => unmatched.extend(u),
                }
            }
            if iface_kind == InterfaceKind::Rtt {
                match try_select_rtt(&config, &mut cmd_rx)? {
                    Selected::Device { handle, info } => {
                        let device_api_info = try_connect(&cmd_tx, handle, &config)
                            .await
                            .map_err(|e| connect_failed(info, e))?;
                        return Ok(T::from_cmd(
                            create_commander(config, cmd_tx, device_api_info).await,
                        ));
                    }
                    Selected::NotFound { unmatched: u } => unmatched.extend(u),
                }
            }
        }

        Err(not_found(&config, unmatched))
    }

    pub fn connect_blocking(self) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);
        let mut unmatched = vec![];

        for iface_kind in check_interfaces(&config)? {
            #[cfg(feature = "usb")]
            if iface_kind == InterfaceKind::Usb {
                match crate::usb::try_connect_blocking(&config, &mut cmd_rx)? {
                    Selected::Device { handle, info } => {
                        let device_api_info = try_connect_blocking(&cmd_tx, handle, &config)
                            .map_err(|e| connect_failed(info, e))?;
                        return Ok(T::from_cmd(create_commander_blocking(
                            config,
                            cmd_tx,
                            device_api_info,
                        )));
                    }
                    Selected::NotFound { unmatched: u } => unmatched.extend(u),
                }
            }
            if iface_kind == InterfaceKind::Rtt {
                match try_select_rtt(&config, &mut cmd_rx)? {
                    Selected::Device { handle, info } => {
                        let device_api_info = try_connect_blocking(&cmd_tx, handle, &config)
                            .map_err(|e| connect_failed(info, e))?;
                        return Ok(T::from_cmd(create_commander_blocking(
                            config,
                            cmd_tx,
                            device_api_info,
                        )));
                    }
                    Selected::NotFound { unmatched: u } => unmatched.extend(u),
                }
            }
        }

        Err(not_found(&config, unmatched))
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

fn try_select_rtt(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "rtt")]
    return crate::rtt::try_connect(config, cmd_rx);
    #[cfg(not(feature = "rtt"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "RTT selected in the client config, but wire_weaver_client is built without the `rtt` feature"
        ))
    }
}

async fn try_connect(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    config: &ValidatedConfig,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
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
    config: &ValidatedConfig,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .blocking_send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
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
        match commander.introspect().get_as_sent().await {
            Ok(Some((sent_api_bundle, sent_size))) => commander.set_device_introspect(
                IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size),
            ),
            Ok(None) => debug!("device has introspection disabled and its API is not in the cache"),
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    commander.resolve_api_match();
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
        match commander.introspect().get_as_sent_blocking() {
            Ok(Some((sent_api_bundle, sent_size))) => commander.set_device_introspect(
                IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size),
            ),
            Ok(None) => debug!("device has introspection disabled and its API is not in the cache"),
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    commander.resolve_api_match();
    commander
}

fn check_interfaces(config: &ValidatedConfig) -> Result<Vec<InterfaceKind>> {
    let interfaces = config.interfaces();
    if interfaces.is_empty() {
        return Err(crate::Error::NoTransportSelected.into());
    }
    Ok(interfaces)
}

fn not_found(config: &ValidatedConfig, unmatched: Vec<crate::DeviceInfo>) -> Error {
    crate::Error::DeviceNotFound {
        filters: config.describe_filters(),
        unmatched,
    }
    .into()
}

// Event loop is already started for this device, so there is no point in trying other ones
fn connect_failed(device: Box<crate::DeviceInfo>, e: Error) -> Error {
    crate::Error::ConnectFailed {
        device,
        reason: format!("{e:#}"),
    }
    .into()
}
