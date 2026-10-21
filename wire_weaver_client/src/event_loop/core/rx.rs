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

use super::{ToRx, ToTx, compact_to_full, is_due};
use crate::Error;
use crate::device_info::{ConnectionInfo, DeviceApiInfo};
use crate::event_loop::command::EventLoopExitReason;
use crate::event_loop::rx_dispatcher::{DispatcherCommand, DispatcherMessage, RxDispatcher};
use crate::tap::LinkEvents;

const PEER_TIMEOUT: Duration = Duration::from_millis(ww_link::PEER_TIMEOUT_MS);
/// How many messages that fail to decode (probably from an old session or a different link version)
/// to tolerate before giving up.
const MAX_MALFORMED_MESSAGES: u32 = 10;

pub(crate) enum RxInput<'i> {
    /// One de-framed link message: framer `user_kind` and payload.
    Message {
        kind: u8,
        message: &'i [u8],
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
    /// Link events into the link's tap
    events: LinkEvents,

    connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    client_version: Option<FullVersionOwned>,
    device_api_info: Option<DeviceApiInfo>,
    /// This connection's setup token, see [ww_link::Message::GetDeviceInfo]
    token: u64,
    /// A DeviceInfo with [Self::token] arrived: everything before it was from a previous session
    token_seen: bool,

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
            events: LinkEvents::default(),
            connected_tx: None,
            client_version: None,
            device_api_info: None,
            token: 0,
            token_seen: false,
            last_rx_at: None,
            malformed_messages_left: MAX_MALFORMED_MESSAGES,
        }
    }

    pub fn handle(&mut self, now: Instant, input: RxInput<'_>) {
        let r = match input {
            RxInput::Message {
                kind,
                message: payload,
            } => self.on_message(now, kind, payload),
            RxInput::TransportError(e) => {
                self.events.error(&e);
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
        let streams = match self.phase {
            Phase::Up => self.dispatcher.next_stream_poll_at(),
            Phase::Idle | Phase::LinkSetup => None,
        };
        [self.dispatcher.next_prune_at(), peer, streams]
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
                hook,
                retry,
            } => {
                self.dispatcher.handle_cmd(
                    now,
                    DispatcherCommand::OnReturn {
                        seq,
                        done_tx,
                        timeout,
                        hook,
                        retry,
                    },
                );
            }
            ToRx::OnStreamEvent {
                path_kind,
                stream_event_tx,
                registered_tx,
                sink,
            } => {
                self.dispatcher.handle_cmd(
                    now,
                    DispatcherCommand::OnStreamEvent {
                        path_kind: *path_kind,
                        stream_event_tx,
                        registered_tx,
                        sink,
                    },
                );
            }
            ToRx::ExpectForward { seq, tag, timeout } => {
                self.dispatcher.expect_forward(now, seq, tag, timeout)
            }
            ToRx::ForwardSink(tx) => self.dispatcher.forward_sink = Some(tx),
            ToRx::ForwardFailed { tag, error } => self.dispatcher.forward_failed(tag, error),
            ToRx::ObserveRoute { hook } => self
                .dispatcher
                .handle_cmd(now, DispatcherCommand::ObserveRoute { hook }),
            ToRx::KeepStreams => self.dispatcher.keep_streams = true,
            ToRx::TransportUp {
                client_version,
                token,
                connected_tx,
                link,
            } => {
                self.events.set(link);
                self.phase = Phase::LinkSetup;
                self.client_version = Some(*client_version);
                self.token = token;
                self.token_seen = false;
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
        self.forward_freed(now);
    }

    fn on_timer(&mut self, now: Instant) -> anyhow::Result<Option<EventLoopExitReason>> {
        self.dispatcher.prune(now);
        self.forward_freed(now);
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
            Ok(msg) if !self.token_seen && !matches!(msg, Message::DeviceInfo(_)) => {
                // from a previous session: an answer still in the device's buffers, a half-sent message
                debug!(
                    "dropping {:?} from before this session's DeviceInfo",
                    msg.kind()
                );
                Ok(None)
            }
            Ok(msg) => self.on_link_message(msg),
            Err(ww_link::Error::UnknownKind(kind)) => {
                warn!("unknown link message kind {kind}, ignoring");
                Ok(None)
            }
            Err(e) => {
                self.events.error(&format!("{e:?}"));
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
        self.forward_freed(now);
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
                self.dispatcher
                    .handle_msg(DispatcherMessage::MessageBytes(bytes));
            }
            Message::Disconnect(reason) => {
                self.events
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
                if self.phase != Phase::LinkSetup || self.token_seen {
                    warn!("unexpected DeviceInfo in {:?}, ignoring", self.phase);
                    return Ok(None);
                }
                // an older link never answers with the token: that, not a timeout, is the error
                if info.token == 0
                    || !info
                        .dev_link_version
                        .is_protocol_compatible(&ww_link::LINK_VERSION)
                {
                    let v = |v: &ww_version::CompactVersion| {
                        format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0)
                    };
                    let device = if info.token == 0 {
                        format!("{} (without setup token)", v(&info.dev_link_version))
                    } else {
                        v(&info.dev_link_version)
                    };
                    return Err(anyhow!(Error::IncompatibleLinkVersion {
                        device,
                        host: v(&ww_link::LINK_VERSION),
                    }));
                }
                if info.token != self.token {
                    debug!(
                        "dropping DeviceInfo of a previous session (token {:#x})",
                        info.token
                    );
                    return Ok(None);
                }
                self.token_seen = true;
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
            Message::LinkReady { token } => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected LinkReady in {:?}, ignoring", self.phase);
                    return Ok(None);
                }
                if token != self.token {
                    debug!("dropping LinkReady of a previous session (token {token:#x})");
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
            Message::Loopback { repeat, seq, data } => {
                if self.phase == Phase::Up
                    && let Some(sink) = &self.dispatcher.forward_sink
                {
                    // an echo for a client of the hub, its own layout: | repeat: u32 LE | seq: u32 LE | data |
                    let mut payload = Vec::with_capacity(8 + data.len());
                    payload.extend_from_slice(&repeat.to_le_bytes());
                    payload.extend_from_slice(&seq.to_le_bytes());
                    payload.extend_from_slice(data);
                    _ = sink.send(crate::event_loop::command::Forwarded::Loopback(payload));
                } else if self.phase == Phase::Up {
                    // tx ignores it when no test is running
                    self.output.push_back(RxOutput::ToTx(ToTx::Loopback {
                        seq,
                        len: data.len(),
                        intact: super::loopback::is_intact(data),
                    }));
                }
            }
            Message::GetDeviceInfo { .. }
            | Message::LinkSetup(_)
            | Message::GetStats
            | Message::Stats(_) => {
                warn!("ignoring device-only or unsupported message: {msg:?}");
            }
        }
        Ok(None)
    }

    // Helpers

    /// Freed seq numbers, and what streams send (`Ack`, `Resend`), to tx.
    fn forward_freed(&mut self, now: Instant) {
        for seq in self.dispatcher.take_freed() {
            self.output.push_back(RxOutput::ToTx(ToTx::Freed(seq)));
        }
        for (retry, done_tx, timeout, hook) in self.dispatcher.take_retries() {
            self.output.push_back(RxOutput::ToTx(ToTx::RetryFull {
                retry,
                done_tx,
                timeout,
                hook,
            }));
        }
        if self.phase == Phase::Up {
            for bytes in self.dispatcher.poll_streams(now) {
                self.output.push_back(RxOutput::ToTx(ToTx::Send(bytes)));
            }
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
        self.token_seen = false;
        self.last_rx_at = None;
        // freed seqs don't matter anymore, tx clears in-flight on exit
        self.dispatcher.take_freed();
        for (_, done_tx, _, _) in self.dispatcher.take_retries() {
            _ = done_tx.send(Err(Error::Disconnected));
        }
        if notify_tx {
            self.output
                .push_back(RxOutput::ToTx(ToTx::PeerGone(result)));
        }
        self.output.push_back(RxOutput::Exit);
    }
}
