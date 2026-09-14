//! Rx half: link messages in, dispatcher and [ToTx] events out. Owns decoding, the version check,
//! the dispatcher (responses, streams, request timeouts) and the peer timeout.
//!
//! Ordering: a wrapper must drain [RxInput::FromTx] messages before feeding each received frame's
//! messages, so that [ToRx::Expect] is registered before the corresponding answer is decoded.
//! Since tx emits `Expect` before the request bytes and channels are FIFO, this is sufficient.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::sync::oneshot;
use tracing::{debug, error, info, trace, warn};
use ww_link::{DisconnectReason, Message};
use ww_version::FullVersionOwned;

use super::{ToRx, ToTx, Tracers, compact_to_full, is_due};
use crate::Error;
use crate::device_info::{ConnectionInfo, DeviceApiInfo};
use crate::event_loop::command::EventLoopExitReason;
use crate::event_loop::rx_dispatcher::{DispatcherCommand, DispatcherMessage, RxDispatcher};

const PEER_TIMEOUT: Duration = Duration::from_millis(ww_link::PEER_TIMEOUT_MS);
/// How many messages that fail to decode (probably from an old session or a different link version)
/// to tolerate before giving up.
const MAX_MALFORMED_MESSAGES: u32 = 10;

pub(crate) enum RxInput<'i> {
    /// One de-framed link message: framer `user_kind` and payload.
    Message {
        kind: u8,
        payload: &'i [u8],
    },
    /// Read failed.
    TransportError(String),
    FromTx(ToRx),
    /// In response to [RxCore::poll_timeout] deadline.
    Timer,
}

pub(crate) enum RxOutput {
    ToTx(ToTx),
    /// Stop reading. Dispatcher has been notified already.
    Exit,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// Not connected, ignore everything but [ToRx::TransportUp]
    Idle,
    /// Expecting DeviceInfo / LinkReady
    LinkSetup,
    /// Data flows
    Up,
}

pub(crate) struct RxCore {
    phase: Phase,
    dispatcher: RxDispatcher,
    output: VecDeque<RxOutput>,
    tracers: Tracers,

    connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    client_version: Option<FullVersionOwned>,
    device_api_info: Option<DeviceApiInfo>,

    last_rx_at: Option<Instant>,
    malformed_messages_left: u32,
}

impl Default for RxCore {
    fn default() -> Self {
        Self::new()
    }
}

impl RxCore {
    pub fn new() -> Self {
        RxCore {
            phase: Phase::Idle,
            dispatcher: RxDispatcher::default(),
            output: VecDeque::new(),
            tracers: Tracers::default(),
            connected_tx: None,
            client_version: None,
            device_api_info: None,
            last_rx_at: None,
            malformed_messages_left: MAX_MALFORMED_MESSAGES,
        }
    }

    pub fn handle(&mut self, now: Instant, input: RxInput<'_>) {
        let r = match input {
            RxInput::Message { kind, payload } => self.on_message(now, kind, payload),
            RxInput::TransportError(e) => {
                self.tracers.error(&e);
                Err(anyhow!(Error::Transport(e)))
            }
            RxInput::FromTx(msg) => {
                self.on_from_tx(now, msg);
                Ok(None)
            }
            RxInput::Timer => self.on_timer(now),
        };
        match r {
            Ok(None) => {}
            Ok(Some(reason)) => self.exit(Ok(reason), true),
            Err(e) => self.exit(Err(e), true),
        }
    }

    pub fn poll_output(&mut self) -> Option<RxOutput> {
        self.output.pop_front()
    }

    /// Earliest instant at which [RxInput::Timer] should be fed, None if nothing is scheduled.
    pub fn poll_timeout(&self) -> Option<Instant> {
        let peer = match self.phase {
            Phase::Up => self.last_rx_at.map(|t| t + PEER_TIMEOUT),
            Phase::Idle | Phase::LinkSetup => None,
        };
        [self.dispatcher.next_prune_at(), peer]
            .into_iter()
            .flatten()
            .min()
    }

    /// Pending connection reporter, if the connection was never established. For [super::TxCore::into_residual].
    pub fn into_connected_tx(self) -> Option<oneshot::Sender<ConnectionInfo>> {
        self.connected_tx
    }

    // Inputs

    fn on_from_tx(&mut self, now: Instant, msg: ToRx) {
        match msg {
            ToRx::Expect {
                seq,
                done_tx,
                timeout,
            } => {
                self.dispatcher.handle_cmd(
                    now,
                    DispatcherCommand::OnReturn {
                        seq,
                        done_tx,
                        timeout,
                    },
                );
            }
            ToRx::OnStreamEvent {
                path_kind,
                stream_event_tx,
            } => {
                self.dispatcher.handle_cmd(
                    now,
                    DispatcherCommand::OnStreamEvent {
                        path_kind: *path_kind,
                        stream_event_tx,
                    },
                );
            }
            ToRx::RegisterTracer(tx) => self.tracers.register(tx),
            ToRx::TransportUp {
                client_version,
                connected_tx,
            } => {
                self.phase = Phase::LinkSetup;
                self.client_version = Some(*client_version);
                self.connected_tx = connected_tx;
                self.last_rx_at = Some(now);
                self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
            }
            ToRx::Stop(err) => {
                debug!("tx side is gone, exiting");
                // tx decided and reports the result to the client, nothing to send back;
                // only a pending connected_tx (if any) is answered here
                let result = match err {
                    Some(e) => Err(anyhow!(e)),
                    None => Ok(EventLoopExitReason::DisconnectCommand),
                };
                self.exit(result, false);
            }
        }
        self.forward_freed();
    }

    fn on_timer(&mut self, now: Instant) -> anyhow::Result<Option<EventLoopExitReason>> {
        self.dispatcher.prune(now);
        self.forward_freed();
        if self.phase == Phase::Up && is_due(self.last_rx_at.map(|t| t + PEER_TIMEOUT), now) {
            warn!("nothing received from device for {PEER_TIMEOUT:?}, exiting");
            return Err(anyhow!(Error::NoPingFromDevice));
        }
        Ok(None)
    }

    fn on_message(
        &mut self,
        now: Instant,
        kind: u8,
        payload: &[u8],
    ) -> anyhow::Result<Option<EventLoopExitReason>> {
        if self.phase == Phase::Idle {
            warn!("message received while transport is down, ignoring");
            return Ok(None);
        }
        self.last_rx_at = Some(now);
        let r = match Message::decode(kind, payload) {
            Ok(msg) => self.on_link_message(msg),
            Err(ww_link::Error::UnknownKind(kind)) => {
                warn!("unknown link message kind {kind}, ignoring");
                Ok(None)
            }
            Err(e) => {
                self.tracers.error(&format!("{e:?}"));
                if self.malformed_messages_left > 0 {
                    warn!(
                        "malformed link message {e:?}, probably old message from previous session or link version mismatch?"
                    );
                    self.malformed_messages_left -= 1;
                    Ok(None)
                } else {
                    Err(anyhow!(Error::Transport(format!("{e:?}"))))
                }
            }
        };
        self.forward_freed();
        r
    }

    fn on_link_message(&mut self, msg: Message<'_>) -> anyhow::Result<Option<EventLoopExitReason>> {
        trace!("rx: {msg:?}");
        match msg {
            Message::Data { bytes, .. } => {
                if self.phase != Phase::Up {
                    warn!("got data before link is up, ignoring");
                    return Ok(None);
                }
                if bytes.is_empty() {
                    warn!("got empty event data, ignoring");
                    return Ok(None);
                }
                self.tracers.event(bytes);
                self.dispatcher
                    .handle_msg(DispatcherMessage::MessageBytes(bytes));
            }
            Message::Disconnect(reason) => {
                self.tracers
                    .disconnected(&format!("remote: {reason:?}"), false);
                if self.phase != Phase::Up && reason != DisconnectReason::IncompatibleVersion {
                    warn!(
                        "received Disconnect({reason:?}) from remote device, ignoring, must be from old session"
                    );
                    return Ok(None);
                }
                if reason == DisconnectReason::IncompatibleVersion
                    || reason == DisconnectReason::CommanderDropped
                {
                    error!("received Disconnect({reason:?}), exiting");
                } else {
                    info!("received Disconnect({reason:?}) from remote device, exiting");
                }
                return Ok(Some(EventLoopExitReason::DisconnectFromDevice));
            }
            Message::Ping | Message::Nop => {}
            Message::DeviceInfo(info) => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected DeviceInfo in {:?}, ignoring", self.phase);
                    return Ok(None);
                }
                let device_api_info = DeviceApiInfo {
                    link_version: compact_to_full(&info.dev_link_version),
                    max_message_size: info.dev_max_message_len as usize,
                    api_model_version: compact_to_full(&info.api_model_version),
                    user_api_version: info.user_api_version.make_owned(),
                    user_api_hash: info.hash.make_owned(),
                };
                info!(
                    "connected device: {device_api_info:?}, acc_window = {}us",
                    info.packet_accumulation_time_us
                );
                if let Some(client_version) = self.client_version.as_ref()
                    && !client_version.crate_id.is_empty() // dyn connection without code generated API, using introspect data from a device only
                    && !client_version.is_protocol_compatible(&device_api_info.user_api_version)
                {
                    return Err(anyhow!(Error::IncompatibleDeviceProtocol));
                }
                self.device_api_info = Some(device_api_info);
                self.output
                    .push_back(RxOutput::ToTx(ToTx::DeviceInfo(Box::new(
                        info.make_owned(),
                    ))));
            }
            Message::LinkReady => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected LinkReady in {:?}, ignoring", self.phase);
                    return Ok(None);
                }
                self.phase = Phase::Up;
                self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
                self.dispatcher.handle_msg(DispatcherMessage::Connected);
                if let Some(tx) = self.connected_tx.take() {
                    _ = tx.send(ConnectionInfo {
                        result: Ok(self
                            .device_api_info
                            .clone()
                            .unwrap_or(DeviceApiInfo::empty())),
                    });
                }
                self.output.push_back(RxOutput::ToTx(ToTx::LinkReady));
            }
            Message::Loopback { .. } => {} // ignore when not testing
            Message::GetDeviceInfo
            | Message::LinkSetup(_)
            | Message::GetStats
            | Message::Stats(_) => {
                warn!("ignoring device-only or unsupported message: {msg:?}");
            }
        }
        Ok(None)
    }

    // Helpers

    fn forward_freed(&mut self) {
        for seq in self.dispatcher.take_freed() {
            self.output.push_back(RxOutput::ToTx(ToTx::Freed(seq)));
        }
    }

    fn exit(&mut self, result: anyhow::Result<EventLoopExitReason>, notify_tx: bool) {
        match &result {
            Ok(r) => debug!("rx exiting with: {r:?}"),
            Err(e) => error!("rx exiting with: {e:?}"),
        }
        if self.phase == Phase::Up {
            self.dispatcher.handle_msg(DispatcherMessage::Disconnected);
        }
        if let Err(e) = &result
            && let Some(tx) = self.connected_tx.take()
        {
            _ = tx.send(ConnectionInfo::err(anyhow!("{e}")));
        }
        self.phase = Phase::Idle;
        self.last_rx_at = None;
        // freed seqs don't matter anymore, tx clears in-flight on exit
        self.dispatcher.take_freed();
        if notify_tx {
            self.output
                .push_back(RxOutput::ToTx(ToTx::PeerGone(result)));
        }
        self.output.push_back(RxOutput::Exit);
    }
}
