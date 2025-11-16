use crate::ww_nusb::{Sink, Source};
use crate::{ConnectionInfo, ConnectionState, IRQ_MAX_PACKET_SIZE, MAX_MESSAGE_SIZE, UsbError};
use nusb::transfer::TransferError;
use nusb::{DeviceInfo, Interface, Speed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, trace, warn};
use wire_weaver::shrink_wrap::ref_vec::RefVec;
use wire_weaver::shrink_wrap::{BufReader, BufWriter, DeserializeShrinkWrap, SerializeShrinkWrap};
use wire_weaver_client_common::ww_client_server::PathKindOwned;
use wire_weaver_client_common::{
    Command, Error, OnError,
    event_loop_state::CommonState,
    ww_client_server::{Event, EventKind, Request, RequestKind},
};
use wire_weaver_usb_link::{
    DisconnectReason, Error as LinkError, MessageKind, ProtocolInfo, WireWeaverUsbLink,
};

struct State {
    common: CommonState,
    message_rx: [u8; MAX_MESSAGE_SIZE],
    user_protocol: ProtocolInfo,
    conn_state: Arc<RwLock<ConnectionInfo>>,
    device_info: Option<DeviceInfo>,
    max_protocol_mismatched_messages: u32,
}

impl State {
    fn new(conn_state: Arc<RwLock<ConnectionInfo>>, user_protocol: ProtocolInfo) -> Self {
        State {
            common: CommonState::default(),
            message_rx: [0u8; MAX_MESSAGE_SIZE],
            user_protocol,
            conn_state,
            device_info: None,
            max_protocol_mismatched_messages: 10,
        }
    }

    fn on_disconnect(&mut self) {
        self.common.on_disconnect();
        self.device_info = None;
        self.max_protocol_mismatched_messages = 10;
    }
}

pub async fn usb_worker(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    conn_state: Arc<RwLock<ConnectionInfo>>,
    user_protocol: ProtocolInfo,
    max_hs_usb_packet_size: usize,
) {
    let mut state = State::new(conn_state, user_protocol);
    state.conn_state.write().await.worker_running = false;

    let mut tx_buf = [0u8; IRQ_MAX_PACKET_SIZE];
    let mut rx_buf = [0u8; IRQ_MAX_PACKET_SIZE];
    let mut link = None;

    loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(&mut cmd_rx, l, &mut state).await {
                    Ok(r) => {
                        info!("usb event loop (inner) exited with {:?}", r);
                        if r == EventLoopResult::Exit {
                            break;
                        }
                    }
                    Err(e) => error!("usb event loop (inner) exited with {:?}", e),
                }
                if state.common.exit_on_error {
                    break;
                } else {
                    info!("will try to reconnect");
                    state.common.cancel_all_requests();
                    state.on_disconnect();
                    link = None;
                    continue;
                }
            }
            None => match wait_for_connection_and_queue_commands(&mut cmd_rx, &mut state).await {
                Ok(Some((interface, di))) => {
                    let client_server_protocol = ProtocolInfo {
                        protocol_id: wire_weaver_client_common::ww::PROTOCOL_GID,
                        major_version: 0,
                        minor_version: 1,
                    };
                    let max_packet_size = match di.speed() {
                        Some(speed) => match speed {
                            Speed::Low => 8,
                            Speed::Full => 64,
                            Speed::High | Speed::Super | Speed::SuperPlus => max_hs_usb_packet_size,
                            _ => 64,
                        },
                        None => 64,
                    };
                    // TODO: tweak to actually hit next USB packet
                    if max_packet_size <= 64 {
                        state.common.packet_accumulation_time = Duration::from_micros(900);
                    } else {
                        state.common.packet_accumulation_time = Duration::from_micros(100);
                    }
                    debug!("max_packet_size: {}", max_packet_size);
                    link = Some(WireWeaverUsbLink::new(
                        client_server_protocol,
                        state.user_protocol,
                        Sink::new(interface.clone()),
                        &mut tx_buf[..max_packet_size],
                        Source::new(interface),
                        &mut rx_buf[..max_packet_size],
                    ));
                    state.device_info = Some(di);
                }
                Ok(None) => {
                    // OnError::KeepRetrying
                    continue;
                }
                Err(_) => {
                    // timeout expired or OnError::Immediate
                    break;
                }
            },
        }
    }
    state.common.cancel_all_streams();
    state.common.cancel_all_requests();
    state.conn_state.write().await.worker_running = false;
    debug!("usb worker exited");
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    state: &mut State,
) -> Result<Option<(Interface, DeviceInfo)>, ()> {
    loop {
        let Some(cmd) = cmd_rx.recv().await else {
            // all senders have been dropped
            debug!("usb worker exiting, because all command senders were dropped");
            return Err(());
        };
        match cmd {
            Command::Connect {
                filter,
                on_error,
                connected_tx,
                user_protocol_version,
            } => {
                // TODO: process commands with timeout expired before connected?
                let (interface, di) = match crate::connection::connect(filter, on_error).await {
                    Ok(i_di) => i_di,
                    Err(e) => {
                        state.conn_state.write().await.state = ConnectionState::Error {
                            error_string: format!("{:?}", e),
                        };
                        // TODO: drop requests if any
                        return if on_error == OnError::KeepRetrying {
                            Ok(None)
                        } else {
                            if let Some(tx) = connected_tx {
                                _ = tx.send(Err(e));
                            }
                            Err(())
                        };
                    }
                };
                state
                    .common
                    .on_connect(on_error, connected_tx, user_protocol_version); // TODO: use user protocol version
                return Ok(Some((interface, di)));
            }
            Command::Subscribe {
                path_kind,
                stream_data_tx,
            } => {
                if let PathKindOwned::Absolute { path } = path_kind {
                    state.common.stream_handlers.insert(path, stream_data_tx);
                }
                // TODO: register stream handler when using traits, absolute path will only be known when device replies
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
                return Err(());
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

#[derive(Debug, PartialEq)]
enum EventLoopResult {
    DisconnectKeepStreams,
    Disconnect,
    Exit,
}

async fn process_commands_and_endpoints(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
) -> Result<EventLoopResult, Error> {
    link.send_get_device_info()
        .await
        .map_err(|e| Error::Transport(format!("{:?}", e)))?;
    let mut scratch = [0u8; 512];
    let mut link_setup_retries = 5;
    loop {
        let duration = if state.common.link_setup_done {
            if let Some(instant) = state.common.packet_started_instant {
                let dt_since_packet_start = Instant::now() - instant;
                state
                    .common
                    .packet_accumulation_time
                    .checked_sub(dt_since_packet_start)
                    .unwrap_or(Duration::from_secs(0))
            } else {
                Duration::from_millis(wire_weaver_usb_link::PING_INTERVAL_MS)
            }
        } else {
            // resend GetDeviceInfo, might not be needed as packets should not get silently lost (apart from the very first), but just in case
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.receive_message(&mut state.message_rx) => {
                match handle_message(message, link, state).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("usb event loop: all CanBus instances were dropped, exiting");
                    link.send_disconnect(DisconnectReason::RequestByUser).await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    return Ok(EventLoopResult::Exit);
                };
                match handle_command(cmd, link, state, &mut scratch).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                }
            }
            _ = timer => {
                if !state.common.link_setup_done {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        link.send_get_device_info().await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                        link_setup_retries -= 1;
                    } else {
                        error!("usb worker exiting, because link setup failed after several retries");
                        return Err(Error::LinkSetupTimeout);
                    }
                } else {
                    if let Some(last) = &state.common.last_ping_instant {
                        let dt = Instant::now() - *last;
                        if dt > Duration::from_secs(5) {
                            warn!("No ping from device for 5 seconds, exiting");
                            return Ok(EventLoopResult::Disconnect);
                        }
                    }
                    if let Some(instant) = state.common.packet_started_instant {
                        trace!("Sending accumulated packet {}us", (Instant::now() - instant).as_micros());
                        state.common.packet_started_instant = None;
                        link.force_send().await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    } else {
                        trace!("Sending ping");
                        link.send_ping().await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    }
                }
                state.common.prune_timed_out_requests();
                // TODO: send accumulated message or Ping
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
    message: Result<MessageKind, LinkError<TransferError, TransferError>>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
) -> Result<EventLoopSpinResult, Error> {
    match message {
        Ok(MessageKind::Data(len)) => {
            if len == 0 {
                warn!("got empty event data, ignoring");
                return Ok(EventLoopSpinResult::Continue);
            }
            let packet = &state.message_rx[..len];
            let mut rd = BufReader::new(packet);
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
                    EventKind::StreamData { path, data } => {
                        let path = path.iter().map(|p| p.unwrap()).collect::<Vec<_>>();
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
            // tx_events.send(Event::Received(packet.to_vec())).await.unwrap();
        }
        Ok(MessageKind::Disconnect(reason)) => {
            info!("Received Disconnect({reason:?}) from remote device, exiting");
            return Ok(EventLoopSpinResult::DisconnectFromDevice);
        }
        Ok(MessageKind::Ping) => {
            trace!("Ping");
            state.common.last_ping_instant = Some(Instant::now());
        }
        Ok(MessageKind::DeviceInfo {
            max_message_len,
            link_version,
            client_server_protocol,
            user_protocol,
        }) => {
            info!(
                "Received DeviceInfo: max_message_len: {}, link_version: {}, client_server: {:?}, user_protocol: {:?}",
                max_message_len, link_version, client_server_protocol, user_protocol
            );
            // only one version is in use right now, so no need to choose between different client server versions or link versions
            link.send_link_setup(MAX_MESSAGE_SIZE as u32)
                .await
                .map_err(|e| Error::Transport(UsbError::Link(e).into()))?;
        }
        Ok(MessageKind::LinkSetupResult { versions_matches }) => {
            if !versions_matches {
                error!("device rejected LinkSetup, exiting");
                if let Some(tx) = state.common.connected_tx.take() {
                    _ = tx.send(Err(Error::IncompatibleDeviceProtocol));
                }
                return Err(Error::IncompatibleDeviceProtocol);
            }
            info!("LinkSetup complete");
            state.max_protocol_mismatched_messages = 10;
            if let Some(di) = &state.device_info {
                state.conn_state.write().await.state = ConnectionState::Connected {
                    device_info: di.clone(),
                };
            }
            if let Some(tx) = state.common.connected_tx.take() {
                _ = tx.send(Ok(()));
            }
            state.common.link_setup_done = true;
        }
        Err(e @ LinkError::ProtocolsVersionMismatch) => {
            if state.max_protocol_mismatched_messages > 0 {
                warn!(
                    "Protocols version mismatch, probably old message from previous session or missed packet?"
                );
                state.max_protocol_mismatched_messages -= 1;
            } else {
                return Err(Error::Transport(UsbError::Link(e).into()));
            }
        }
        Err(e @ LinkError::InternalBufOverflow | e @ LinkError::MessageTooBig) => {
            warn!("handle_message: ignoring {e:?}");
        }
        Err(e) => return Err(Error::Transport(UsbError::Link(e).into())),
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn handle_command(
    cmd: Command,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<EventLoopSpinResult, Error> {
    match cmd {
        Command::Connect { .. } => {
            warn!("Ignoring Connect while already connected");
        }
        Command::DisconnectKeepStreams { disconnected_tx } => {
            info!("Disconnecting on user request (but keeping streams ready for re-use)");
            link.send_disconnect(DisconnectReason::RequestByUser)
                .await
                .map_err(|e| Error::Transport(format!("{:?}", e)))?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectKeepStreams);
        }
        Command::DisconnectAndExit { disconnected_tx } => {
            info!("Disconnecting and stopping USB event loop on user request");
            link.send_disconnect(DisconnectReason::RequestByUser)
                .await
                .map_err(|e| Error::Transport(format!("{:?}", e)))?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectAndExit);
        }
        Command::SendCall {
            args_bytes,
            path_kind,
            timeout,
            done_tx,
        } => {
            trace!("sending call to {path_kind:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = Request {
                seq,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Call {
                    args: RefVec::new_bytes(&args_bytes),
                },
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::SendWrite {
            value_bytes,
            path_kind,
            timeout,
            done_tx,
        } => {
            trace!("sending write to {path_kind:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = Request {
                seq,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Write {
                    data: RefVec::new_bytes(&value_bytes),
                },
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::SendRead {
            path_kind,
            timeout,
            done_tx,
        } => {
            trace!("sending read to {path_kind:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = Request {
                seq,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Read,
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::Subscribe {
            path_kind,
            stream_data_tx,
        } => {
            if let PathKindOwned::Absolute { path } = path_kind {
                state.common.stream_handlers.insert(path, stream_data_tx);
            }
            // TODO: is it correct to ignore non absolute paths here?
        }
    }
    Ok(EventLoopSpinResult::Continue)
}

// TODO: forward error back to caller instead of exiting from event loop
async fn serialize_request_send(
    request: Request<'_>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<(), Error> {
    let mut wr = BufWriter::new(scratch);
    request.ser_shrink_wrap(&mut wr)?;
    let request_bytes = wr.finish_and_take()?.to_vec();

    link.send_message(&request_bytes)
        .await
        .map_err(|e| Error::Transport(format!("{:?}", e)))?; // TODO: Is there a need to guard with timeout here, can device get stuck and not receive?
    if link.is_tx_queue_empty() {
        state.common.packet_started_instant = None;
    } else {
        state.common.packet_started_instant = Some(Instant::now());
    }
    Ok(())
}
