//! Sans-IO event loop core: all the protocol logic of a client event loop without any IO,
//! timers or channels of its own.
//!
//! Intended use (both async and blocking wrappers look the same):
//! ```ignore
//! loop {
//!     while let Some(output) = core.poll_output() {
//!         match output {
//!             Output::Connect(handle) => { /* open transport, then Input::TransportUp / TransportError */ }
//!             Output::SendFrame(frame) => { /* write to transport */ }
//!             Output::Exit(result) => { /* stop */ }
//!         }
//!     }
//!     // wait for a command, a frame or core.poll_timeout(), whichever comes first
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
use ww_link::{DisconnectReason, LinkSetup, Message, RxOwned, TxOwned};
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
    /// `frame_size` is the maximum frame (e.g., USB packet) the transport can carry.
    TransportUp { frame_size: usize },
    /// Transport could not be opened or failed while in use.
    TransportError(String),
    /// Frame received from the transport, exactly as it came (frame boundaries are significant).
    Frame(&'i [u8]),
    /// In response to [Core::poll_timeout] deadline.
    Timer,
}

pub(crate) enum Output {
    /// Open a transport to the device behind this handle (from [Command::Connect]),
    /// then feed [Input::TransportUp] or [Input::TransportError].
    Connect(DeviceHandle),
    /// Frame to send to the device.
    SendFrame(Vec<u8>),
    /// Send out all preceding frames, close the transport and stop.
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

/// Framer + encode scratch, separate from rx to allow replying while holding a received message.
struct LinkTx {
    tx: TxOwned,
    scratch: Vec<u8>,
}

struct Link {
    tx: LinkTx,
    rx: RxOwned,
}

pub(crate) struct Core {
    phase: Phase,
    link: Option<Link>,
    dispatcher: RxDispatcher,
    output: VecDeque<Output>,

    // Connection
    connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    exited_tx: Option<oneshot::Sender<EventLoopResidual>>,
    client_version: Option<FullVersionOwned>,
    device_api_info: Option<DeviceApiInfo>,
    tracers: Vec<mpsc::UnboundedSender<TraceEvent>>,

    // Timers
    link_setup_retries_left: u32,
    next_link_setup_retry_at: Option<Instant>,
    /// Set when a partially filled frame is waiting for more messages
    frame_started_at: Option<Instant>,
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
            link: None,
            dispatcher: RxDispatcher::default(),
            output: VecDeque::new(),
            connected_tx: None,
            exited_tx: None,
            client_version: None,
            device_api_info: None,
            tracers: vec![],
            link_setup_retries_left: LINK_SETUP_RETRIES,
            next_link_setup_retry_at: None,
            frame_started_at: None,
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
                self.send_disconnect(DisconnectReason::CommanderDropped);
                Ok(Flow::Exit(EventLoopExitReason::CommanderDropped))
            }
            Input::TransportUp { frame_size } => self.on_transport_up(now, frame_size),
            Input::TransportError(e) => {
                self.trace_error(e.clone());
                Err(anyhow!(Error::Transport(e)))
            }
            Input::Frame(frame) => self.on_frame(now, frame),
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
                    .frame_started_at
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
                self.send_disconnect(reason);
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
                self.send_disconnect(reason);
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
        let link = self.link.as_mut().ok_or(Error::Disconnected)?;
        link.tx.send(
            &Message::Data {
                channel: 0,
                bytes: &bytes,
            },
            false,
            &mut self.output,
        )?;
        if link.tx.tx.is_empty() {
            self.frame_started_at = None;
        } else if self.frame_started_at.is_none() {
            self.frame_started_at = Some(now);
        }
        Ok(())
    }

    fn on_transport_up(&mut self, now: Instant, frame_size: usize) -> anyhow::Result<Flow> {
        if self.phase != Phase::Connecting {
            warn!("unexpected TransportUp in {:?}", self.phase);
            return Ok(Flow::Continue);
        }
        debug!("transport up, frame size: {frame_size}");
        self.link = Some(Link {
            tx: LinkTx {
                tx: TxOwned::new(frame_size),
                scratch: vec![0u8; MAX_MESSAGE_SIZE],
            },
            rx: RxOwned::new(MAX_MESSAGE_SIZE + frame_size),
        });
        self.phase = Phase::LinkSetup;
        self.malformed_messages_left = MAX_MALFORMED_MESSAGES;
        self.link_setup_retries_left = LINK_SETUP_RETRIES;
        self.next_link_setup_retry_at = Some(now + LINK_SETUP_RETRY_INTERVAL);
        self.send_get_device_info()?;
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
                        self.send_get_device_info()?;
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
                    self.frame_started_at
                        .map(|t| t + self.frame_accumulation_time),
                    now,
                );
                let ping_due = is_due(self.next_ping_at, now);
                if accumulated_due || ping_due {
                    let link = self.link.as_mut().ok_or(Error::Disconnected)?;
                    if link.tx.tx.is_empty() {
                        trace!("sending ping");
                        link.tx.send(&Message::Ping, true, &mut self.output)?;
                    } else {
                        trace!(
                            "sending accumulated frame {}us",
                            self.frame_started_at
                                .map(|t| (now - t).as_micros())
                                .unwrap_or(0)
                        );
                        link.tx.flush(&mut self.output);
                    }
                    self.frame_started_at = None;
                    self.next_ping_at = Some(now + PING_INTERVAL);
                }
            }
            Phase::Idle | Phase::Connecting => {}
        }
        Ok(Flow::Continue)
    }

    fn on_frame(&mut self, now: Instant, frame: &[u8]) -> anyhow::Result<Flow> {
        let Some(mut link) = self.link.take() else {
            warn!("frame received while transport is down, ignoring");
            return Ok(Flow::Continue);
        };
        let r = self.process_frame(now, &mut link, frame);
        self.link = Some(link);
        r
    }

    fn process_frame(
        &mut self,
        now: Instant,
        link: &mut Link,
        frame: &[u8],
    ) -> anyhow::Result<Flow> {
        trace!("rx frame: {}: {:02x?}", frame.len(), frame);
        self.last_rx_at = Some(now);
        if link.rx.stage(frame).is_err() {
            warn!("rx assembly buffer overflow, dropping frame");
            self.trace_error("rx assembly buffer overflow".into());
            return Ok(Flow::Continue);
        }
        loop {
            link.rx.reassemble();
            let Some((kind, bytes)) = link.rx.message() else {
                break;
            };
            let flow = match Message::decode(kind, bytes) {
                Ok(msg) => self.on_link_message(now, &mut link.tx, msg)?,
                Err(ww_link::Error::UnknownKind(kind)) => {
                    warn!("unknown link message kind {kind}, ignoring");
                    Flow::Continue
                }
                Err(e) => {
                    self.trace_error(format!("{e:?}"));
                    if self.malformed_messages_left > 0 {
                        warn!(
                            "malformed link message {e:?}, probably old message from previous session or link version mismatch?"
                        );
                        self.malformed_messages_left -= 1;
                        Flow::Continue
                    } else {
                        return Err(anyhow!(Error::Transport(format!("{e:?}"))));
                    }
                }
            };
            if let Flow::Exit(reason) = flow {
                return Ok(Flow::Exit(reason));
            }
        }
        Ok(Flow::Continue)
    }

    fn on_link_message(
        &mut self,
        now: Instant,
        tx: &mut LinkTx,
        msg: Message<'_>,
    ) -> anyhow::Result<Flow> {
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
                tx.send(
                    &Message::LinkSetup(LinkSetup {
                        host_user_version: client_version.as_ref(),
                        host_max_message_len: MAX_MESSAGE_SIZE as u32,
                    }),
                    true,
                    &mut self.output,
                )?;
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

    fn send_get_device_info(&mut self) -> anyhow::Result<()> {
        let link = self.link.as_mut().ok_or(Error::Disconnected)?;
        // Nop is forced out alone first: if USB data toggle bits are messed up after re-connection,
        // the first packet might get lost and this ensures it's not GetDeviceInfo.
        link.tx.send(&Message::Nop, true, &mut self.output)?;
        link.tx
            .send(&Message::GetDeviceInfo, true, &mut self.output)?;
        Ok(())
    }

    /// Best effort, errors are ignored as we are going down anyway.
    fn send_disconnect(&mut self, reason: DisconnectReason) {
        if let Some(link) = self.link.as_mut() {
            _ = link
                .tx
                .send(&Message::Disconnect(reason), true, &mut self.output);
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
        self.link = None;
        self.frame_started_at = None;
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

impl LinkTx {
    /// Write a message into the current frame, emitting full frames along the way.
    /// With `force`, the frame is emitted right away even if not full.
    fn send(
        &mut self,
        msg: &Message<'_>,
        force: bool,
        out: &mut VecDeque<Output>,
    ) -> Result<(), Error> {
        let (kind, payload) = msg
            .encode(&mut self.scratch)
            .map_err(|e| Error::Transport(format!("{e:?}")))?;
        loop {
            match self.tx.write(kind, payload) {
                Ok(true) => break,
                Ok(false) => {
                    if !Self::flush_inner(&mut self.tx, out) {
                        // nothing was written and nothing fits: cannot make progress
                        return Err(Error::Transport("message does not fit into a frame".into()));
                    }
                }
                Err(()) => {
                    return Err(Error::Transport(format!(
                        "message of {} bytes is too big for the link",
                        payload.len()
                    )));
                }
            }
        }
        if force {
            self.flush(out);
        }
        Ok(())
    }

    fn flush(&mut self, out: &mut VecDeque<Output>) {
        Self::flush_inner(&mut self.tx, out);
    }

    fn flush_inner(tx: &mut TxOwned, out: &mut VecDeque<Output>) -> bool {
        match tx.flush_to_vec() {
            Some(frame) => {
                trace!("tx frame: {}: {:02x?}", frame.len(), frame);
                out.push_back(Output::SendFrame(frame));
                true
            }
            None => false,
        }
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
    use ww_link::{DeviceInfo, Kind, RxOwned, TxOwned};
    use ww_version::{ApiHashPair, CompactVersion, FullVersion, Version};

    const FRAME: usize = 64;

    /// Fake device side: framer pair + what it received, decoded.
    struct Device {
        tx: TxOwned,
        rx: RxOwned,
    }

    impl Device {
        fn new() -> Self {
            Device {
                tx: TxOwned::new(FRAME),
                rx: RxOwned::new(MAX_MESSAGE_SIZE + FRAME),
            }
        }

        /// Feed frames from host, return kinds of decoded messages (payload of Data is kept).
        fn receive(&mut self, frames: Vec<Vec<u8>>) -> Vec<(Kind, Vec<u8>)> {
            let mut out = vec![];
            for f in frames {
                self.rx.stage(&f).unwrap();
                loop {
                    self.rx.reassemble();
                    let Some((k, b)) = self.rx.message() else {
                        break;
                    };
                    out.push((Kind::from_repr(k).unwrap(), b.to_vec()));
                }
            }
            out
        }

        fn frame(&mut self, msg: &Message<'_>) -> Vec<u8> {
            let mut scratch = [0u8; 256];
            let (k, p) = msg.encode(&mut scratch).unwrap();
            assert_eq!(self.tx.write(k, p), Ok(true));
            self.tx.flush_to_vec().unwrap()
        }
    }

    fn drain(core: &mut Core) -> (Vec<Vec<u8>>, Vec<Output>) {
        let mut frames = vec![];
        let mut other = vec![];
        while let Some(o) = core.poll_output() {
            match o {
                Output::SendFrame(f) => frames.push(f),
                o => other.push(o),
            }
        }
        (frames, other)
    }

    fn connect(
        core: &mut Core,
        dev: &mut Device,
        now: Instant,
    ) -> oneshot::Receiver<ConnectionInfo> {
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
        let (frames, other) = drain(core);
        assert!(frames.is_empty());
        assert!(matches!(other.as_slice(), [Output::Connect(_)]));
        assert_eq!(core.poll_timeout(), None);

        core.handle(now, Input::TransportUp { frame_size: FRAME });
        let (frames, other) = drain(core);
        assert!(other.is_empty());
        let kinds: Vec<_> = dev.receive(frames).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, [Kind::Nop, Kind::GetDeviceInfo]);
        assert_eq!(core.poll_timeout(), Some(now + LINK_SETUP_RETRY_INTERVAL));

        let f = dev.frame(&Message::DeviceInfo(DeviceInfo {
            dev_link_version: CompactVersion::new(ww_version::GlobalTypeId::new(512), 0, 1, 0),
            api_model_version: CompactVersion::new(ww_version::GlobalTypeId::new(513), 0, 2, 0),
            user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 512,
            packet_accumulation_time_us: 500,
        }));
        core.handle(now, Input::Frame(&f));
        let (frames, _) = drain(core);
        let got = dev.receive(frames);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, Kind::LinkSetup);
        let setup = Message::decode(Kind::LinkSetup as u8, &got[0].1).unwrap();
        let Message::LinkSetup(setup) = setup else {
            panic!()
        };
        assert_eq!(setup.host_max_message_len, MAX_MESSAGE_SIZE as u32);
        assert_eq!(setup.host_user_version.crate_id, "test");

        let f = dev.frame(&Message::LinkReady);
        core.handle(now, Input::Frame(&f));
        assert!(drain(core).0.is_empty());
        connected_rx
    }

    #[test]
    fn link_setup_then_data_and_ping() {
        let now = Instant::now();
        let mut core = Core::new();
        let mut dev = Device::new();
        let mut connected_rx = connect(&mut core, &mut dev, now);
        let info = connected_rx.try_recv().unwrap().result.unwrap();
        assert_eq!(info.max_message_size, 512);
        assert_eq!(core.poll_timeout(), Some(now + PING_INTERVAL));

        // request without response expected: accumulated, not sent right away
        core.handle(
            now,
            Input::Command(Command::SendMessage {
                bytes: vec![0, 0, 1, 2, 3],
                done_tx: None,
            }),
        );
        assert!(drain(&mut core).0.is_empty());
        assert_eq!(core.poll_timeout(), Some(now + Duration::from_micros(500)));

        // accumulation timer fires: frame goes out
        let now = now + Duration::from_micros(500);
        core.handle(now, Input::Timer);
        let (frames, _) = drain(&mut core);
        assert_eq!(dev.receive(frames), [(Kind::Data0, vec![0, 0, 1, 2, 3])]);

        // nothing else to send: ping
        let now = now + PING_INTERVAL;
        core.handle(now, Input::Timer);
        let (frames, _) = drain(&mut core);
        assert_eq!(dev.receive(frames), [(Kind::Ping, vec![])]);

        // silent device eventually times out
        let now = now + PEER_TIMEOUT;
        core.handle(now, Input::Timer);
        let (_, other) = drain(&mut core);
        assert!(matches!(other.as_slice(), [Output::Exit(Err(_))]));
    }

    #[test]
    fn request_response_via_dispatcher() {
        let now = Instant::now();
        let mut core = Core::new();
        let mut dev = Device::new();
        connect(&mut core, &mut dev, now);

        let (done_tx, mut done_rx) = oneshot::channel();
        core.handle(
            now,
            Input::Command(Command::SendMessage {
                bytes: vec![0, 0, 0xAA],
                done_tx: Some((done_tx, Duration::from_secs(1))),
            }),
        );
        core.handle(now + Duration::from_millis(1), Input::Timer);
        let (frames, _) = drain(&mut core);
        let got = dev.receive(frames);
        let seq = u16::from_le_bytes([got[0].1[0], got[0].1[1]]);
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
        let f = dev.frame(&Message::Data { channel: 0, bytes });
        core.handle(now, Input::Frame(&f));
        assert_eq!(done_rx.try_recv().unwrap().unwrap(), vec![7]);
    }

    #[test]
    fn disconnect_from_device() {
        let now = Instant::now();
        let mut core = Core::new();
        let mut dev = Device::new();
        connect(&mut core, &mut dev, now);
        let f = dev.frame(&Message::Disconnect(DisconnectReason::RequestByUser));
        core.handle(now, Input::Frame(&f));
        let (_, other) = drain(&mut core);
        assert!(matches!(
            other.as_slice(),
            [Output::Exit(Ok(EventLoopExitReason::DisconnectFromDevice))]
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
        core.handle(now, Input::TransportUp { frame_size: FRAME });
        drain(&mut core);
        for _ in 0..LINK_SETUP_RETRIES {
            now += LINK_SETUP_RETRY_INTERVAL;
            core.handle(now, Input::Timer);
            let (frames, other) = drain(&mut core);
            assert_eq!(frames.len(), 2);
            assert!(other.is_empty());
        }
        now += LINK_SETUP_RETRY_INTERVAL;
        core.handle(now, Input::Timer);
        let (_, other) = drain(&mut core);
        assert!(matches!(other.as_slice(), [Output::Exit(Err(_))]));
    }
}
