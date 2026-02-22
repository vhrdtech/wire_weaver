use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::net::IpAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, trace, warn};
use wire_weaver_client_common::event_loop_state::CommonState;
use wire_weaver_client_common::rx_dispatcher::DispatcherMessage;
use wire_weaver_client_common::{Command, DeviceInfoBundle, Error};

pub struct WsTarget {
    pub addr: IpAddr,
    pub port: u16,
    pub path: String,
}

#[derive(thiserror::Error, Debug)]
pub enum WsError {
    #[error("All command senders were dropped")]
    CmdTxDropped,
    #[error("ws io error {}", .0)]
    Io(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Link setup failed after several retries")]
    LinkSetupTimeout,
    #[error("Other end disconnected?")]
    UnexpectedDisconnect,
    #[error("Failed to serialize or deserialize: {:?}", .0)]
    ShrinkWrap(shrink_wrap::Error),
    #[error("No compatible protocol versions available")]
    IncompatibleDeviceProtocol,
    #[error("Link setup error")]
    LinkSetupError,
    #[error("Transport specific error: {}", .0)]
    Transport(String),
    #[error("Internal error {}", .0)]
    Internal(String),
    #[error("")]
    ExitRequested,
}

impl From<shrink_wrap::Error> for WsError {
    fn from(e: shrink_wrap::Error) -> Self {
        WsError::ShrinkWrap(e)
    }
}

#[derive(Default)]
struct State {
    common: CommonState,
}

struct Link {
    _target: WsTarget,
    tx: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    rx: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub async fn ws_worker(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    mut to_dispatcher: mpsc::UnboundedSender<DispatcherMessage>,
) {
    debug!("ws worker started");
    let mut state = State::default();
    let mut link = None;
    loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(&mut cmd_rx, l, &mut state, &mut to_dispatcher)
                    .await
                {
                    Ok(r) => {
                        info!("loop (inner) exited with {:?}", r);
                        if r == EventLoopResult::Exit {
                            break;
                        }
                    }
                    Err(e) => error!("loop (inner) exited with {:?}", e),
                }
                if state.common.exit_on_error {
                    break;
                } else {
                    info!("will try to reconnect");
                    state.common.on_disconnect();
                    link = None;
                    continue;
                }
            }
            None => match wait_for_connection_and_queue_commands(&mut cmd_rx, &mut state).await {
                Ok(Some(l)) => {
                    link = Some(l);
                }
                Ok(None) => {}
                Err(e) => {
                    error!("wait_for_connection.. exited with {:?}", e);
                    break;
                }
            },
        }
    }
    debug!("ws worker exited");
}

#[derive(Debug, PartialEq)]
enum EventLoopResult {
    DisconnectKeepStreams,
    Disconnect,
    Exit,
}

async fn process_commands_and_endpoints(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    link: &mut Link,
    state: &mut State,
    to_dispatcher: &mut mpsc::UnboundedSender<DispatcherMessage>,
) -> Result<EventLoopResult, WsError> {
    link.tx.send(Message::Text("versions?".into())).await?;
    let mut link_setup_retries = 5;
    loop {
        let duration = if state.common.link_up {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.rx.next() => {
                match message {
                    Some(Ok(message)) => match handle_message(message, &mut link.tx, state, to_dispatcher).await? {
                        EventLoopSpinResult::Continue => {}
                        EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                        EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                        EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                    }
                    Some(Err(e)) => {
                        return Err(e.into());
                    }
                    None => {
                        debug!("got None from rx");
                        return Err(WsError::UnexpectedDisconnect);
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("all tx instances were dropped, exiting");
                    link.tx.send(Message::Close(None)).await?;
                    return Ok(EventLoopResult::Exit);
                };
                match handle_command(cmd, &mut link.tx, state).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                }
            }
            _ = timer => {
                if !state.common.link_up {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        link.tx.send(Message::Text("versions?".into())).await?;
                        link_setup_retries -= 1;
                    } else {
                        error!("worker exiting, because link setup failed after several retries");
                        return Err(WsError::LinkSetupTimeout);
                    }
                }
                // TODO: send accumulated message or Ping
            }
        }
    }
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    state: &mut State,
) -> Result<Option<Link>, WsError> {
    loop {
        let Some(cmd) = cmd_rx.recv().await else {
            debug!("ws worker exiting, because all command senders were dropped");
            return Err(WsError::CmdTxDropped);
        };
        match cmd {
            Command::Connect {
                filter,
                on_error,
                connected_tx,
                client_version,
            } => {
                let Some((addr, port, path)) = filter.as_web_socket() else {
                    return Err(WsError::Transport(format!(
                        "{filter:?} is not supported for WebSocket"
                    )));
                };
                state
                    .common
                    .on_connect(on_error, connected_tx, client_version); // TODO: use user protocol version
                let (ws, _response) =
                    tokio_tungstenite::connect_async(format!("ws://{}:{}/{}", addr, port, path))
                        .await?;
                let (tx, rx) = ws.split();
                return Ok(Some(Link {
                    _target: WsTarget { addr, port, path },
                    tx,
                    rx,
                }));
            }
            Command::RegisterTracer { trace_event_tx } => {
                state.common.tracers.push(trace_event_tx);
            }
            Command::DisconnectKeepStreams { disconnected_tx } => {
                if let Some(tx) = disconnected_tx {
                    let _ = tx.send(());
                }
                return Ok(None);
            }
            Command::DisconnectAndExit { disconnected_tx } => {
                if let Some(tx) = disconnected_tx {
                    let _ = tx.send(());
                }
                state.common.exit_on_error = true;
                return Err(WsError::ExitRequested);
            }
            Command::SendMessage { .. } => {
                warn!("ignoring send message while disconnected");
            }
            Command::LoopbackTest { .. } => {}
        }
    }
}

enum EventLoopSpinResult {
    Continue,
    DisconnectKeepStreams,
    DisconnectAndExit,
    DisconnectFromDevice,
}

async fn handle_message(
    message: Message,
    tx: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    state: &mut State,
    to_dispatcher: &mut mpsc::UnboundedSender<DispatcherMessage>,
) -> Result<EventLoopSpinResult, WsError> {
    match message {
        Message::Binary(data) => {
            if data.is_empty() {
                warn!("got empty event data, ignoring");
                return Ok(EventLoopSpinResult::Continue);
            }
            state.common.trace_event(&data);
            to_dispatcher
                .send(DispatcherMessage::MessageBytes(data.into()))
                .map_err(|_| WsError::Internal("rx dispatcher is not running".into()))?;
        }
        Message::Close(_close_frame) => {
            info!("Received Disconnect from remote device, exiting");
            return Ok(EventLoopSpinResult::DisconnectFromDevice);
        }
        Message::Ping(_bytes) => {
            trace!("Ping");
        }
        Message::Text(sideband_text) => {
            let pieces = sideband_text.split(' ').collect::<Vec<_>>();
            if pieces.is_empty() {
                return Err(WsError::LinkSetupError);
            }
            let op = pieces[0];
            if op == "device_info" {
                tx.send(Message::Text("link_setup 2048 0 6 0 1 100 0 1".into()))
                    .await?; // TODO: send proper versions
            } else if op == "link_setup_result" {
                let version_matches = true;
                if !version_matches {
                    error!("device rejected LinkSetup, exiting");
                    if let Some(tx) = state.common.connected_tx.take() {
                        _ = tx.send(Err(Error::IncompatibleDeviceProtocol));
                    }
                    return Err(WsError::IncompatibleDeviceProtocol);
                }
                info!("LinkSetup complete");
                to_dispatcher
                    .send(DispatcherMessage::Connected)
                    .map_err(|_| WsError::Internal("rx dispatcher is not running".into()))?;
                if let Some(tx) = state.common.connected_tx.take() {
                    _ = tx.send(Ok(DeviceInfoBundle::empty())); // TODO: ws: device info bundle
                }
                state.common.link_up = true;
            } else {
                error!("Unexpected sideband message received: {op}");
                return Err(WsError::LinkSetupError);
            }
        }
        Message::Pong(_) => {}
        Message::Frame(_) => {}
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn handle_command(
    cmd: Command,
    tx: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    state: &mut State,
) -> Result<EventLoopSpinResult, WsError> {
    match cmd {
        Command::Connect { .. } => {
            warn!("Ignoring Connect while already connected");
        }
        Command::RegisterTracer { trace_event_tx } => {
            state.common.tracers.push(trace_event_tx);
        }
        Command::DisconnectKeepStreams { disconnected_tx } => {
            info!("Disconnecting on user request (but keeping streams ready for re-use)");
            state.common.trace_disconnect("client request", true);
            tx.send(Message::Close(None)).await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectKeepStreams);
        }
        Command::DisconnectAndExit { disconnected_tx } => {
            info!("Disconnecting and stopping event loop on user request");
            state.common.trace_disconnect("client request", false);
            tx.send(Message::Close(None)).await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectAndExit);
        }
        Command::SendMessage { bytes } => {
            state.common.trace_request(&bytes);
            tx.send(Message::Binary(bytes.into())).await?;
            // TODO: check in WireShark whether messages batch together or else force send on timer
        }
        Command::LoopbackTest { .. } => {
            todo!()
        }
    }
    Ok(EventLoopSpinResult::Continue)
}
