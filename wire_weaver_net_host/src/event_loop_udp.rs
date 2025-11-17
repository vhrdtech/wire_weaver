use shrink_wrap::ref_vec::RefVec;
use shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap};
use std::net::IpAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use wire_weaver_client_common::event_loop_state::CommonState;
use wire_weaver_client_common::ww_client_server::{Event, EventKind};
use wire_weaver_client_common::ww_version::FullVersion;
use wire_weaver_client_common::{Command, Error, ww_client_server, ww_client_server::RequestKind};
use wire_weaver_udp_link::{Datagram, Op, UDP_LINK_MAGIC};

const MAX_RX_DATAGRAM_LEN: usize = 1500 - 20 - 8;

pub struct UdpTarget {
    pub addr: IpAddr,
    pub port: u16,
}

pub type UdpError = ();

#[derive(Default)]
struct State {
    common: CommonState,
}

struct Link {
    target: UdpTarget,
    socket: UdpSocket,
    seq: u16,
    scratch: [u8; 2048],
}

pub async fn udp_worker(mut cmd_rx: mpsc::UnboundedReceiver<Command>) {
    debug!("udp worker started");
    let mut state = State::default();
    let mut link = None;
    let mut scratch = [0u8; 2048];
    let mut udp_rx = [0u8; MAX_RX_DATAGRAM_LEN];
    loop {
        match &mut link {
            Some(l) => {
                match process_commands_and_endpoints(
                    &mut cmd_rx,
                    l,
                    &mut state,
                    &mut scratch,
                    &mut udp_rx,
                )
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
    debug!("udp worker exited");
}

impl Link {
    // TODO: UDP accumulate and send many at once
    async fn send_op(&mut self, op: Op<'_>) -> Result<(), Error> {
        let datagram = Datagram {
            magic: UDP_LINK_MAGIC,
            seq: self.seq,
            ops: RefVec::Slice { slice: &[op] },
        };
        let bytes = datagram.to_ww_bytes(&mut self.scratch)?;
        trace!(
            "sending datagram to {}:{} {:02x?}",
            self.target.addr, self.target.port, bytes
        );
        self.socket
            .send_to(bytes, (self.target.addr, self.target.port))
            .await?;
        self.seq = self.seq.wrapping_add(1);
        Ok(())
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
    link: &mut Link,
    state: &mut State,
    scratch: &mut [u8],
    udp_rx: &mut [u8],
) -> Result<EventLoopResult, Error> {
    link.send_op(Op::GetDeviceInfo).await?;
    let mut link_setup_retries = 5;
    loop {
        let duration = if state.common.link_setup_done {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(50)
        };
        let timer = tokio::time::sleep(duration);
        tokio::select! {
            len_remote = link.socket.recv_from(udp_rx) => {
                let len_remote = len_remote?;
                let len = len_remote.0;
                let remote = len_remote.1;
                let datagram = &udp_rx[..len];
                trace!("received datagram from {:?} {:02x?}", remote, datagram);
                match handle_datagram(datagram, link, state).await? {
                    EventLoopSpinResult::Continue => {}
                    EventLoopSpinResult::DisconnectKeepStreams => return Ok(EventLoopResult::DisconnectKeepStreams),
                    EventLoopSpinResult::DisconnectFromDevice => return Ok(EventLoopResult::Disconnect),
                    EventLoopSpinResult::DisconnectAndExit => return Ok(EventLoopResult::Exit)
                }
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    info!("all tx instances were dropped, exiting");
                    _ = link.send_op(Op::Disconnect { reason: "cmd_tx_dropped" }).await;
                    return Ok(EventLoopResult::Exit);
                };
                match handle_command(cmd, link, state, scratch).await? {
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
                        link.send_op(Op::GetDeviceInfo).await?;
                        link_setup_retries -= 1;
                    } else {
                        error!("worker exiting, because link setup failed after several retries");
                        return Err(Error::LinkSetupTimeout);
                    }
                }
                state.common.prune_timed_out_requests();
                // TODO: send accumulated message or Ping
            }
        }
    }
}

async fn wait_for_connection_and_queue_commands(
    cmd_rx: &mut mpsc::UnboundedReceiver<Command>,
    state: &mut State,
) -> Result<Option<Link>, Error> {
    loop {
        let Some(cmd) = cmd_rx.recv().await else {
            return Err(Error::CmdTxDropped);
        };
        match cmd {
            Command::Connect {
                filter,
                user_protocol_version,
                on_error,
                connected_tx,
            } => {
                state
                    .common
                    .on_connect(on_error, connected_tx, user_protocol_version);
                let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
                let local_addr = udp_socket.local_addr();
                trace!("local addr: {:?}", local_addr);
                return Ok(Some(Link {
                    target: filter,
                    socket: udp_socket,
                    seq: 0,
                    scratch: [0u8; 2048],
                }));
            }
            Command::StreamOpen {
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
                return Err(Error::ExitRequested);
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

async fn handle_datagram(
    datagram: &[u8],
    link: &mut Link,
    state: &mut State,
) -> Result<EventLoopSpinResult, Error> {
    let datagram = Datagram::from_ww_bytes(datagram)?;
    // TODO: discard duplicates
    for op in datagram.ops.iter() {
        let op = op?;
        match op {
            Op::RequestData { .. } => {}
            Op::EventData { data } => {
                if data.is_empty() {
                    warn!("got empty event data, ignoring");
                    return Ok(EventLoopSpinResult::Continue);
                }
                let event = match Event::from_ww_bytes(data.as_slice()) {
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
                            if let Some((done_tx, _)) = state.common.response_map.remove(&event.seq)
                            {
                                let r = data.as_slice().to_vec();
                                let _ = done_tx.send(Ok(r));
                            }
                        }
                        EventKind::Written => {
                            if let Some((done_tx, _)) = state.common.response_map.remove(&event.seq)
                            {
                                let _ = done_tx.send(Ok(Vec::new()));
                            }
                        }
                        EventKind::StreamData { path, data } => {
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
            Op::GetDeviceInfo => {}
            Op::DeviceInfo {
                server,
                user,
                max_datagram_length,
            } => {
                info!(
                    "DeviceInfo: server: {server:?} user: {user:?} max_len: {max_datagram_length:?}"
                );
                if let Some(user_version) = &state.common.user_protocol_version {
                    link.send_op(Op::LinkSetup {
                        client: ww_client_server::FULL_VERSION,
                        user: FullVersion {
                            crate_id: user_version.crate_id.as_str(),
                            version: user_version.version.as_ref(),
                        },
                        max_datagram_length: MAX_RX_DATAGRAM_LEN as u16,
                    })
                    .await?;
                } else {
                    error!("no user protocol version");
                }
            }
            Op::LinkSetup { .. } => {}
            Op::LinkSetupResult { is_compatible } => {
                if !is_compatible {
                    error!("device rejected LinkSetup, exiting");
                    if let Some(tx) = state.common.connected_tx.take() {
                        _ = tx.send(Err(Error::IncompatibleDeviceProtocol));
                    }
                    return Err(Error::IncompatibleDeviceProtocol);
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
            }
            Op::KeepAlive => {}
            Op::Disconnect { reason } => {
                info!("Received Disconnect from remote device (reason '{reason}'), exiting");
                return Ok(EventLoopSpinResult::DisconnectFromDevice);
            }
        }
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn handle_command(
    cmd: Command,
    link: &mut Link,
    state: &mut State,
    scratch: &mut [u8],
) -> Result<EventLoopSpinResult, Error> {
    match cmd {
        Command::Connect { .. } => {
            warn!("Ignoring Connect while already connected");
        }
        Command::DisconnectKeepStreams { disconnected_tx } => {
            info!("Disconnecting on user request (but keeping streams ready for re-use)");
            link.send_op(Op::Disconnect {
                reason: "disconnect_keep_streams",
            })
            .await?;
            if let Some(done_tx) = disconnected_tx {
                let _ = done_tx.send(());
            }
            return Ok(EventLoopSpinResult::DisconnectKeepStreams);
        }
        Command::DisconnectAndExit { disconnected_tx } => {
            info!("Disconnecting and stopping event loop on user request");
            link.send_op(Op::Disconnect {
                reason: "disconnect_and_exit",
            })
            .await?;
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
            trace!("sending call to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
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
            trace!("sending write to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
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
            trace!("sending read to {path:?}");
            let seq = state.common.register_prune_next_seq(timeout, done_tx);
            let request = ww_client_server::Request {
                seq,
                path: RefVec::Slice { slice: &path },
                kind: RequestKind::Read,
            };
            serialize_request_send(request, link, state, scratch).await?;
        }
        Command::StreamOpen {
            path_kind,
            stream_data_tx,
        } => {
            state.common.stream_handlers.insert(path, stream_data_tx);
        }
    }
    Ok(EventLoopSpinResult::Continue)
}

async fn serialize_request_send(
    request: ww_client_server::Request<'_>,
    link: &mut Link,
    _state: &mut State,
    scratch: &mut [u8],
    // scratch2: &mut [u8],
) -> Result<(), Error> {
    let request_bytes = request.to_ww_bytes(scratch)?;
    link.send_op(Op::RequestData {
        data: RefVec::new_bytes(request_bytes),
    })
    .await?;

    // if link.is_tx_queue_empty() {
    //     state.common.packet_started_instant = None;
    // } else {
    //     state.common.packet_started_instant = Some(Instant::now());
    // }
    // link.force_send().await?; // TODO: force send on timer
    Ok(())
}
