use std::{any::Any, marker::PhantomData};

use crate::config::InterfaceKind;
use crate::event_loop::transport::Selected;
use crate::{
    Commander, WwClient,
    config::{ClientConfig, IntrospectBundle, ValidatedConfig},
    device_info::DeviceApiInfo,
    event_loop::command::{Command, EventLoopResidual},
};
use anyhow::{Error, Result};
use std::sync::{Arc, Mutex};
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
        let (device_api_info, residual_rx) =
            connect_to(&config, &cmd_tx, &mut cmd_rx).await.result?;
        let reconnect = Reconnect::new(config.clone(), residual_rx);
        let mut commander = create_commander(config, cmd_tx, device_api_info).await;
        commander.set_reconnect(reconnect);
        Ok(T::from_cmd(commander))
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
            if iface_kind == InterfaceKind::InProcess {
                match try_select_in_process(&config, &mut cmd_rx)? {
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
            if iface_kind == InterfaceKind::WebSocket {
                match try_select_ws(&config, &mut cmd_rx)? {
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
            if iface_kind == InterfaceKind::Udp {
                match try_select_udp(&config, &mut cmd_rx)? {
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

/// What a connection attempt left behind, see [connect_to].
pub(crate) struct Attempt {
    /// The device's API and the receiver of the event loop's residual, or why the attempt failed.
    pub(crate) result: Result<(DeviceApiInfo, oneshot::Receiver<EventLoopResidual>)>,
    /// Set when an event loop was started and failed to connect: the receiver of its residual.
    pub(crate) started: Option<oneshot::Receiver<EventLoopResidual>>,
}

impl Attempt {
    fn failed(e: Error) -> Self {
        Attempt {
            result: Err(e),
            started: None,
        }
    }
}

/// Selects a device with the transports of `config` (in order), starts an event loop on `cmd_rx` for it and connects.
/// The loop's residual (its command channel back) comes through the returned receiver when it exits; keeping it is
/// what makes [Commander::reconnect] possible. `cmd_rx` is taken when a transport found a device.
pub(crate) async fn connect_to(
    config: &ValidatedConfig,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Attempt {
    let mut unmatched = vec![];
    let kinds = match check_interfaces(config) {
        Ok(kinds) => kinds,
        Err(e) => return Attempt::failed(e),
    };
    for iface_kind in kinds {
        let selected = match iface_kind {
            #[cfg(feature = "usb")]
            InterfaceKind::Usb => crate::usb::try_connect(config, cmd_rx).await,
            #[cfg(not(feature = "usb"))]
            InterfaceKind::Usb => continue,
            InterfaceKind::InProcess => try_select_in_process(config, cmd_rx),
            InterfaceKind::Rtt => try_select_rtt(config, cmd_rx),
            InterfaceKind::WebSocket => try_select_ws(config, cmd_rx),
            InterfaceKind::Udp => try_select_udp(config, cmd_rx),
            // not a transport the client can connect over yet
            InterfaceKind::Ipc => continue,
        };
        match selected {
            Ok(Selected::Device { handle, info }) => {
                let (residual_tx, residual_rx) = oneshot::channel();
                return match try_connect(cmd_tx, handle, config, residual_tx).await {
                    Ok(device_api_info) => Attempt {
                        result: Ok((device_api_info, residual_rx)),
                        started: None,
                    },
                    Err(e) => Attempt {
                        result: Err(connect_failed(info, e)),
                        started: Some(residual_rx),
                    },
                };
            }
            Ok(Selected::NotFound { unmatched: u }) => unmatched.extend(u),
            Err(e) => return Attempt::failed(e),
        }
    }
    Attempt::failed(not_found(config, unmatched))
}

/// What [Commander::reconnect] needs: the configuration the Commander was connected with, and the event loop's
/// command channel once that exited.
pub(crate) struct Reconnect {
    config: ValidatedConfig,
    state: Mutex<LoopState>,
}

enum LoopState {
    /// An event loop runs (or exited and its residual is in the receiver)
    Running(oneshot::Receiver<EventLoopResidual>),
    /// No loop: the command channel waits for the next one
    Stopped(mpsc::Receiver<Command>),
    /// A reconnect is under way
    Busy,
    /// The attempt failed after it took the command channel: nothing to connect on any more
    Lost,
}

impl Reconnect {
    fn new(config: ValidatedConfig, residual: oneshot::Receiver<EventLoopResidual>) -> Arc<Self> {
        Arc::new(Reconnect {
            config,
            state: Mutex::new(LoopState::Running(residual)),
        })
    }

    /// Connects again with the configuration of the first connection, on the command channel of the event loop that
    /// exited (after `disconnect()`).
    pub(crate) async fn run(
        &self,
        cmd_tx: &mpsc::Sender<Command>,
    ) -> Result<DeviceApiInfo, crate::Error> {
        let state = std::mem::replace(
            &mut *self.state.lock().unwrap_or_else(|e| e.into_inner()),
            LoopState::Busy,
        );
        let mut cmd_rx = match state {
            LoopState::Stopped(cmd_rx) => Some(cmd_rx),
            LoopState::Running(mut residual) => {
                // the residual is sent before `disconnect()` returns, so it is there if the loop is gone
                let exited = residual.try_recv().map_err(|_| ());
                match exited {
                    Ok(residual) => Some(residual.cmd_rx),
                    Err(()) => {
                        *self.state.lock().unwrap_or_else(|e| e.into_inner()) =
                            LoopState::Running(residual);
                        return Err(crate::Error::User(
                            "still connected (or the event loop is gone), disconnect().keep_streams() first".into(),
                        ));
                    }
                }
            }
            LoopState::Busy => {
                return Err(crate::Error::User(
                    "a reconnect is already under way".into(),
                ));
            }
            LoopState::Lost => {
                *self.state.lock().unwrap_or_else(|e| e.into_inner()) = LoopState::Lost;
                return Err(crate::Error::User(
                    "the event loop is gone, connect with a new client".into(),
                ));
            }
        };
        let Attempt { result, started } = connect_to(&self.config, cmd_tx, &mut cmd_rx).await;
        let (next, answer) = match result {
            Ok((info, residual)) => (LoopState::Running(residual), Ok(info)),
            Err(e) => {
                let next = match (cmd_rx, started) {
                    (Some(cmd_rx), _) => LoopState::Stopped(cmd_rx),
                    (None, Some(residual)) => LoopState::Running(residual),
                    (None, None) => LoopState::Lost,
                };
                (next, Err(crate::Error::Other(format!("{e:#}"))))
            }
        };
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = next;
        answer
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

fn try_select_ws(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "ws")]
    return crate::ws::try_connect(config, cmd_rx);
    #[cfg(not(feature = "ws"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "WebSocket selected in the client config, but wire_weaver_client is built without the `ws` feature"
        ))
    }
}

fn try_select_udp(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "udp")]
    return crate::udp::try_connect(config, cmd_rx);
    #[cfg(not(feature = "udp"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "UDP selected in the client config, but wire_weaver_client is built without the `udp` feature"
        ))
    }
}

fn try_select_in_process(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "in_process")]
    return crate::in_process::try_connect(config, cmd_rx);
    #[cfg(not(feature = "in_process"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "In-process device selected in the client config, but wire_weaver_client is built without the `in_process` feature"
        ))
    }
}

async fn try_connect(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    config: &ValidatedConfig,
    residual_tx: oneshot::Sender<EventLoopResidual>,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
            link_setup_timeout_ms: config.link_setup_timeout_ms,
            tap: config.tap.clone(),
            connected_tx: Some(connected_tx),
            failed_tx: Some(residual_tx),
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
            link_setup_timeout_ms: config.link_setup_timeout_ms,
            tap: config.tap.clone(),
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
    commander.traits_only = config.traits_only;
    commander.set_stream_queue_bytes(config.stream_queue_bytes);
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
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent().await {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
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
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent_blocking() {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
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
