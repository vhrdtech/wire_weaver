//! Sans-IO event loop core: all the protocol logic of a client event loop without any IO,
//! timers, channels or framing of its own.
//!
//! The core speaks in [ww_link] messages: `(kind, payload)` pairs. Packing them into frames (and choosing
//! a framer configuration that suits the medium) is up to the wrapper, the core only says *when* to flush.
//!
//! Intended use (both async and blocking wrappers look the same):
//! ```ignore
//! loop {
//!     while let Some(output) = core.poll_output() {
//!         match output {
//!             Output::Connect(handle) => { /* open transport, then Input::TransportUp / TransportError */ }
//!             Output::Send { kind, payload } => { /* framer.write(kind, &payload), sending full frames as they fill up */ }
//!             Output::Flush => { /* send whatever is in the framer, even if not full */ }
//!             Output::Exit(result) => { /* stop */ }
//!         }
//!     }
//!     // wait for a command, a frame or core.poll_timeout(), whichever comes first;
//!     // received frames go through the framer and each message is fed as Input::Message
//!     core.handle(Instant::now(), input);
//! }
//! ```
//!
//! Outputs are queued internally, wrappers drain them after each [Core::handle] call.
//! Backpressure is up to the wrapper: do not feed new inputs while the output queue is not drained.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, trace, warn};
use ww_link::{DisconnectReason, LinkSetup, Message};
use ww_version::{FullVersionOwned, VersionOwned};

use crate::device_info::{ConnectionInfo, DeviceApiInfo};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::{Command, EventLoopExitReason, EventLoopResidual, TestProgress};
use crate::event_loop::rx_dispatcher::{DispatcherCommand, DispatcherMessage, RxDispatcher};
use crate::tracing::tracing::TraceEvent;
use crate::{Error, SeqTy};

/// Maximum ww_client_server message this host can receive, advertised to a device during link setup.
pub(crate) const MAX_MESSAGE_SIZE: usize = 2048;

const PING_INTERVAL: Duration = Duration::from_millis(ww_link::PING_INTERVAL_MS);
const PEER_TIMEOUT: Duration = Duration::from_millis(ww_link::PEER_TIMEOUT_MS);
const LINK_SETUP_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const LINK_SETUP_RETRIES: u32 = 5;
/// How many messages that fail to decode (probably from an old session or a different link version)
/// to tolerate before giving up.
const MAX_MALFORMED_MESSAGES: u32 = 10;
/// Deadlines this close to `now` are considered due, to avoid spinning on tiny sleeps.
const TIMER_TOLERANCE: Duration = Duration::from_micros(10);

pub(crate) enum Input<'i> {
    /// Command from [Commander](crate::Commander)
    Command(Command),
    /// All command senders were dropped, disconnect and exit.
    CommanderDropped,
    /// Transport requested via [Output::Connect] is open.
    TransportUp,
    /// Transport could not be opened or failed while in use.
    TransportError(String),
    /// One de-framed link message: framer `user_kind` and payload.
    Message { kind: u8, payload: &'i [u8] },
    /// In response to [Core::poll_timeout] deadline.
    Timer,
}

pub(crate) enum Output {
    /// Open a transport to the device behind this handle (from [Command::Connect]),
    /// then feed [Input::TransportUp] or [Input::TransportError].
    Connect(DeviceHandle),
    /// Link message to write into the framer. Frames that fill up along the way are sent right away,
    /// a partially filled one is kept until [Output::Flush].
    Send { kind: u8, payload: Vec<u8> },
    /// Send the current frame now, even if not full. No-op when there is nothing pending.
    Flush,
    /// Flush and send all preceding messages, close the transport and stop.
    /// [Core::into_residual] returns what a wrapper should hand back to the client.
    Exit(anyhow::Result<EventLoopExitReason>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// No transport, waiting for [Command::Connect]
    Idle,
    /// [Output::Connect] emitted, waiting for the transport
    Connecting,
    /// Transport is up, exchanging GetDeviceInfo / DeviceInfo / LinkSetup / LinkReady
    LinkSetup,
    /// Data can be exchanged
    Up,
}

enum Flow {
    Continue,
    Exit(EventLoopExitReason),
}

pub(crate) struct Core {
    phase: Phase,
    dispatcher: RxDispatcher,
    output: VecDeque<Output>,
    scratch: Vec<u8>,

    // Connection
    connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    exited_tx: Option<oneshot::Sender<EventLoopResidual>>,
    client_version: Option<FullVersionOwned>,
    device_api_info: Option<DeviceApiInfo>,
    tracers: Vec<mpsc::UnboundedSender<TraceEvent>>,

    // Timers
    link_setup_retries_left: u32,
    next_link_setup_retry_at: Option<Instant>,
    /// Set when data was sent but not yet flushed, waiting for more messages to fill the frame
    unflushed_since: Option<Instant>,
    /// Requested by a device, how long to accumulate messages into one frame before sending it out
    frame_accumulation_time: Duration,
    next_ping_at: Option<Instant>,
    last_rx_at: Option<Instant>,

    malformed_messages_left: u32,
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

impl Core {
    pub fn new() -> Self {
        Core {
            phase: Phase::Idle,
            dispatcher: RxDispatcher::default(),
            output: VecDeque::new(),
            scratch: vec![0u8; MAX_MESSAGE_SIZE],
            connected_tx: None,
            exited_tx: None,
            client_version: None,
            device_api_info: None,
            tracers: vec![],
            link_setup_retries_left: LINK_SETUP_RETRIES,
            next_link_setup_retry_at: None,
            unflushed_since: None,
            frame_accumulation_time: Duration::from_millis(1),
            next_ping_at: None,
            last_rx_at: None,
            malformed_messages_left: MAX_MALFORMED_MESSAGES,
        }
    }

    pub fn handle(&mut self, now: Instant, input: Input<'_>) {
        let r = match input {
            Input::Command(cmd) => self.on_command(now, cmd),
            Input::CommanderDropped => {
                info!("all command senders were dropped, exiting");
                self.send_disconnect(now, DisconnectReason::CommanderDropped);
                Ok(Flow::Exit(EventLoopExitReason::CommanderDropped))
            }
            Input::TransportUp => self.on_transport_up(now),
            Input::TransportError(e) => {
                self.trace_error(e.clone());
                Err(anyhow!(Error::Transport(e)))
            }
            Input::Message { kind, payload } => self.on_message(now, kind, payload),
            Input::Timer => self.on_timer(now),
        };
        match r {
            Ok(Flow::Continue) => {}
            Ok(Flow::Exit(reason)) => self.exit(Ok(reason)),
            Err(e) => self.exit(Err(e)),
        }
    }

    pub fn poll_output(&mut self) -> Option<Output> {
        self.output.pop_front()
    }

    /// Earliest instant at which [Input::Timer] should be fed, None if nothing is scheduled.
    pub fn poll_timeout(&self) -> Option<Instant> {
        let mut deadlines = [self.dispatcher.next_prune_at(), None, None, None, None];
        match self.phase {
            Phase::LinkSetup => deadlines[1] = self.next_link_setup_retry_at,
            Phase::Up => {
                deadlines[2] = self
                    .unflushed_since
                    .map(|t| t + self.frame_accumulation_time);
                deadlines[3] = self.next_ping_at;
                deadlines[4] = self.last_rx_at.map(|t| t + PEER_TIMEOUT);
            }
            Phase::Idle | Phase::Connecting => {}
        }
        deadlines.into_iter().flatten().min()
    }

    /// Everything the client needs after [Output::Exit] to report the result or re-connect later.
    pub fn into_residual(
        self,
        cmd_rx: mpsc::Receiver<Command>,
        result: anyhow::Result<EventLoopExitReason>,
    ) -> (
        Option<oneshot::Sender<EventLoopResidual>>,
        EventLoopResidual,
    ) {
        (
            self.exited_tx,
            EventLoopResidual {
                cmd_rx,
                connected_tx: self.connected_tx,
                result,
            },
        )
    }

    // Inputs

    fn on_command(&mut self, now: Instant, cmd: Command) -> anyhow::Result<Flow> {
        match cmd {
            Command::Connect {
                handle,
                client_version,
                connected_tx,
                failed_tx,
            } => {
                if self.phase != Phase::Idle {
                    warn!("ignoring Connect while already connected");
                    if let Some(tx) = connected_tx {
                        _ = tx.send(ConnectionInfo::err(anyhow!("already connected")));
                    }
                    return Ok(Flow::Continue);
                }
                self.connected_tx = connected_tx;
                self.exited_tx = failed_tx;
                self.client_version = Some(*client_version);
                self.phase = Phase::Connecting;
                self.output.push_back(Output::Connect(handle));
            }
            Command::RegisterTracer { trace_event_tx } => {
                self.tracers.push(trace_event_tx);
            }
            Command::DisconnectKeepStreams {
                disconnected_tx,
                reason,
            } => {
                info!("disconnecting on user request (but keeping streams ready for re-use)");
                self.trace_disconnect("client request", true);
                self.send_disconnect(now, reason);
                if let Some(tx) = disconnected_tx {
                    _ = tx.send(());
                }
                return Ok(Flow::Exit(
                    EventLoopExitReason::DisconnectKeepStreamsCommand,
                ));
            }
            Command::DisconnectAndExit {
                disconnected_tx,
                reason,
            } => {
                info!("disconnecting and stopping event loop on user request");
                self.trace_disconnect("client request", false);
                self.send_disconnect(now, reason);
                if let Some(tx) = disconnected_tx {
                    _ = tx.send(());
                }
                return Ok(Flow::Exit(EventLoopExitReason::DisconnectCommand));
            }
            Command::SendMessage { bytes, done_tx } => {
                self.on_send_message(now, bytes, done_tx)?;
            }
            Command::OnStreamEvent {
                path_kind,
                stream_event_tx,
            } => {
                self.dispatcher
                    .handle_cmd(DispatcherCommand::OnStreamEvent {
                        path_kind: *path_kind,
                        stream_event_tx,
                    });
            }
            Command::LoopbackTest { progress_tx, .. } => {
                // TODO: port loopback test onto the sans-IO core
                _ = progress_tx.send(TestProgress::FatalError(
                    "loopback test is not supported by this event loop yet".into(),
                ));
            }
        }
        Ok(Flow::Continue)
    }

    fn on_send_message(
        &mut self,
        now: Instant,
        mut bytes: Vec<u8>,
        done_tx: Option<(oneshot::Sender<Result<Vec<u8>, Error>>, Duration)>,
    ) -> anyhow::Result<()> {
        if self.phase != Phase::Up {
            warn!("ignoring SendMessage while disconnected");
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(Error::Disconnected));
            }
            return Ok(());
        }
        if let Some((done_tx, timeout)) = done_tx {
            if let Some(seq) = self.dispatcher.next_seq() {
                // NOTE: this is the only use of Request in this crate, a bit unfortunate to mix it in here, but otherwise
                // every CommandSender have to get a unique seq number somehow and previous implementation that was doing that
                // was much uglier and had limitations (see the last use of it at git sha: 0113fa4)
                ww_client_server::Request::set_seq(&mut bytes, seq as SeqTy);
                self.dispatcher.handle_cmd(DispatcherCommand::OnReturn {
                    seq,
                    done_tx,
                    timeout,
                });
            } else {
                // TODO: backpressure when out of request IDs
                _ = done_tx.send(Err(Error::Other("No more request IDs available".into())));
                return Ok(());
            }
        }
        self.trace_request(&bytes);
        self.output.push_back(Output::Send {
            kind: ww_link::Kind::Data0 as u8,
            payload: bytes,
        });
        if self.unflushed_since.is_none() {
            self.unflushed_since = Some(now);
        }
        Ok(())
    }

    fn on_transport_up(&mut self, now: Instant) -> anyhow::Result<Flow> {
        if self.phase != Phase::Connecting {
            warn!("unexpected TransportUp in {:?}", self.phase);
            return Ok(Flow::Continue);
        }
        debug!("transport up");
        self.phase = Phase::LinkSetup;
        self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
        self.link_setup_retries_left = LINK_SETUP_RETRIES;
        self.next_link_setup_retry_at = Some(now + LINK_SETUP_RETRY_INTERVAL);
        self.send_get_device_info(now)?;
        Ok(Flow::Continue)
    }

    fn on_timer(&mut self, now: Instant) -> anyhow::Result<Flow> {
        self.dispatcher.prune(now);
        match self.phase {
            Phase::LinkSetup => {
                if is_due(self.next_link_setup_retry_at, now) {
                    if self.link_setup_retries_left > 0 {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        self.link_setup_retries_left -= 1;
                        self.next_link_setup_retry_at = Some(now + LINK_SETUP_RETRY_INTERVAL);
                        self.send_get_device_info(now)?;
                    } else {
                        error!("exiting, because link setup failed after several retries");
                        return Err(anyhow!(Error::LinkSetupTimeout));
                    }
                }
            }
            Phase::Up => {
                if is_due(self.last_rx_at.map(|t| t + PEER_TIMEOUT), now) {
                    warn!("nothing received from device for {PEER_TIMEOUT:?}, exiting");
                    return Err(anyhow!(Error::NoPingFromDevice));
                }
                let accumulated_due = is_due(
                    self.unflushed_since
                        .map(|t| t + self.frame_accumulation_time),
                    now,
                );
                let ping_due = is_due(self.next_ping_at, now);
                if accumulated_due {
                    trace!(
                        "flushing accumulated messages after {}us",
                        self.unflushed_since
                            .map(|t| (now - t).as_micros())
                            .unwrap_or(0)
                    );
                    self.flush(now);
                } else if ping_due {
                    trace!("sending ping");
                    self.send(&Message::Ping)?;
                    self.flush(now);
                }
            }
            Phase::Idle | Phase::Connecting => {}
        }
        Ok(Flow::Continue)
    }

    fn on_message(&mut self, now: Instant, kind: u8, payload: &[u8]) -> anyhow::Result<Flow> {
        if matches!(self.phase, Phase::Idle | Phase::Connecting) {
            warn!("message received while transport is down, ignoring");
            return Ok(Flow::Continue);
        }
        self.last_rx_at = Some(now);
        match Message::decode(kind, payload) {
            Ok(msg) => self.on_link_message(now, msg),
            Err(ww_link::Error::UnknownKind(kind)) => {
                warn!("unknown link message kind {kind}, ignoring");
                Ok(Flow::Continue)
            }
            Err(e) => {
                self.trace_error(format!("{e:?}"));
                if self.malformed_messages_left > 0 {
                    warn!(
                        "malformed link message {e:?}, probably old message from previous session or link version mismatch?"
                    );
                    self.malformed_messages_left -= 1;
                    Ok(Flow::Continue)
                } else {
                    Err(anyhow!(Error::Transport(format!("{e:?}"))))
                }
            }
        }
    }

    fn on_link_message(&mut self, now: Instant, msg: Message<'_>) -> anyhow::Result<Flow> {
        trace!("link: {msg:?}");
        match msg {
            Message::Data { bytes, .. } => {
                if self.phase != Phase::Up {
                    warn!("got data before link is up, ignoring");
                    return Ok(Flow::Continue);
                }
                if bytes.is_empty() {
                    warn!("got empty event data, ignoring");
                    return Ok(Flow::Continue);
                }
                self.trace_event(bytes);
                self.dispatcher
                    .handle_msg(DispatcherMessage::MessageBytes(bytes));
            }
            Message::Disconnect(reason) => {
                self.trace_disconnect(format!("remote: {reason:?}").as_str(), false);
                if self.phase != Phase::Up && reason != DisconnectReason::IncompatibleVersion {
                    warn!(
                        "received Disconnect({reason:?}) from remote device, ignoring, must be from old session"
                    );
                    return Ok(Flow::Continue);
                }
                if reason == DisconnectReason::IncompatibleVersion
                    || reason == DisconnectReason::CommanderDropped
                {
                    error!("received Disconnect({reason:?}), exiting");
                } else {
                    info!("received Disconnect({reason:?}) from remote device, exiting");
                }
                return Ok(Flow::Exit(EventLoopExitReason::DisconnectFromDevice));
            }
            Message::Ping | Message::Nop => {}
            Message::DeviceInfo(info) => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected DeviceInfo in {:?}, ignoring", self.phase);
                    return Ok(Flow::Continue);
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
                self.frame_accumulation_time =
                    Duration::from_micros(info.packet_accumulation_time_us as u64);
                self.device_api_info = Some(device_api_info);
                let client_version = self
                    .client_version
                    .clone()
                    .unwrap_or(FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)));
                self.send(&Message::LinkSetup(LinkSetup {
                    host_user_version: client_version.as_ref(),
                    host_max_message_len: MAX_MESSAGE_SIZE as u32,
                }))?;
                self.flush(now);
            }
            Message::LinkReady => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected LinkReady in {:?}, ignoring", self.phase);
                    return Ok(Flow::Continue);
                }
                info!("link setup complete");
                self.phase = Phase::Up;
                self.next_link_setup_retry_at = None;
                self.next_ping_at = Some(now + PING_INTERVAL);
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
                self.trace_connected();
            }
            Message::Loopback { .. } => {} // ignore when not testing
            Message::GetDeviceInfo
            | Message::LinkSetup(_)
            | Message::GetStats
            | Message::Stats(_) => {
                warn!("ignoring device-only or unsupported message: {msg:?}");
            }
        }
        Ok(Flow::Continue)
    }

    // Helpers

    /// Encode a link message and queue it for the wrapper's framer.
    fn send(&mut self, msg: &Message<'_>) -> Result<(), Error> {
        trace!("tx: {msg:?}");
        let (kind, payload) = msg
            .encode(&mut self.scratch)
            .map_err(|e| Error::Transport(format!("{e:?}")))?;
        self.output.push_back(Output::Send {
            kind,
            payload: payload.to_vec(),
        });
        Ok(())
    }

    /// Ask the wrapper to send the current frame; also restarts the ping timer.
    fn flush(&mut self, now: Instant) {
        self.output.push_back(Output::Flush);
        self.unflushed_since = None;
        if self.phase == Phase::Up {
            self.next_ping_at = Some(now + PING_INTERVAL);
        }
    }

    fn send_get_device_info(&mut self, now: Instant) -> anyhow::Result<()> {
        // Nop is flushed alone first: if USB data toggle bits are messed up after re-connection,
        // the first packet might get lost and this ensures it's not GetDeviceInfo.
        self.send(&Message::Nop)?;
        self.flush(now);
        self.send(&Message::GetDeviceInfo)?;
        self.flush(now);
        Ok(())
    }

    /// Best effort, errors are ignored as we are going down anyway.
    fn send_disconnect(&mut self, now: Instant, reason: DisconnectReason) {
        if matches!(self.phase, Phase::LinkSetup | Phase::Up) {
            _ = self.send(&Message::Disconnect(reason));
            self.flush(now);
        }
    }

    fn exit(&mut self, result: anyhow::Result<EventLoopExitReason>) {
        match &result {
            Ok(r) => debug!("exiting with: {r:?}"),
            Err(e) => error!("exiting with: {e:?}"),
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
        self.unflushed_since = None;
        self.next_ping_at = None;
        self.last_rx_at = None;
        self.next_link_setup_retry_at = None;
        self.output.push_back(Output::Exit(result));
    }

    // Tracing

    fn trace(&mut self, event: impl Fn() -> TraceEvent) {
        self.tracers.retain(|tx| tx.send(event()).is_ok());
    }

    fn trace_connected(&mut self) {
        self.trace(|| TraceEvent::Connected {});
    }

    fn trace_request(&mut self, bytes: &[u8]) {
        self.trace(|| TraceEvent::Request {
            bytes: bytes.to_vec(),
        });
    }

    fn trace_event(&mut self, bytes: &[u8]) {
        self.trace(|| TraceEvent::Event {
            bytes: bytes.to_vec(),
        });
    }

    fn trace_disconnect(&mut self, reason: &str, keep_streams: bool) {
        self.trace(|| TraceEvent::Disconnected {
            reason: reason.to_string(),
            keep_streams,
        });
    }

    fn trace_error(&mut self, reason: String) {
        self.trace(|| TraceEvent::Error {
            reason: reason.clone(),
        });
    }
}

fn is_due(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_some_and(|d| d.saturating_duration_since(now) < TIMER_TOLERANCE)
}

fn compact_to_full(v: &ww_version::CompactVersion) -> FullVersionOwned {
    FullVersionOwned::new(
        format!("G{}", v.gid.id.0),
        VersionOwned::new(v.major.0, v.minor.0, v.patch.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_link::{DeviceInfo, Kind};
    use ww_version::{ApiHashPair, CompactVersion, FullVersion, Version};

    /// What the wrapper would have pushed into a framer: messages, grouped by Flush.
    #[derive(Debug, PartialEq, Eq)]
    struct Sent {
        kind: Kind,
        payload: Vec<u8>,
        /// Frame index this message went into
        frame: usize,
    }

    /// Drain all outputs: Send messages (tagged with the index of the Flush that followed them) and everything else.
    fn drain_all(core: &mut Core) -> (Vec<Sent>, usize, Vec<Output>) {
        let mut sent = vec![];
        let mut other = vec![];
        let mut flushes = 0;
        while let Some(o) = core.poll_output() {
            match o {
                Output::Send { kind, payload } => sent.push(Sent {
                    kind: Kind::from_repr(kind).unwrap(),
                    payload,
                    frame: flushes,
                }),
                Output::Flush => flushes += 1,
                o => other.push(o),
            }
        }
        (sent, flushes, other)
    }

    fn drain(core: &mut Core) -> (Vec<Sent>, Vec<Output>) {
        let (sent, _, other) = drain_all(core);
        (sent, other)
    }

    fn kinds(sent: &[Sent]) -> Vec<Kind> {
        sent.iter().map(|s| s.kind).collect()
    }

    fn feed(core: &mut Core, now: Instant, msg: &Message<'_>) {
        let mut scratch = [0u8; 256];
        let (kind, payload) = msg.encode(&mut scratch).unwrap();
        core.handle(now, Input::Message { kind, payload });
    }

    fn connect(core: &mut Core, now: Instant) -> oneshot::Receiver<ConnectionInfo> {
        let (connected_tx, connected_rx) = oneshot::channel();
        core.handle(
            now,
            Input::Command(Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "test".into(),
                    VersionOwned::new(0, 1, 0),
                )),
                connected_tx: Some(connected_tx),
                failed_tx: None,
            }),
        );
        let (sent, other) = drain(core);
        assert!(sent.is_empty());
        assert!(matches!(other.as_slice(), [Output::Connect(_)]));
        assert_eq!(core.poll_timeout(), None);

        core.handle(now, Input::TransportUp);
        let (sent, other) = drain(core);
        assert!(other.is_empty());
        assert_eq!(kinds(&sent), [Kind::Nop, Kind::GetDeviceInfo]);
        // Nop and GetDeviceInfo are flushed separately
        assert_eq!((sent[0].frame, sent[1].frame), (0, 1));
        assert_eq!(core.poll_timeout(), Some(now + LINK_SETUP_RETRY_INTERVAL));

        feed(
            core,
            now,
            &Message::DeviceInfo(DeviceInfo {
                dev_link_version: CompactVersion::new(ww_version::GlobalTypeId::new(512), 0, 1, 0),
                api_model_version: CompactVersion::new(ww_version::GlobalTypeId::new(513), 0, 2, 0),
                user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
                hash: ApiHashPair::empty(),
                dev_max_message_len: 512,
                packet_accumulation_time_us: 500,
            }),
        );
        let (sent, _) = drain(core);
        assert_eq!(kinds(&sent), [Kind::LinkSetup]);
        let Message::LinkSetup(setup) =
            Message::decode(Kind::LinkSetup as u8, &sent[0].payload).unwrap()
        else {
            panic!()
        };
        assert_eq!(setup.host_max_message_len, MAX_MESSAGE_SIZE as u32);
        assert_eq!(setup.host_user_version.crate_id, "test");

        feed(core, now, &Message::LinkReady);
        assert!(drain(core).0.is_empty());
        connected_rx
    }

    #[test]
    fn link_setup_then_data_and_ping() {
        let now = Instant::now();
        let mut core = Core::new();
        let mut connected_rx = connect(&mut core, now);
        let info = connected_rx.try_recv().unwrap().result.unwrap();
        assert_eq!(info.max_message_size, 512);
        assert_eq!(core.poll_timeout(), Some(now + PING_INTERVAL));

        // two requests: written to the framer right away, but not flushed until the accumulation window ends
        for bytes in [vec![0, 0, 1], vec![0, 0, 2]] {
            core.handle(
                now,
                Input::Command(Command::SendMessage {
                    bytes,
                    done_tx: None,
                }),
            );
        }
        let (sent, _) = drain(&mut core);
        assert_eq!(kinds(&sent), [Kind::Data0, Kind::Data0]);
        assert_eq!((sent[0].frame, sent[1].frame), (0, 0)); // no Flush yet
        assert_eq!(core.poll_timeout(), Some(now + Duration::from_micros(500)));

        let now = now + Duration::from_micros(500);
        core.handle(now, Input::Timer);
        let (sent, flushes, other) = drain_all(&mut core);
        assert!(sent.is_empty() && other.is_empty());
        assert_eq!(flushes, 1, "accumulation window ended: exactly one Flush");
        // and the ping timer is restarted by the flush
        assert_eq!(core.poll_timeout(), Some(now + PING_INTERVAL));

        // nothing else to send: ping
        let now = now + PING_INTERVAL;
        core.handle(now, Input::Timer);
        let (sent, _) = drain(&mut core);
        assert_eq!(kinds(&sent), [Kind::Ping]);

        // silent device eventually times out
        let now = now + PEER_TIMEOUT;
        core.handle(now, Input::Timer);
        let (_, other) = drain(&mut core);
        assert!(matches!(other.as_slice(), [Output::Exit(Err(_))]));
    }

    #[test]
    fn flush_is_emitted_after_accumulation_window() {
        let now = Instant::now();
        let mut core = Core::new();
        connect(&mut core, now);
        core.handle(
            now,
            Input::Command(Command::SendMessage {
                bytes: vec![0, 0, 1],
                done_tx: None,
            }),
        );
        drain(&mut core);
        core.handle(now + Duration::from_micros(500), Input::Timer);
        assert!(matches!(core.poll_output(), Some(Output::Flush)));
        assert!(core.poll_output().is_none());
    }

    #[test]
    fn request_response_via_dispatcher() {
        let now = Instant::now();
        let mut core = Core::new();
        connect(&mut core, now);

        let (done_tx, mut done_rx) = oneshot::channel();
        core.handle(
            now,
            Input::Command(Command::SendMessage {
                bytes: vec![0, 0, 0xAA],
                done_tx: Some((done_tx, Duration::from_secs(1))),
            }),
        );
        let (sent, _) = drain(&mut core);
        let seq = u16::from_le_bytes([sent[0].payload[0], sent[0].payload[1]]);
        assert_ne!(seq, 0);

        // Event { seq, Value { data: [7] } }
        let event = ww_client_server::Event {
            seq,
            result: Ok(ww_client_server::EventKind::Value {
                data: wire_weaver::shrink_wrap::tail_bytes::TailBytes(&[7]),
            }),
        };
        let mut buf = [0u8; 32];
        let bytes =
            wire_weaver::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(&event, &mut buf).unwrap();
        feed(&mut core, now, &Message::Data { channel: 0, bytes });
        assert_eq!(done_rx.try_recv().unwrap().unwrap(), vec![7]);
    }

    #[test]
    fn disconnect_from_device() {
        let now = Instant::now();
        let mut core = Core::new();
        connect(&mut core, now);
        feed(
            &mut core,
            now,
            &Message::Disconnect(DisconnectReason::RequestByUser),
        );
        let (_, other) = drain(&mut core);
        assert!(matches!(
            other.as_slice(),
            [Output::Exit(Ok(EventLoopExitReason::DisconnectFromDevice))]
        ));
    }

    #[test]
    fn disconnect_command_sends_disconnect_then_exits() {
        let now = Instant::now();
        let mut core = Core::new();
        connect(&mut core, now);
        core.handle(
            now,
            Input::Command(Command::DisconnectAndExit {
                disconnected_tx: None,
                reason: DisconnectReason::RequestByUser,
            }),
        );
        let (sent, other) = drain(&mut core);
        assert_eq!(kinds(&sent), [Kind::Disconnect]);
        assert!(matches!(
            other.as_slice(),
            [Output::Exit(Ok(EventLoopExitReason::DisconnectCommand))]
        ));
    }

    #[test]
    fn link_setup_retries_then_fails() {
        let mut now = Instant::now();
        let mut core = Core::new();
        core.handle(
            now,
            Input::Command(Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "".into(),
                    VersionOwned::new(0, 0, 0),
                )),
                connected_tx: None,
                failed_tx: None,
            }),
        );
        drain(&mut core);
        core.handle(now, Input::TransportUp);
        drain(&mut core);
        for _ in 0..LINK_SETUP_RETRIES {
            now += LINK_SETUP_RETRY_INTERVAL;
            core.handle(now, Input::Timer);
            let (sent, other) = drain(&mut core);
            assert_eq!(kinds(&sent), [Kind::Nop, Kind::GetDeviceInfo]);
            assert!(other.is_empty());
        }
        now += LINK_SETUP_RETRY_INTERVAL;
        core.handle(now, Input::Timer);
        let (_, other) = drain(&mut core);
        assert!(matches!(other.as_slice(), [Output::Exit(Err(_))]));
    }
}
