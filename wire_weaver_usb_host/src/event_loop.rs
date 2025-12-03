use crate::ww_nusb::{Sink, Source};
use crate::{ConnectionInfo, ConnectionState, MAX_MESSAGE_SIZE, UsbError};
use nusb::transfer::TransferError;
use nusb::{DeviceInfo, Interface, Speed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, trace, warn};
use wire_weaver::shrink_wrap::ref_vec::RefVec;
use wire_weaver::shrink_wrap::{BufReader, BufWriter, DeserializeShrinkWrap, SerializeShrinkWrap};
use wire_weaver::ww_version::FullVersion;
use wire_weaver_client_common::ww_client_server::PathKindOwned;
use wire_weaver_client_common::{
    Command, Error, OnError, StreamEvent, TestProgress,
    event_loop_state::CommonState,
    ww_client_server::{Event, EventKind, Request, RequestKind},
};
use wire_weaver_usb_link::{
    DisconnectReason, Error as LinkError, MessageKind, PING_INTERVAL_MS, WireWeaverUsbLink,
};

struct State {
    common: CommonState,
    message_rx: [u8; MAX_MESSAGE_SIZE],
    user_protocol: FullVersion<'static>,
    conn_state: Arc<RwLock<ConnectionInfo>>,
    device_info: Option<DeviceInfo>,
    max_protocol_mismatched_messages: u32,
    irq_packet_size: usize,
}

impl State {
    fn new(conn_state: Arc<RwLock<ConnectionInfo>>, user_protocol: FullVersion<'static>) -> Self {
        State {
            common: CommonState::default(),
            message_rx: [0u8; MAX_MESSAGE_SIZE],
            user_protocol,
            conn_state,
            device_info: None,
            max_protocol_mismatched_messages: 10,
            irq_packet_size: 0,
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
    user_protocol: FullVersion<'static>,
) {
    let mut state = State::new(conn_state, user_protocol);
    state.conn_state.write().await.worker_running = false;

    let mut tx_buf = [0u8; 1024];
    let mut rx_buf = [0u8; 1024];
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
                    let max_irq_packet_size = match di.speed() {
                        Some(speed) => match speed {
                            Speed::Low => 8,
                            Speed::Full => 64,
                            Speed::High | Speed::Super | Speed::SuperPlus => 1024,
                            _ => 64,
                        },
                        None => 64,
                    };
                    // TODO: HS IRQ max is 1024, but bulk max is 512
                    // TODO: tweak to actually hit next USB packet
                    if max_irq_packet_size <= 64 {
                        state.common.packet_accumulation_time = Duration::from_micros(900);
                    } else {
                        state.common.packet_accumulation_time = Duration::from_micros(100);
                    }
                    state.irq_packet_size = max_irq_packet_size;
                    debug!("max_packet_size: {}", max_irq_packet_size);
                    link = Some(WireWeaverUsbLink::new(
                        state.user_protocol.clone(),
                        Sink::new(&interface, max_irq_packet_size).unwrap(),
                        &mut tx_buf[..max_irq_packet_size],
                        Source::new(&interface, max_irq_packet_size).unwrap(),
                        &mut rx_buf[..max_irq_packet_size],
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
                    .on_connect(on_error, connected_tx, user_protocol_version);
                return Ok(Some((interface, di)));
            }
            Command::StreamOpen {
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
            Command::LoopbackTest { progress_tx, .. } => {
                _ = progress_tx.send(TestProgress::FatalError("Not connected".into()));
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
    let mut scratch = [0u8; MAX_MESSAGE_SIZE];
    let mut link_setup_retries = 5;
    let ping_period = Duration::from_millis(PING_INTERVAL_MS);
    const TIMER_IGNORE_PERIOD: Duration = Duration::from_micros(10);
    let mut next_tx_ping_instant = Instant::now() + ping_period;
    loop {
        let duration = if state.common.link_setup_done {
            let now = Instant::now();
            let till_force_send = if let Some(instant) = state.common.packet_started_instant {
                let dt_since_packet_start = now
                    .checked_duration_since(instant)
                    .unwrap_or(Duration::from_millis(0));
                let till_force_send = state
                    .common
                    .packet_accumulation_time
                    .checked_sub(dt_since_packet_start)
                    .unwrap_or(Duration::from_millis(0));
                if till_force_send < TIMER_IGNORE_PERIOD {
                    state.common.packet_started_instant = None;
                    debug!("sending accumulated packet (timer ignore)");
                    link.force_send()
                        .await
                        .map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    next_tx_ping_instant = now + ping_period;
                    None
                } else {
                    Some(till_force_send)
                }
            } else {
                None
            };
            let till_ping = next_tx_ping_instant
                .checked_duration_since(now)
                .unwrap_or(Duration::from_millis(0));
            let till_ping = if till_ping < TIMER_IGNORE_PERIOD {
                debug!("sending ping (timer ignore)");
                link.send_ping()
                    .await
                    .map_err(|e| Error::Transport(format!("{:?}", e)))?;
                next_tx_ping_instant = now + ping_period;
                ping_period
            } else {
                till_ping
            };
            let till_min = till_force_send
                .map(|f| f.min(till_ping))
                .unwrap_or(till_ping);
            till_min
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
                    if let Some(last) = &state.common.last_rx_ping_instant {
                        let dt = Instant::now() - *last;
                        if dt > Duration::from_secs(10) {
                            warn!("no ping from device for 10 seconds, exiting");
                            return Ok(EventLoopResult::Disconnect);
                        }
                    }
                    if let Some(instant) = state.common.packet_started_instant {
                        trace!("sending accumulated packet {}us", (Instant::now() - instant).as_micros());
                        state.common.packet_started_instant = None;
                        link.force_send().await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    } else {
                        trace!("sending ping");
                        link.send_ping().await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    }
                    next_tx_ping_instant = Instant::now() + ping_period;
                }
                state.common.prune_timed_out_requests();
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
            state.common.last_rx_ping_instant = Some(Instant::now());
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
                            let data = data.as_slice().to_vec();
                            should_drop_handler = tx.send(StreamEvent::Data(data)).is_err();
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
            return if !state.common.link_setup_done
                && reason != DisconnectReason::IncompatibleVersion
            {
                warn!(
                    "Received Disconnect({reason:?}) from remote device, ignoring, must be from old session"
                );
                Ok(EventLoopSpinResult::Continue)
            } else {
                info!("Received Disconnect({reason:?}) from remote device, exiting");
                Ok(EventLoopSpinResult::DisconnectFromDevice)
            };
        }
        Ok(MessageKind::Ping) => {
            trace!("Ping");
            state.common.last_rx_ping_instant = Some(Instant::now());
        }
        Ok(MessageKind::DeviceInfo {
            max_message_len,
            link_version,
        }) => {
            info!(
                "Received DeviceInfo: max_message_len: {}, link_version: {:?}, remote_protocol: {:?}",
                max_message_len,
                link_version,
                link.remote_protocol()
            );
            // only one version is in use right now, so no need to choose between different link versions
            link.send_link_setup(MAX_MESSAGE_SIZE as u32)
                .await
                .map_err(|e| Error::Transport(UsbError::Link(e).into()))?;
        }
        Ok(MessageKind::LinkUp) => {
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
        Ok(MessageKind::Loopback { .. }) => {} // ignore when not testing
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
            // wait for Disconnect op to be actually sent out
            // link.tx_mut().flush().await
            tokio::time::sleep(Duration::from_millis(3)).await;
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
            // wait for Disconnect op to be actually sent out
            // link.tx_mut().flush().await - does not seem to be working, submitted transfer still get cancelled in-flight
            tokio::time::sleep(Duration::from_millis(3)).await;
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
            let seq = if done_tx.is_some() {
                state.common.register_prune_next_seq(timeout, done_tx)
            } else {
                0
            };
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
            let seq = if done_tx.is_some() {
                state.common.register_prune_next_seq(timeout, done_tx)
            } else {
                0
            };
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
        Command::StreamOpen {
            path_kind,
            stream_data_tx,
        } => {
            if let PathKindOwned::Absolute { path } = path_kind {
                state.common.stream_handlers.insert(path, stream_data_tx);
            }
            // TODO: is it correct to ignore non absolute paths here?
        }
        Command::LoopbackTest {
            test_duration,
            packet_size,
            progress_tx,
        } => {
            let packet_size = if let Some(requested) = packet_size {
                requested.min(state.irq_packet_size)
            } else {
                state.irq_packet_size
            };
            crate::loopback::loopback_test(test_duration, packet_size, progress_tx, link, scratch)
                .await;
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
    } else if state.common.packet_started_instant.is_none() {
        state.common.packet_started_instant = Some(Instant::now());
    }
    Ok(())
}
