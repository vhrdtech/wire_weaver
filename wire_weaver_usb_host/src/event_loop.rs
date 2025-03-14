use crate::ww::no_alloc_client::client_server_v0_1::{EventKind, RequestKind};
use crate::ww_nusb::{Sink, Source};
use crate::{
    Command, ConnectionInfo, ConnectionState, DEFAULT_REQUEST_TIMEOUT, Error, IRQ_MAX_PACKET_SIZE,
    MAX_MESSAGE_SIZE, OnError, SeqTy,
};
use nusb::transfer::TransferError;
use nusb::{DeviceInfo, Interface, Speed};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{debug, error, info, trace, warn};
use wire_weaver::shrink_wrap::vec::RefVec;
use wire_weaver::shrink_wrap::{BufReader, BufWriter, ElementSize, SerializeShrinkWrap};
use wire_weaver_usb_link::{Error as LinkError, MessageKind, ProtocolInfo, WireWeaverUsbLink};

type ResponseSender = oneshot::Sender<Result<Vec<u8>, Error>>;
type StreamUpdateSender = mpsc::UnboundedSender<Result<Vec<u8>, Error>>;

struct State {
    exit_on_error: bool,
    request_id: SeqTy,
    message_rx: [u8; MAX_MESSAGE_SIZE],
    user_protocol: ProtocolInfo,
    conn_state: Arc<RwLock<ConnectionInfo>>,
    connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
    device_info: Option<DeviceInfo>,
    max_protocol_mismatched_messages: u32,
    response_map: HashMap<SeqTy, (ResponseSender, Instant)>,
    stream_handlers: HashMap<Vec<u16>, StreamUpdateSender>,
    link_setup_done: bool,
    packet_started_instant: Option<Instant>,
}

impl State {
    fn new(conn_state: Arc<RwLock<ConnectionInfo>>, user_protocol: ProtocolInfo) -> Self {
        State {
            exit_on_error: true,
            request_id: 1,
            message_rx: [0u8; MAX_MESSAGE_SIZE],
            user_protocol,
            conn_state,
            connected_tx: None,
            device_info: None,
            max_protocol_mismatched_messages: 10,
            response_map: Default::default(),
            stream_handlers: Default::default(),
            link_setup_done: false,
            packet_started_instant: None,
        }
    }

    fn increment_request_id(&mut self) {
        let mut iterations_left = SeqTy::MAX as usize;
        while self.response_map.contains_key(&self.request_id) && iterations_left > 0 {
            self.request_id = self.request_id.wrapping_add(1);
            if self.request_id == 0 {
                self.request_id += 1;
            }
            iterations_left -= 1;
        }
    }

    fn register_prune_next_seq(
        &mut self,
        timeout: Option<Duration>,
        done_tx: Option<ResponseSender>,
    ) -> SeqTy {
        if let Some(done_tx) = done_tx {
            let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
            let prune_at = Instant::now() + timeout;
            self.response_map
                .insert(self.request_id, (done_tx, prune_at));
            let seq = self.request_id;
            self.increment_request_id();
            seq
        } else {
            0
        }
    }

    fn cancel_all_requests(&mut self) {
        for (_request_id, (tx, _)) in self.response_map.drain() {
            let _ = tx.send(Err(Error::Disconnected));
        }
    }

    fn cancel_all_streams(&mut self) {
        for (_path, tx) in self.stream_handlers.drain() {
            let _ = tx.send(Err(Error::Disconnected));
        }
    }

    fn on_disconnect(&mut self) {
        self.request_id = 1;
        self.connected_tx = None;
        self.device_info = None;
        self.max_protocol_mismatched_messages = 10;
        self.link_setup_done = false;
        self.packet_started_instant = None;
    }

    fn prune_timed_out_requests(&mut self) {
        let mut to_prune = vec![];
        let now = Instant::now();
        for (request_id, (_, prune_at)) in &self.response_map {
            if &now >= prune_at {
                to_prune.push(*request_id);
            }
        }
        for request_id in to_prune {
            if let Some((tx, _)) = self.response_map.remove(&request_id) {
                let _ = tx.send(Err(Error::Timeout));
            }
        }
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
            Some(link_ref) => {
                match process_commands_and_endpoints(&mut cmd_rx, link_ref, &mut state).await {
                    Ok(r) => info!("usb event loop (inner) exited with {:?}", r),
                    Err(e) => error!("usb event loop (inner) exited with {:?}", e),
                }
                if state.exit_on_error {
                    break;
                } else {
                    info!("will try to reconnect");
                    state.cancel_all_requests();
                    state.on_disconnect();
                    link = None;
                    continue;
                }
            }
            None => match wait_for_connection_and_queue_commands(&mut cmd_rx, &mut state).await {
                Ok(Some((interface, di))) => {
                    let client_server_protocol = ProtocolInfo {
                        protocol_id: crate::ww::no_alloc_client::client_server_v0_1::PROTOCOL_GID,
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
    state.cancel_all_streams();
    state.cancel_all_requests();
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
                on_error: timeout,
                connected_tx,
            } => {
                state.exit_on_error = timeout != OnError::KeepRetrying;
                state.request_id = 1;
                // TODO: process commands with timeout expired before connected?
                let (interface, di) = match crate::connection::connect(filter, timeout).await {
                    Ok(i_di) => i_di,
                    Err(e) => {
                        state.conn_state.write().await.state = ConnectionState::Error {
                            error_string: format!("{:?}", e),
                        };
                        // TODO: drop requests if any
                        return if timeout == OnError::KeepRetrying {
                            Ok(None)
                        } else {
                            if let Some(tx) = connected_tx {
                                _ = tx.send(Err(e.into()));
                            }
                            Err(())
                        };
                    }
                };
                state.connected_tx = connected_tx;
                return Ok(Some((interface, di)));
            }
            Command::Subscribe {
                path,
                stream_data_tx,
            } => {
                state.stream_handlers.insert(path, stream_data_tx);
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
                state.exit_on_error = true;
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

#[derive(Debug)]
enum EventLoopResult {
    DisconnectKeepStreams,
    DisconnectFromDevice,
    DisconnectAndExit,
}

async fn process_commands_and_endpoints(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
) -> Result<EventLoopResult, Error> {
    link.send_get_device_info().await?;
    let mut scratch = [0u8; 512];
    let mut link_setup_retries = 5;
    loop {
        let duration = if state.link_setup_done {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.receive_message(&mut state.message_rx) => {
                match handle_message(message, link, state).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::DisconnectFromDevice),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::DisconnectAndExit)
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("usb event loop: all CanBus instances were dropped, exiting");
                    link.send_disconnect().await?;
                    return Ok(EventLoopResult::DisconnectAndExit);
                };
                match handle_command(cmd, link, state, &mut scratch).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::DisconnectFromDevice),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::DisconnectAndExit)
                }
            }
            _ = timer => {
                if !state.link_setup_done {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        link.send_get_device_info().await?;
                        link_setup_retries -= 1;
                    } else {
                        error!("usb worker exiting, because link setup failed after several retries");
                        return Err(Error::LinkSetupTimeout);
                    }
                }
                state.prune_timed_out_requests();
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
            let packet = &state.message_rx[..len];
            let mut rd = BufReader::new(packet);
            let event: crate::ww::no_alloc_client::client_server_v0_1::Event =
                rd.read(ElementSize::Implied)?;
            trace!("event: {event:?}");
            match event.result {
                Ok(event_kind) => match event_kind {
                    EventKind::ReturnValue { data } | EventKind::ReadValue { data } => {
                        if let Some((done_tx, _)) = state.response_map.remove(&event.seq) {
                            let r = data
                                .byte_slice()
                                .map(|b| b.to_vec())
                                .map_err(|_| Error::ByteSliceReadFailed);
                            let _ = done_tx.send(r);
                        }
                    }
                    EventKind::Written => {
                        if let Some((done_tx, _)) = state.response_map.remove(&event.seq) {
                            let _ = done_tx.send(Ok(Vec::new()));
                        }
                    }
                    EventKind::StreamUpdate { path, data } => {
                        let path = path.iter().map(|p| p.unwrap().0).collect::<Vec<_>>();
                        let mut should_drop_handler = false;
                        if let Some(tx) = state.stream_handlers.get_mut(&path) {
                            let r = data
                                .byte_slice()
                                .map(|b| b.to_vec())
                                .map_err(|_| Error::ByteSliceReadFailed);
                            should_drop_handler = tx.send(r).is_err();
                        }
                        if should_drop_handler {
                            info!(
                                "Dropping stream handler with path: {path:?}, because rx end was dropped"
                            );
                            state.stream_handlers.remove(&path);
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    if let Some((done_tx, _)) = state.response_map.remove(&event.seq) {
                        let _ = done_tx.send(Err(Error::RemoteError(e)));
                    }
                }
            }
            // tx_events.send(Event::Received(packet.to_vec())).await.unwrap();
        }
        Ok(MessageKind::Disconnect) => {
            info!("Received Disconnect from remote device, exiting");
            return Ok(EventLoopSpinResult::DisconnectFromDevice);
        }
        Ok(MessageKind::Ping) => {
            trace!("Ping");
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
            link.send_link_setup(MAX_MESSAGE_SIZE as u32).await?;
        }
        Ok(MessageKind::LinkSetupResult { versions_matches }) => {
            if !versions_matches {
                error!("device rejected LinkSetup, exiting");
                if let Some(tx) = state.connected_tx.take() {
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
            if let Some(tx) = state.connected_tx.take() {
                _ = tx.send(Ok(()));
            }
            state.link_setup_done = true;
        }
        Err(e @ LinkError::ProtocolsVersionMismatch) => {
            if state.max_protocol_mismatched_messages > 0 {
                warn!(
                    "Protocols version mismatch, probably old message from previous session or missed packet?"
                );
                state.max_protocol_mismatched_messages -= 1;
            } else {
                return Err(e.into());
            }
        }
        Err(e @ LinkError::InternalBufOverflow | e @ LinkError::MessageTooBig) => {
            warn!("handle_message: ignoring {e:?}");
        }
        Err(e) => return Err(e.into()),
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
            info!("Disconnecting on user request");
            link.send_disconnect().await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectKeepStreams);
        }
        Command::DisconnectAndExit { disconnected_tx } => {
            info!("Disconnecting and stopping USB event loop on user request");
            link.send_disconnect().await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            state.exit_on_error = true;
            return Ok(EventLoopSpinResult::DisconnectAndExit);
        }
        Command::SendCall {
            args_bytes,
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending call to {path:?}");
            let seq = state.register_prune_next_seq(timeout, done_tx);
            let request = crate::ww::no_alloc_client::client_server_v0_1::Request {
                seq,
                path: RefVec::Slice {
                    slice: &path,
                    element_size: ElementSize::UnsizedSelfDescribing,
                },
                kind: RequestKind::Call {
                    args: RefVec::new_byte_slice(&args_bytes),
                },
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::SendWrite {
            value_bytes,
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending write to {path:?}");
            let seq = state.register_prune_next_seq(timeout, done_tx);
            let request = crate::ww::no_alloc_client::client_server_v0_1::Request {
                seq,
                path: RefVec::Slice {
                    slice: &path,
                    element_size: ElementSize::UnsizedSelfDescribing,
                },
                kind: RequestKind::Write {
                    data: RefVec::new_byte_slice(&value_bytes),
                },
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::SendRead {
            path,
            timeout,
            done_tx,
        } => {
            trace!("sending read to {path:?}");
            let seq = state.register_prune_next_seq(timeout, done_tx);
            let request = crate::ww::no_alloc_client::client_server_v0_1::Request {
                seq,
                path: RefVec::Slice {
                    slice: &path,
                    element_size: ElementSize::UnsizedSelfDescribing,
                },
                kind: RequestKind::Read,
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::Subscribe {
            path,
            stream_data_tx,
        } => {
            state.stream_handlers.insert(path, stream_data_tx);
        }
    }
    Ok(EventLoopSpinResult::Continue)
}

// TODO: forward error back to caller instead of exiting from event loop
async fn serialize_request_send(
    request: crate::ww::no_alloc_client::client_server_v0_1::Request<'_>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<(), Error> {
    let mut wr = BufWriter::new(scratch);
    request.ser_shrink_wrap(&mut wr)?;
    let request_bytes = wr.finish_and_take()?.to_vec();

    link.send_message(&request_bytes).await?; // TODO: Is there a need to guard with timeout here, can device get stuck and not receive?
    if link.is_tx_queue_empty() {
        state.packet_started_instant = None;
    } else {
        state.packet_started_instant = Some(Instant::now());
    }
    link.force_send().await?; // TODO: force send on timer
    Ok(())
}
