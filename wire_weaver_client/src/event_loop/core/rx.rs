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
use ww_link::{DisconnectReason, Kind, Message, Role};

use super::{GENERATIONS, ToRx, ToTx, compact_to_full, host_hello, is_due};
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
    /// Expecting the device's hello, DeviceInfo, LinkReady
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
    client_version: Option<crate::ClientApi>,
    /// [ToRx::TransportUp]`::host_max_message_len`
    host_max_message_len: usize,
    device_api_info: Option<DeviceApiInfo>,
    /// This connection's setup token, see [ww_link::Message::Hello]
    token: u64,
    /// The device's hello with [Self::token] arrived: everything before it was from a previous session
    token_seen: bool,
    /// This session's DeviceInfo arrived
    info_seen: bool,

    last_rx_at: Option<Instant>,
    malformed_messages_left: u32,
    /// See [Command::Connect](crate::event_loop::command::Command::Connect)`::keep_streams_on_loss`
    keep_streams_on_loss: bool,
    /// [ToRx::Disconnecting]: tx sent `Disconnect`
    disconnecting: bool,
    /// [ToRx::Relink]: link setup with the hub that adopted the device, the dispatcher is connected already
    relinking: bool,
}

/// What [RxCore::link_part] gives for the link residual (CLT-44).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RxPart {
    pub last_rx_ago: Duration,
    /// Streams open on the device, by path
    pub streams: Vec<Vec<u32>>,
    /// The `Observe` request of each observed level, as last sent
    pub observed: Vec<Vec<u8>>,
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
            host_max_message_len: crate::DEFAULT_MAX_MESSAGE_LEN,
            device_api_info: None,
            token: 0,
            token_seen: false,
            info_seen: false,
            last_rx_at: None,
            malformed_messages_left: MAX_MALFORMED_MESSAGES,
            keep_streams_on_loss: false,
            disconnecting: false,
            relinking: false,
        }
    }

    /// The link's state, for the link residual (CLT-44).
    pub fn link_part(&self, now: Instant) -> RxPart {
        RxPart {
            last_rx_ago: self
                .last_rx_at
                .map(|t| now.saturating_duration_since(t))
                .unwrap_or_default(),
            streams: self.dispatcher.open_stream_paths(),
            observed: self.dispatcher.observe_requests(),
        }
    }

    pub fn handle(&mut self, now: Instant, input: RxInput<'_>) {
        let r = match input {
            RxInput::Message {
                kind,
                message: payload,
            } => self.on_message(now, kind, payload),
            RxInput::TransportError(e) if self.disconnecting => {
                // the other side closed in answer to our Disconnect
                debug!("medium closed after Disconnect: {e}");
                Ok(Some(EventLoopExitReason::DisconnectCommand))
            }
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
            ToRx::Disconnecting => self.disconnecting = true,
            ToRx::TransportUp {
                client_version,
                host_max_message_len,
                token,
                connected_tx,
                link,
                keep_streams_on_loss,
            } => {
                self.events.set(link);
                self.keep_streams_on_loss = keep_streams_on_loss;
                self.phase = Phase::LinkSetup;
                self.client_version = Some(*client_version);
                self.host_max_message_len = host_max_message_len;
                self.token = token;
                self.token_seen = false;
                self.info_seen = false;
                self.connected_tx = connected_tx;
                self.last_rx_at = Some(now);
                self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
            }
            ToRx::Relink { token } => {
                self.phase = Phase::LinkSetup;
                self.token = token;
                self.token_seen = false;
                self.info_seen = false;
                self.relinking = true;
                // through the hub now: a loss is reconnected through it, streams resumed (CLT-40)
                self.keep_streams_on_loss = true;
                self.last_rx_at = Some(now);
                self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
            }
            ToRx::Adopted {
                device_info,
                last_rx_ago,
                forwards,
                forward_timeout,
            } => {
                let info = DeviceApiInfo {
                    link_version: compact_to_full(&device_info.dev_link_version),
                    max_message_size: device_info.dev_max_message_len as usize,
                    host_max_message_size: self.host_max_message_len,
                    api_model_version: compact_to_full(&device_info.api_model_version),
                    user_api_version: device_info.user_api_version.clone(),
                    user_api_hash: device_info.hash.clone(),
                    user_api_wire: device_info.user_api_wire,
                    generations: GENERATIONS,
                    accumulation_time: Duration::from_micros(
                        device_info.packet_accumulation_time_us.into(),
                    ),
                    image_facts: device_info.image_facts,
                };
                info!("adopted a live link to {:?}", info.user_api_version);
                self.dispatcher.message_limit = Some(info.message_limit());
                self.device_api_info = Some(info.clone());
                self.token_seen = true;
                self.info_seen = true;
                self.phase = Phase::Up;
                self.last_rx_at = Some(now.checked_sub(last_rx_ago).unwrap_or(now));
                self.dispatcher.handle_msg(DispatcherMessage::Connected);
                for (seq, tag) in forwards {
                    self.dispatcher
                        .expect_forward(now, seq, tag, forward_timeout);
                }
                if let Some(tx) = self.connected_tx.take() {
                    _ = tx.send(ConnectionInfo { result: Ok(info) });
                }
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
            Ok(Message::DeviceInfo(info))
                if !self.token_seen && info.token == pre_hello_token(self.token) =>
            {
                // a device from before the hello read the first 8 bytes of ours as its token and echoed them
                Err(anyhow!(Error::IncompatibleLinkVersion {
                    device: format!(
                        "{}.{}.{} (before the hello)",
                        info.dev_link_version.major.0,
                        info.dev_link_version.minor.0,
                        info.dev_link_version.patch.0
                    ),
                    host: format!(
                        "{}.{}.{}",
                        ww_link::LINK_VERSION.major.0,
                        ww_link::LINK_VERSION.minor.0,
                        ww_link::LINK_VERSION.patch.0
                    ),
                }))
            }
            Ok(msg) if !self.token_seen && !matches!(msg, Message::Hello(_)) => {
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
            Err(ww_link::Error::Malformed(kind @ (Kind::Hello | Kind::DeviceInfo)))
                if self.phase == Phase::LinkSetup =>
            {
                // setup messages of this device's own session always decode: these are of another format
                self.events
                    .error(&format!("unknown peer format: {kind:?} {payload:02x?}"));
                Err(anyhow!(Error::UnknownPeerFormat {
                    kind: kind as u8,
                    bytes: payload.to_vec(),
                }))
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
                return Ok(Some(EventLoopExitReason::DisconnectFromDevice(reason)));
            }
            Message::Ping | Message::Nop => {}
            Message::Hello(hello) => {
                if self.phase != Phase::LinkSetup || self.token_seen || hello.role != Role::Device {
                    warn!("unexpected hello in {:?}, ignoring: {hello:?}", self.phase);
                    return Ok(None);
                }
                if hello.token != self.token {
                    debug!(
                        "dropping the hello of a previous session (token {:#x})",
                        hello.token
                    );
                    return Ok(None);
                }
                if let Some(m) = GENERATIONS.mismatch(&hello.generations) {
                    return Err(anyhow!(Error::IncompatibleGeneration {
                        layer: m.layer.name(),
                        device: m.theirs,
                        host: m.ours,
                    }));
                }
                self.token_seen = true;
            }
            Message::DeviceInfo(info) => {
                if self.phase != Phase::LinkSetup || !self.token_seen || self.info_seen {
                    warn!("unexpected DeviceInfo in {:?}, ignoring", self.phase);
                    return Ok(None);
                }
                if info.token != self.token {
                    debug!(
                        "dropping DeviceInfo of a previous session (token {:#x})",
                        info.token
                    );
                    return Ok(None);
                }
                self.info_seen = true;
                let device_api_info = device_api_info(&info, self.host_max_message_len);
                info!(
                    "connected device: {device_api_info:?}, acc_window = {}us",
                    info.packet_accumulation_time_us
                );
                // a dynamic client (no generated code) works with any API through the device's introspection data;
                // a generated one needs the same API on the same wire line (EVO-25), whatever the crate versions; a host
                // with clients of several lines takes a device on any of them (EVO-28)
                if let Some(client) = self.client_version.as_ref()
                    && !client.is_dynamic()
                    && let Some(host_wire) = client.wire
                    && client
                        .for_device(
                            &device_api_info.user_api_version.crate_id,
                            device_api_info.user_api_wire,
                        )
                        .is_none()
                {
                    return Err(anyhow!(Error::IncompatibleDeviceProtocol {
                        device: Box::new(device_api_info.user_api_version.clone()),
                        device_wire: device_api_info.user_api_wire,
                        host: Box::new(client.version.clone()),
                        host_wire,
                        host_other_wires: client.other_lines.iter().map(|(_, w)| *w).collect(),
                    }));
                }
                self.dispatcher.message_limit = Some(device_api_info.message_limit());
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
                if std::mem::take(&mut self.relinking) {
                    info!("link goes on through the ww hub");
                } else {
                    self.dispatcher.handle_msg(DispatcherMessage::Connected);
                }
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
                    && let Some(sink) = &mut self.dispatcher.forward_sink
                {
                    // an echo for a client of the hub, its own layout: | repeat: u32 LE | seq: u32 LE | data |
                    let mut payload = Vec::with_capacity(8 + data.len());
                    payload.extend_from_slice(&repeat.to_le_bytes());
                    payload.extend_from_slice(&seq.to_le_bytes());
                    payload.extend_from_slice(data);
                    sink.forward(crate::event_loop::command::Forwarded::Loopback(payload));
                } else if self.phase == Phase::Up {
                    // tx ignores it when no test is running
                    self.output.push_back(RxOutput::ToTx(ToTx::Loopback {
                        seq,
                        len: data.len(),
                        intact: super::loopback::is_intact(data),
                    }));
                }
            }
            Message::LinkSetup(_) | Message::GetStats | Message::Stats(_) => {
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
        for retry in self.dispatcher.take_absolute_retries() {
            self.output
                .push_back(RxOutput::ToTx(ToTx::ResolveAbsolute(Box::new(retry))));
        }
        // a stream opened with a codec this host doesn't know: closed, then opened again with raw (API-55)
        for (close, reopen) in self.dispatcher.take_reopens() {
            self.output.push_back(RxOutput::ToTx(ToTx::Send(close)));
            if let Some((retry, done_tx, timeout, hook)) = reopen {
                self.output.push_back(RxOutput::ToTx(ToTx::RetryFull {
                    retry,
                    done_tx,
                    timeout,
                    hook,
                }));
            }
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
        // the link went (not a disconnect asked for): the client reconnects by itself and resumes them (CLT-40)
        let lost = matches!(
            result,
            Err(_) | Ok(EventLoopExitReason::DisconnectFromDevice(_))
        );
        if lost && self.keep_streams_on_loss {
            self.dispatcher.keep_streams = true;
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
        self.info_seen = false;
        self.last_rx_at = None;
        // freed seqs don't matter anymore, tx clears in-flight on exit
        self.dispatcher.take_freed();
        for (_, done_tx, _, _) in self.dispatcher.take_retries() {
            _ = done_tx.send(Err(Error::Disconnected));
        }
        for retry in self.dispatcher.take_absolute_retries() {
            _ = retry.done_tx.send(Err(Error::Disconnected));
        }
        for (_, reopen) in self.dispatcher.take_reopens() {
            if let Some((_, done_tx, _, _)) = reopen {
                _ = done_tx.send(Err(Error::Disconnected));
            }
        }
        if notify_tx {
            self.output
                .push_back(RxOutput::ToTx(ToTx::PeerGone(result)));
        }
        self.output.push_back(RxOutput::Exit);
    }
}

/// What a device's `DeviceInfo` says, as the client keeps it.
/// The token a device from before the hello reads from the host's hello: its first 8 bytes.
fn pre_hello_token(token: u64) -> u64 {
    let bytes = host_hello(token).to_bytes();
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}

fn device_api_info(info: &ww_link::DeviceInfo<'_>, host_max_message_len: usize) -> DeviceApiInfo {
    DeviceApiInfo {
        link_version: compact_to_full(&info.dev_link_version),
        max_message_size: info.dev_max_message_len as usize,
        host_max_message_size: host_max_message_len,
        api_model_version: compact_to_full(&info.api_model_version),
        user_api_version: info.user_api_version.make_owned(),
        user_api_hash: info.hash.make_owned(),
        user_api_wire: info.user_api_wire,
        generations: GENERATIONS,
        accumulation_time: Duration::from_micros(info.packet_accumulation_time_us.into()),
        image_facts: info.image_facts,
    }
}
