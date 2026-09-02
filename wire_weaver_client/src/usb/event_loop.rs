use crate::Error;
use crate::device_info::{ConnectionInfo, DeviceApiInfo};
use crate::event_loop::command::{Command, EventLoopExitReason, EventLoopResidual, TestProgress};
use crate::event_loop::event_loop_state::CommonState;
use crate::event_loop::rx_dispatcher::{DispatcherCommand, DispatcherMessage, RxDispatcher};
use crate::usb::loopback::loopback_test;

use super::ww_nusb::{Sink, Source};
use super::{MAX_MESSAGE_SIZE, UsbError};
use anyhow::anyhow;
use either::Either;
use nusb::DeviceInfo;
use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, trace, warn};
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
// use wire_weaver_client::EventLoopExitReason;
// use wire_weaver_client::rx_dispatcher::{DispatcherCommand, DispatcherMessage, RxDispatcher};
// use wire_weaver_client::{
//     Command, ConnectionInfo, DeviceApiInfo, Error, EventLoopResidual, TestProgress,
//     event_loop_state::CommonState,
// };
use wire_weaver_usb_link::{
    DisconnectReason, Error as LinkError, MessageKind, PING_INTERVAL_MS, PacketSink, PacketSource,
    WireWeaverUsbLink,
};

struct State {
    common: CommonState,
    message_rx: [u8; MAX_MESSAGE_SIZE],
    device_info: Option<DeviceInfo>,
    max_protocol_mismatched_messages: u32,
    max_packet_size: usize,
}

impl State {
    fn new() -> Self {
        State {
            common: CommonState::default(),
            message_rx: [0u8; MAX_MESSAGE_SIZE],
            device_info: None,
            max_protocol_mismatched_messages: 10,
            max_packet_size: 0,
        }
    }

    // fn on_disconnect(&mut self) {
    //     self.common.on_disconnect();
    //     self.device_info = None;
    //     self.max_protocol_mismatched_messages = 10;
    // }
}

pub async fn usb_worker(mut cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    let mut state = State::new();
    let mut rx_dispatcher = RxDispatcher::default();

    let mut tx_buf = [0u8; 1024];
    let mut rx_buf = [0u8; 1024];
    let mut link = None;
    let mut _usb_device = None;

    let mut exited_tx = None;
    let mut connected_tx = None;

    let result = loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(&mut cmd_rx, l, &mut state, &mut rx_dispatcher)
                    .await
                {
                    Ok(r) => {
                        debug!("exiting with: {:?}", r);
                        break Ok(r);
                    }
                    Err(e) => {
                        error!("exiting with: {:?}", e);
                        break Err(e);
                    }
                }
            }
            None => match wait_for_connection_and_queue_commands(&mut cmd_rx, &mut state).await {
                Ok(Either::Left(wait_ok)) => {
                    state.max_packet_size = wait_ok.dev.max_packet_size;
                    debug!("max_packet_size: {}", wait_ok.dev.max_packet_size);
                    let is_bulk = wait_ok.dev.transfer_type == TransferType::Bulk;
                    let sink =
                        Sink::new(&wait_ok.dev.interface, wait_ok.dev.max_packet_size, is_bulk)
                            .unwrap();
                    let source =
                        Source::new(&wait_ok.dev.interface, wait_ok.dev.max_packet_size, is_bulk)
                            .unwrap();
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
                    link = Some(WireWeaverUsbLink::new_host(
                        *wait_ok.client_version,
                        sink,
                        &mut tx_buf[..wait_ok.dev.max_packet_size],
                        source,
                        &mut rx_buf[..wait_ok.dev.max_packet_size],
                    ));
                    state.device_info = Some(*wait_ok.di);
                    exited_tx = wait_ok.exited_tx;
                    _usb_device = Some(wait_ok.dev.device);
                }
                Ok(Either::Right(reason)) => {
                    break Ok(reason);
                }
                Err(e) => {
                    exited_tx = e.exited_tx;
                    connected_tx = e.connected_tx;
                    break Err(e.error);
                }
            },
        }
    };

    if let Some(tx) = exited_tx {
        _ = tx.send(EventLoopResidual {
            cmd_rx,
            connected_tx,
            result,
        });
    }
    debug!("usb worker exited");
}

struct WaitOk {
    di: Box<DeviceInfo>,
    dev: super::connect::ConnectOk,
    client_version: Box<FullVersionOwned>,
    // connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    exited_tx: Option<oneshot::Sender<EventLoopResidual>>,
}

struct WaitErr {
    connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    exited_tx: Option<oneshot::Sender<EventLoopResidual>>,
    error: anyhow::Error,
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::Receiver<Command>,
    state: &mut State,
) -> Result<Either<WaitOk, EventLoopExitReason>, WaitErr> {
    loop {
        let Some(cmd) = cmd_rx.recv().await else {
            // all senders have been dropped
            debug!("usb worker exiting, because all command senders were dropped");
            return Err(WaitErr {
                connected_tx: None,
                exited_tx: None,
                error: anyhow!("all command senders dropped"),
            });
        };
        match cmd {
            Command::Connect {
                handle,
                client_version,
                connected_tx,
                failed_tx,
            } => {
                let Ok(di) = handle.downcast::<DeviceInfo>() else {
                    return Err(WaitErr {
                        connected_tx,
                        exited_tx: failed_tx,
                        error: anyhow!(""),
                    });
                };
                return match super::connect::connect(&di) {
                    Ok(dev) => {
                        state
                            .common
                            .on_connect(connected_tx, *client_version.clone());
                        Ok(Either::Left(WaitOk {
                            di,
                            dev,
                            client_version,
                            exited_tx: failed_tx,
                        }))
                    }
                    Err(e) => Err(WaitErr {
                        connected_tx,
                        exited_tx: failed_tx,
                        error: e,
                    }),
                };
            }
            Command::RegisterTracer { trace_event_tx } => {
                state.common.tracers.push(trace_event_tx);
            }
            Command::DisconnectKeepStreams {
                disconnected_tx, ..
            } => {
                if let Some(tx) = disconnected_tx {
                    let _ = tx.send(());
                }
                return Ok(Either::Right(
                    EventLoopExitReason::DisconnectKeepStreamsCommand,
                ));
            }
            Command::DisconnectAndExit {
                disconnected_tx, ..
            } => {
                if let Some(tx) = disconnected_tx {
                    let _ = tx.send(());
                }
                return Ok(Either::Right(EventLoopExitReason::DisconnectCommand));
            }
            Command::SendMessage { .. } => {
                warn!("ignoring send message while disconnected");
            }
            Command::OnStreamEvent { .. } => {
                // TODO: do not ignore OnStreamEvent when disconnected
                warn!("ignoring on stream event while disconnected for now");
            }
            Command::LoopbackTest { progress_tx, .. } => {
                _ = progress_tx.send(TestProgress::FatalError("Not connected".into()));
            }
        }
    }
}

async fn process_commands_and_endpoints<T, R>(
    cmd_rx: &mut mpsc::Receiver<Command>,
    link: &mut WireWeaverUsbLink<'_, T, R>,
    state: &mut State,
    rx_dispatcher: &mut RxDispatcher,
) -> Result<EventLoopExitReason, anyhow::Error>
where
    T: PacketSink<Error = TransferError>,
    R: PacketSource<Error = TransferError>,
{
    link.send_get_device_info()
        .await
        .map_err(|e| anyhow!("{e:?}"))?;
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
        let next_timeout = rx_dispatcher.prune_next_timeout();
        let timeout_timer = tokio::time::sleep(next_timeout);
        let ping_timer = tokio::time::sleep(duration);
        tokio::select! {
            message = link.receive_message(&mut state.message_rx) => {
                match handle_message(message, link, state, rx_dispatcher).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopExitReason::DisconnectKeepStreamsCommand),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopExitReason::DisconnectFromDevice),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopExitReason::DisconnectCommand)
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("all cmd tx instances were dropped, exiting");
                    link.send_disconnect(DisconnectReason::RequestByUser).await.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                    return Ok(EventLoopExitReason::CommanderDropped);
                };
                match handle_command(cmd, link, state, rx_dispatcher, &mut scratch).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopExitReason::DisconnectKeepStreamsCommand),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopExitReason::DisconnectFromDevice),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopExitReason::DisconnectCommand)
                }
            }
            _ = ping_timer => {
                if !state.common.link_up {
                    if link_setup_retries > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        let r = link.send_get_device_info().await;
                        if let Err(wire_weaver_usb_link::Error::SinkError(TransferError::Unknown(code))) = r
                            && code == super::ww_nusb::ERR_WRITE_PACKET_TIMEOUT
                            && let Some(tx) = state.common.connected_tx.take()
                        {
                            _ = tx.send(ConnectionInfo::err(anyhow!("Device is not accepting USB transfers, it might be in an endless loop or in HardFault")));
                        }
                        r.map_err(|e| Error::Transport(format!("{:?}", e)))?;
                        link_setup_retries -= 1;
                    } else {
                        error!("exiting, because link setup failed after several retries");
                        return Err(anyhow!(Error::LinkSetupTimeout));
                    }
                } else {
                    if let Some(last) = &state.common.last_rx_ping_instant {
                        let dt = Instant::now() - *last;
                        if dt > Duration::from_secs(10) {
                            warn!("no ping from device for 10 seconds, exiting");
                            return Err(anyhow!(Error::NoPingFromDevice));
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
            _ = timeout_timer => {
                rx_dispatcher.prune_next_timeout();
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
    rx_dispatcher: &mut RxDispatcher,
) -> Result<EventLoopSpinResult, Error>
where
    T: PacketSink<Error = TransferError>,
    R: PacketSource<Error = TransferError>,
{
    trace!("link: {message:?}");
    match message {
        Ok(MessageKind::Data(len)) => {
            state.common.last_rx_ping_instant = Some(Instant::now());
            if len == 0 {
                warn!("got empty event data, ignoring");
                return Ok(EventLoopSpinResult::Continue);
            }
            let message = &state.message_rx[..len];
            state.common.trace_event(message);
            rx_dispatcher.handle_msg(DispatcherMessage::MessageBytes(message));
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
                    || reason == DisconnectReason::CommanderDropped
                {
                    error!("Received Disconnect({reason:?}), exiting");
                } else {
                    info!("Received Disconnect({reason:?}) from remote device, exiting");
                }
                rx_dispatcher.handle_msg(DispatcherMessage::Disconnected);
                Ok(EventLoopSpinResult::DisconnectFromDevice)
            };
        }
        Ok(MessageKind::Ping) => {
            state.common.last_rx_ping_instant = Some(Instant::now());
        }
        Ok(MessageKind::DeviceInfo {
            max_message_len,
            link_version,
            api_model_version,
            user_api_version,
            api_hash,
            packet_accumulation_time_us,
        }) => {
            let connected_device_info = DeviceApiInfo {
                link_version: FullVersionOwned::new(
                    format!("G{}", link_version.gid.id.0),
                    VersionOwned::new(
                        link_version.major.0,
                        link_version.minor.0,
                        link_version.patch.0,
                    ),
                ),
                max_message_size: max_message_len as usize,
                api_model_version: FullVersionOwned::new(
                    format!("G{}", api_model_version.gid.id.0),
                    VersionOwned::new(
                        api_model_version.major.0,
                        api_model_version.minor.0,
                        api_model_version.patch.0,
                    ),
                ),
                user_api_version,
                user_api_hash: api_hash,
            };
            info!(
                "Connected device: {connected_device_info:?}, acc_window = {}us",
                packet_accumulation_time_us,
            );
            if let Some(client_version) = state.common.client_version.as_ref()
                && !client_version.crate_id.is_empty() // dyn connection without code generated API, using introspect data from a device only
                && !client_version.is_protocol_compatible(&connected_device_info.user_api_version)
            {
                return Err(Error::IncompatibleDeviceProtocol);
            }
            state.common.packet_accumulation_time =
                Duration::from_micros(packet_accumulation_time_us as u64);
            state.common.device_api_info = Some(connected_device_info);
            // only one version is in use right now, so no need to choose between different link versions
            link.send_link_setup(MAX_MESSAGE_SIZE as u32)
                .await
                .map_err(|e| Error::Transport(UsbError::Link(e).into()))?;
        }
        Ok(MessageKind::LinkUp) => {
            info!("LinkSetup complete");
            state.max_protocol_mismatched_messages = 10;
            rx_dispatcher.handle_msg(DispatcherMessage::Connected);
            if let Some(tx) = state.common.connected_tx.take() {
                _ = tx.send(ConnectionInfo {
                    result: Ok(state
                        .common
                        .device_api_info
                        .clone()
                        .unwrap_or(DeviceApiInfo::empty())),
                });
            }
            state.common.on_link_up();
        }
        Ok(MessageKind::IncompatibleVersion) => {
            // device only message, ignore to pass cargo check --all-features
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
    rx_dispatcher: &mut RxDispatcher,
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
        Command::DisconnectKeepStreams {
            disconnected_tx,
            reason,
        } => {
            info!("Disconnecting on user request (but keeping streams ready for re-use)");
            state.common.trace_disconnect("client request", true);
            link.send_disconnect(reason)
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
        Command::DisconnectAndExit {
            disconnected_tx,
            reason,
        } => {
            info!("Disconnecting and stopping USB event loop on user request");
            state.common.trace_disconnect("client request", false);
            link.send_disconnect(reason)
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
        Command::SendMessage {
            mut bytes,
            mut done_tx,
        } => {
            if let Some((done_tx, timeout)) = done_tx.take() {
                if let Some(seq) = rx_dispatcher.next_seq() {
                    // NOTE: this is the only use of Request in this crate, a bit unfortunate to mix it in here, but otherwise
                    // every CommandSender have to get a unique seq number somehow and previous implementation that was doing that
                    // was much uglier and had limitations (see the last use of it at git sha: 0113fa4)
                    ww_client_server::Request::set_seq(&mut bytes, seq);
                    rx_dispatcher.handle_cmd(DispatcherCommand::OnReturn {
                        seq,
                        done_tx,
                        timeout,
                    });
                } else {
                    // TODO: backpressure when out of request IDs
                    _ = done_tx.send(Err(Error::Other("No more request IDs available".into())));
                }
            }
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
        Command::OnStreamEvent {
            path_kind,
            stream_event_tx,
        } => {
            rx_dispatcher.handle_cmd(DispatcherCommand::OnStreamEvent {
                path_kind: *path_kind,
                stream_event_tx,
            });
        }
        Command::LoopbackTest {
            test_duration,
            packet_size,
            progress_tx,
        } => {
            let packet_size = if let Some(requested) = packet_size {
                requested.min(state.max_packet_size)
            } else {
                state.max_packet_size
            };
            loopback_test(test_duration, packet_size, progress_tx, link, scratch).await;
        }
    }
    Ok(EventLoopSpinResult::Continue)
}
