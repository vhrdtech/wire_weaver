use crate::ww_nusb::{Sink, Source};
use crate::{UsbError, MAX_MESSAGE_SIZE};
use nusb::transfer::TransferError;
use nusb::{DeviceInfo, Interface, Speed};
use std::fmt::Debug;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client_common::rx_dispatcher::DispatcherMessage;
use wire_weaver_client_common::{
    event_loop_state::CommonState, Command, DeviceInfoBundle, Error, OnError, TestProgress,
};
use wire_weaver_usb_link::{
    DisconnectReason, Error as LinkError, MessageKind, PacketSink, PacketSource, WireWeaverUsbLink,
    PING_INTERVAL_MS,
};

struct State {
    common: CommonState,
    message_rx: [u8; MAX_MESSAGE_SIZE],
    device_info: Option<DeviceInfo>,
    max_protocol_mismatched_messages: u32,
    irq_packet_size: usize,
}

impl State {
    fn new() -> Self {
        State {
            common: CommonState::default(),
            message_rx: [0u8; MAX_MESSAGE_SIZE],
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
    mut to_dispatcher: mpsc::UnboundedSender<DispatcherMessage>,
) {
    let mut state = State::new();

    let mut tx_buf = [0u8; 1024];
    let mut rx_buf = [0u8; 1024];
    let mut link = None;

    loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(&mut cmd_rx, l, &mut state, &mut to_dispatcher)
                    .await
                {
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
                    state.on_disconnect();
                    link = None;
                    continue;
                }
            }
            None => match wait_for_connection_and_queue_commands(&mut cmd_rx, &mut state).await {
                Ok(Some((interface, di, user_protocol))) => {
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
                    let sink = Sink::new(&interface, max_irq_packet_size).unwrap();
                    let source = Source::new(&interface, max_irq_packet_size).unwrap();
                    cfg_if::cfg_if! {
                        if #[cfg(feature = "usb-tracing")] {
                            use iceoryx2::prelude::*;
                            let node = NodeBuilder::new().create::<ipc_threadsafe::Service>().unwrap();
                            let name = format!("WireWeaver/UsbTrace/{}-{:?}", di.bus_id(), di.port_chain());
                            let service = node.service_builder(&ServiceName::new(format!("{name}/tx").as_str()).unwrap())
                                .publish_subscribe::<crate::tracing::UsbPacket>().open_or_create().unwrap();
                            let publisher = service.publisher_builder().create().unwrap();
                            let sink = crate::tracing::SinkTrace::new(publisher, sink);
                            let service = node.service_builder(&ServiceName::new(format!("{name}/rx").as_str()).unwrap())
                                .publish_subscribe::<crate::tracing::UsbPacket>().open_or_create().unwrap();
                            let publisher = service.publisher_builder().create().unwrap();
                            let source = crate::tracing::SourceTrace::new(publisher, source);
                        }
                    }
                    link = Some(WireWeaverUsbLink::new(
                        user_protocol,
                        sink,
                        &mut tx_buf[..max_irq_packet_size],
                        source,
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
    debug!("usb worker exited");
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    state: &mut State,
) -> Result<Option<(Interface, DeviceInfo, FullVersionOwned)>, ()> {
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
                client_version,
            } => {
                let (interface, di) = match crate::connection::connect(filter, on_error).await {
                    Ok(i_di) => i_di,
                    Err(e) => {
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
                    .on_connect(on_error, connected_tx, client_version.clone());
                return Ok(Some((interface, di, client_version)));
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
                return Err(());
            }
            Command::SendMessage { .. } => {
                warn!("ignoring send message while disconnected");
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

async fn process_commands_and_endpoints<T, R>(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    link: &mut WireWeaverUsbLink<'_, T, R>,
    state: &mut State,
    to_dispatcher: &mut mpsc::UnboundedSender<DispatcherMessage>,
) -> Result<EventLoopResult, Error>
where
    T: PacketSink<Error = TransferError>,
    R: PacketSource<Error = TransferError>,
{
    link.send_get_device_info()
        .await
        .map_err(|e| Error::Transport(format!("{:?}", e)))?;
    let mut scratch = [0u8; MAX_MESSAGE_SIZE];
    let mut link_setup_retries = 5;
    let ping_period = Duration::from_millis(PING_INTERVAL_MS);
    const TIMER_IGNORE_PERIOD: Duration = Duration::from_micros(10);
    let mut next_tx_ping_instant = Instant::now() + ping_period;
    loop {
        let duration = if state.common.link_up {
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
            till_force_send
                .map(|f| f.min(till_ping))
                .unwrap_or(till_ping)
        } else {
            // resend GetDeviceInfo, might not be needed as packets should not get silently lost (apart from the very first), but just in case
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.receive_message(&mut state.message_rx) => {
                match handle_message(message, link, state, to_dispatcher).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("all cmd tx instances were dropped, exiting");
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
                if !state.common.link_up {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        let r = link.send_get_device_info().await;
                        if let Err(wire_weaver_usb_link::Error::SinkError(TransferError::Unknown(code))) = r
                            && code == crate::ww_nusb::ERR_WRITE_PACKET_TIMEOUT
                            && let Some(tx) = state.common.connected_tx.take()
                        {
                            _ = tx.send(Err(Error::Other("Device is not accepting USB transfers, it might be in an endless loop or in HardFault".into())));
                        }
                        r.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                        link_setup_retries -= 1;
                    } else {
                        error!("exiting, because link setup failed after several retries");
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

async fn handle_message<T, R>(
    message: Result<MessageKind, LinkError<TransferError, TransferError>>,
    link: &mut WireWeaverUsbLink<'_, T, R>,
    state: &mut State,
    to_dispatcher: &mut mpsc::UnboundedSender<DispatcherMessage>,
) -> Result<EventLoopSpinResult, Error>
where
    T: PacketSink<Error = TransferError>,
    R: PacketSource<Error = TransferError>,
{
    match message {
        Ok(MessageKind::Data(len)) => {
            state.common.last_rx_ping_instant = Some(Instant::now());
            if len == 0 {
                warn!("got empty event data, ignoring");
                return Ok(EventLoopSpinResult::Continue);
            }
            let message = &state.message_rx[..len];
            state.common.trace_event(message);
            to_dispatcher
                .send(DispatcherMessage::MessageBytes(message.to_vec()))
                .map_err(|_| Error::Other("rx dispatcher is not running".into()))?;
        }
        Ok(MessageKind::Disconnect(reason)) => {
            state
                .common
                .trace_disconnect(format!("remote: {reason:?}").as_str(), false);
            return if !state.common.link_up && reason != DisconnectReason::IncompatibleVersion {
                warn!(
                    "Received Disconnect({reason:?}) from remote device, ignoring, must be from old session"
                );
                Ok(EventLoopSpinResult::Continue)
            } else {
                if reason == DisconnectReason::IncompatibleVersion
                    || reason == DisconnectReason::ApplicationCrash
                {
                    error!("Received Disconnect({reason:?}), exiting");
                } else {
                    info!("Received Disconnect({reason:?}) from remote device, exiting");
                }
                to_dispatcher
                    .send(DispatcherMessage::Disconnected)
                    .map_err(|_| Error::Other("rx dispatcher is not running".into()))?;
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
            api_model_version,
            user_api_version,
            user_api_signature,
        }) => {
            let connected_device_info = DeviceInfoBundle {
                link_version: FullVersionOwned::new(
                    format!("G{}", link_version.global_type_id.0),
                    VersionOwned::new(
                        link_version.major.0,
                        link_version.minor.0,
                        link_version.patch.0,
                    ),
                ),
                max_message_size: max_message_len as usize,
                api_model_version: FullVersionOwned::new(
                    format!("G{}", api_model_version.global_type_id.0),
                    VersionOwned::new(
                        api_model_version.major.0,
                        api_model_version.minor.0,
                        api_model_version.patch.0,
                    ),
                ),
                user_api_version,
                user_api_signature: user_api_signature.into(),
            };
            info!("Connected device: {connected_device_info:?}");
            if let Some(client_version) = state.common.client_version.as_ref()
                && !client_version.crate_id.is_empty() // dyn connection without code generated API, using introspect data from a device only
                && !client_version.is_protocol_compatible(&connected_device_info.user_api_version)
            {
                return Err(Error::IncompatibleDeviceProtocol);
            }
            state.common.device_info = Some(connected_device_info);
            // only one version is in use right now, so no need to choose between different link versions
            link.send_link_setup(MAX_MESSAGE_SIZE as u32)
                .await
                .map_err(|e| Error::Transport(UsbError::Link(e).into()))?;
        }
        Ok(MessageKind::LinkUp) => {
            info!("LinkSetup complete");
            state.max_protocol_mismatched_messages = 10;
            to_dispatcher
                .send(DispatcherMessage::Connected)
                .map_err(|_| Error::Other("rx dispatcher is not running".into()))?;
            if let Some(tx) = state.common.connected_tx.take() {
                _ = tx.send(Ok(state
                    .common
                    .device_info
                    .clone()
                    .unwrap_or(DeviceInfoBundle::empty())));
            }
            state.common.on_link_up();
        }
        Ok(MessageKind::Loopback { .. }) => {} // ignore when not testing
        Err(e @ LinkError::ProtocolsVersionMismatch) => {
            state.common.trace_error(format!("{e:?}"));
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
            state.common.trace_error(format!("{e:?}"));
            warn!("handle_message: ignoring {e:?}");
        }
        Err(e) => return Err(Error::Transport(UsbError::Link(e).into())),
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn handle_command<T, R>(
    cmd: Command,
    link: &mut WireWeaverUsbLink<'_, T, R>,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<EventLoopSpinResult, Error>
where
    T: PacketSink<Error = TransferError>,
    R: PacketSource<Error = TransferError>,
{
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
            state.common.trace_disconnect("client request", false);
            link.send_disconnect(DisconnectReason::RequestByUser)
                .await
                .map_err(|e| Error::Transport(format!("{:?}", e)))?;
            // wait for Disconnect op to be actually sent out
            // link.tx_mut().flush().await - does not seem to be working, submitted transfer still gets canceled in-flight
            tokio::time::sleep(Duration::from_millis(3)).await;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectAndExit);
        }
        Command::SendMessage { bytes } => {
            state.common.trace_request(&bytes);
            link.send_message(&bytes)
                .await
                .map_err(|e| Error::Transport(format!("{:?}", e)))?;
            if link.is_tx_queue_empty() {
                state.common.packet_started_instant = None;
            } else if state.common.packet_started_instant.is_none() {
                state.common.packet_started_instant = Some(Instant::now());
            }
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
