use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use shrink_wrap::ref_vec::RefVec;
use shrink_wrap::{BufReader, BufWriter, DeserializeShrinkWrap, SerializeShrinkWrap};
use std::net::IpAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, trace, warn};
use wire_weaver_client_common::event_loop_state::CommonState;
use wire_weaver_client_common::{Command, Error};
use wire_weaver_client_common::{
    ww_client_server,
    ww_client_server::{Event, EventKind, RequestKind},
};

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
    #[error("")]
    ExitRequested,
}

impl From<shrink_wrap::Error> for WsError {
    fn from(e: shrink_wrap::Error) -> Self {
        WsError::ShrinkWrap(e)
    }
}

struct State {
    common: CommonState<WsError>,
}

impl Default for State {
    fn default() -> Self {
        State {
            common: CommonState::default(),
        }
    }
}

struct Link {
    _target: WsTarget,
    tx: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    rx: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub async fn ws_worker(mut cmd_rx: mpsc::UnboundedReceiver<Command<WsTarget, WsError>>) {
    debug!("ws worker started");
    let mut state = State::default();
    let mut link = None;
    let mut scratch = [0u8; 2048];
    loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(&mut cmd_rx, l, &mut state, &mut scratch).await
                {
                    Ok(r) => {
                        info!("loop (inner) exited with {:?}", r);
                        if r == EventLoopResult::DisconnectAndExit {
                            break;
                        }
                    }
                    Err(e) => error!("loop (inner) exited with {:?}", e),
                }
                if state.common.exit_on_error {
                    break;
                } else {
                    info!("will try to reconnect");
                    state.common.cancel_all_requests();
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
    state.common.cancel_all_streams();
    state.common.cancel_all_requests();
    debug!("ws worker exited");
}

#[derive(Debug, PartialEq)]
enum EventLoopResult {
    DisconnectKeepStreams,
    DisconnectFromDevice,
    DisconnectAndExit,
}

async fn process_commands_and_endpoints(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command<WsTarget, WsError>>,
    link: &mut Link,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<EventLoopResult, WsError> {
    link.tx.send(Message::Text("versions?".into())).await?;
    let mut link_setup_retries = 5;
    loop {
        let duration = if state.common.link_setup_done {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.rx.next() => {
                match message {
                    Some(Ok(message)) => match handle_message(message, &mut link.tx, state).await? {
                        EventLoopSpinResult::Continue => {}
                        EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                        EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::DisconnectFromDevice),
                        EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::DisconnectAndExit)
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
                    return Ok(EventLoopResult::DisconnectAndExit);
                };
                match handle_command(cmd, &mut link.tx, state, scratch).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::DisconnectFromDevice),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::DisconnectAndExit)
                }
            }
            _ = timer => {
                if !state.common.link_setup_done {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        link.tx.send(Message::Text("versions?".into())).await?;
                        link_setup_retries -= 1;
                    } else {
                        error!("worker exiting, because link setup failed after several retries");
                        return Err(WsError::LinkSetupTimeout);
                    }
                }
                state.common.prune_timed_out_requests();
                // TODO: send accumulated message or Ping
            }
        }
    }
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command<WsTarget, WsError>>,
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
                user_protocol_version,
            } => {
                state
                    .common
                    .on_connect(on_error, connected_tx, user_protocol_version); // TODO: use user protocol version
                let (ws, _response) = tokio_tungstenite::connect_async(format!(
                    "ws://{}:{}/{}",
                    filter.addr, filter.port, filter.path
                ))
                .await?;
                let (tx, rx) = ws.split();
                return Ok(Some(Link {
                    _target: filter,
                    tx,
                    rx,
                }));
            }
            Command::Subscribe {
                path,
                stream_data_tx,
            } => {
                state.common.stream_handlers.insert(path, stream_data_tx);
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
            Command::SendCall { done_tx, .. }
            | Command::SendRead { done_tx, .. }
            | Command::SendWrite { done_tx, .. } => {
                if let Some(tx) = done_tx {
                    let _ = tx.send(Err(Error::Disconnected));
                }
            }
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
) -> Result<EventLoopSpinResult, WsError> {
    match message {
        Message::Binary(data) => {
            if data.is_empty() {
                warn!("got empty event data, ignoring");
                return Ok(EventLoopSpinResult::Continue);
            }
            let mut rd = BufReader::new(&data);
            let event = match Event::des_shrink_wrap(&mut rd) {
                Ok(e) => e,
                Err(e) => {
                    warn!("event deserialization failed: {e:?}, ignoring");
                    return Ok(EventLoopSpinResult::Continue);
                }
            };
            trace!("event: {event:?}");
            match event.result {
                Ok(event_kind) => match event_kind {
                    EventKind::ReturnValue { data } | EventKind::ReadValue { data } => {
                        if let Some((done_tx, _)) = state.common.response_map.remove(&event.seq) {
                            let r = data.as_slice().to_vec();
                            let _ = done_tx.send(Ok(r));
                        }
                    }
                    EventKind::Written => {
                        if let Some((done_tx, _)) = state.common.response_map.remove(&event.seq) {
                            let _ = done_tx.send(Ok(Vec::new()));
                        }
                    }
                    EventKind::StreamUpdate { path, data } => {
                        let path = path.iter().map(|p| p.unwrap().0).collect::<Vec<_>>();
                        let mut should_drop_handler = false;
                        if let Some(tx) = state.common.stream_handlers.get_mut(&path) {
                            let r = data.as_slice().to_vec();
                            should_drop_handler = tx.send(Ok(r)).is_err();
                        }
                        if should_drop_handler {
                            info!(
                                "Dropping stream handler with path: {path:?}, because rx end was dropped"
                            );
                            state.common.stream_handlers.remove(&path);
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    if let Some((done_tx, _)) = state.common.response_map.remove(&event.seq) {
                        let _ = done_tx.send(Err(Error::RemoteError(e)));
                    }
                }
            }
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
                // state.common.max_protocol_mismatched_messages = 10;
                // if let Some(di) = &state.device_info {
                //     state.conn_state.write().await.state = ConnectionState::Connected {
                //         device_info: di.clone(),
                //     };
                // }
                if let Some(tx) = state.common.connected_tx.take() {
                    _ = tx.send(Ok(()));
                }
                state.common.link_setup_done = true;
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
    cmd: Command<WsTarget, WsError>,
    tx: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<EventLoopSpinResult, WsError> {
    match cmd {
        Command::Connect { .. } => {
            warn!("Ignoring Connect while already connected");
        }
        Command::DisconnectKeepStreams { disconnected_tx } => {
            info!("Disconnecting on user request (but keeping streams ready for re-use)");
            tx.send(Message::Close(None)).await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectKeepStreams);
        }
        Command::DisconnectAndExit { disconnected_tx } => {
            info!("Disconnecting and stopping event loop on user request");
            tx.send(Message::Close(None)).await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectAndExit);
        }
        Command::SendCall {
            args_bytes,
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending call to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
                kind: RequestKind::Call {
                    args: RefVec::new_bytes(&args_bytes),
                },
            };
            serialize_request_send(request, tx, state, scratch).await?;
        }
        Command::SendWrite {
            value_bytes,
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending write to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
                kind: RequestKind::Write {
                    data: RefVec::new_bytes(&value_bytes),
                },
            };
            serialize_request_send(request, tx, state, scratch).await?;
        }
        Command::SendRead {
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending read to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
                kind: RequestKind::Read,
            };
            serialize_request_send(request, tx, state, scratch).await?;
        }
        Command::Subscribe {
            path,
            stream_data_tx,
        } => {
            state.common.stream_handlers.insert(path, stream_data_tx);
        }
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn serialize_request_send(
    request: ww_client_server::Request<'_>,
    tx: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    _state: &mut State,
    scratch: &mut [u8],
) -> Result<(), WsError> {
    let mut wr = BufWriter::new(scratch);
    request.ser_shrink_wrap(&mut wr)?;
    let request_bytes = wr.finish_and_take()?.to_vec();

    trace!("Sending request: {:02x?}", request_bytes);
    tx.send(Message::Binary(request_bytes.into())).await?;
    // if link.is_tx_queue_empty() {
    //     state.common.packet_started_instant = None;
    // } else {
    //     state.common.packet_started_instant = Some(Instant::now());
    // }
    // link.force_send().await?; // TODO: check in WireShark whether messages batch together or else force send on timer
    Ok(())
}
