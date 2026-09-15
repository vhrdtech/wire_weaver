//! Tx half: commands in, link messages out. Owns link setup retries, the frame accumulation
//! window, pings and seq allocation.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tracing::{debug, error, info, trace, warn};
use ww_link::{DisconnectReason, LinkSetup, Message};
use ww_version::{FullVersionOwned, VersionOwned};

use super::{ToRx, ToTx, Tracers, is_due};
use crate::DEFAULT_MAX_MESSAGE_SIZE;
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::{Command, EventLoopExitReason, EventLoopResidual, TestProgress};
use crate::{Error, SeqTy};

const PING_INTERVAL: Duration = Duration::from_millis(ww_link::PING_INTERVAL_MS);
const LINK_SETUP_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const LINK_SETUP_RETRIES: u32 = 5;

pub(crate) enum TxInput {
    Command(Command),
    /// All command senders were dropped, disconnect and exit.
    CommanderDropped,
    /// Transport requested via [TxOutput::Connect] is open.
    TransportUp,
    /// Transport could not be opened or a write failed.
    TransportError(String),
    FromRx(ToTx),
    /// In response to [TxCore::poll_timeout] deadline.
    Timer,
}

pub(crate) enum TxOutput {
    /// Open a transport to the device behind this handle (from [Command::Connect]),
    /// then feed [TxInput::TransportUp] or [TxInput::TransportError].
    Connect(DeviceHandle),
    /// Link message to write into the framer. Frames that fill up along the way are sent right away,
    /// a partially filled one is kept until [TxOutput::Flush].
    Send {
        kind: u8,
        message: Vec<u8>,
    },
    /// Send the current frame now, even if not full. No-op when there is nothing pending.
    Flush,
    ToRx(ToRx),
    /// Flush and send all preceding messages, close the transport and stop.
    /// [TxCore::into_residual] returns what a wrapper should hand back to the client.
    Exit(anyhow::Result<EventLoopExitReason>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// No transport, waiting for [Command::Connect]
    Idle,
    /// Exited (or connect failed); this core is done and a new event loop must be started to connect again
    Exited,
    /// [TxOutput::Connect] emitted, waiting for the transport
    Connecting,
    /// Sending GetDeviceInfo until rx reports DeviceInfo
    GetDeviceInfo,
    /// LinkSetup sent, waiting for rx to report LinkReady
    LinkSetup,
    /// Data can be exchanged
    Up,
}

enum Flow {
    Continue,
    Exit(EventLoopExitReason),
}

pub(crate) struct TxCore {
    phase: Phase,
    output: VecDeque<TxOutput>,
    scratch: Vec<u8>,
    tracers: Tracers,

    exited_tx: Option<tokio::sync::oneshot::Sender<EventLoopResidual>>,
    client_version: Option<FullVersionOwned>,
    /// Requested by a device, how long to accumulate messages into one frame before sending it out
    frame_accumulation_time: Duration,
    /// From DeviceInfo, requests bigger than this are rejected
    device_max_message_len: usize,

    // Timers
    link_setup_retries_left: u32,
    next_link_setup_retry_at: Option<Instant>,
    /// Set when data was sent but not yet flushed, waiting for more messages to fill the frame
    unflushed_since: Option<Instant>,
    next_ping_at: Option<Instant>,

    // Seq allocation
    next_seq: SeqTy,
    in_flight: HashSet<SeqTy>,
}

impl Default for TxCore {
    fn default() -> Self {
        Self::new()
    }
}

impl TxCore {
    pub fn new() -> Self {
        TxCore {
            phase: Phase::Idle,
            output: VecDeque::new(),
            scratch: vec![0u8; DEFAULT_MAX_MESSAGE_SIZE],
            tracers: Tracers::default(),
            exited_tx: None,
            client_version: None,
            frame_accumulation_time: Duration::from_millis(1),
            device_max_message_len: DEFAULT_MAX_MESSAGE_SIZE,
            link_setup_retries_left: LINK_SETUP_RETRIES,
            next_link_setup_retry_at: None,
            unflushed_since: None,
            next_ping_at: None,
            next_seq: 1,
            in_flight: HashSet::new(),
        }
    }

    pub fn handle(&mut self, now: Instant, input: TxInput) {
        let r = match input {
            TxInput::Command(cmd) => self.on_command(now, cmd),
            TxInput::CommanderDropped => {
                info!("all command senders were dropped, exiting");
                self.send_disconnect(now, DisconnectReason::CommanderDropped);
                Ok(Flow::Exit(EventLoopExitReason::CommanderDropped))
            }
            TxInput::TransportUp => self.on_transport_up(now),
            TxInput::TransportError(e) => {
                self.tracers.error(&e);
                Err(anyhow!(Error::Transport(e)))
            }
            TxInput::FromRx(msg) => self.on_from_rx(now, msg),
            TxInput::Timer => self.on_timer(now),
        };
        match r {
            Ok(Flow::Continue) => {}
            Ok(Flow::Exit(reason)) => self.exit(Ok(reason), true),
            Err(e) => self.exit(Err(e), true),
        }
    }

    pub fn poll_output(&mut self) -> Option<TxOutput> {
        self.output.pop_front()
    }

    /// Earliest instant at which [TxInput::Timer] should be fed, None if nothing is scheduled.
    pub fn poll_timeout(&self) -> Option<Instant> {
        match self.phase {
            Phase::GetDeviceInfo => self.next_link_setup_retry_at,
            Phase::Up => [
                self.unflushed_since
                    .map(|t| t + self.frame_accumulation_time),
                self.next_ping_at,
            ]
            .into_iter()
            .flatten()
            .min(),
            Phase::Idle | Phase::Exited | Phase::Connecting | Phase::LinkSetup => None,
        }
    }

    /// Everything the client needs after [TxOutput::Exit] to report the result or re-connect later.
    /// `connected_tx` is returned by [super::RxCore::into_connected_tx] if it was not consumed.
    pub fn into_residual(
        self,
        cmd_rx: tokio::sync::mpsc::Receiver<Command>,
        connected_tx: Option<tokio::sync::oneshot::Sender<crate::device_info::ConnectionInfo>>,
        result: anyhow::Result<EventLoopExitReason>,
    ) -> (
        Option<tokio::sync::oneshot::Sender<EventLoopResidual>>,
        EventLoopResidual,
    ) {
        (
            self.exited_tx,
            EventLoopResidual {
                cmd_rx,
                connected_tx,
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
                    // proper way to re-connect to the same device, or another one, is to tear down this event loop
                    // then take the residual and start a new one, sending a connect command there
                    let why = if self.phase == Phase::Exited {
                        "event loop already exited, start a new one to connect again"
                    } else {
                        "already connected"
                    };
                    warn!("ignoring Connect: {why}");
                    if let Some(tx) = connected_tx {
                        _ = tx.send(crate::device_info::ConnectionInfo::err(anyhow!(why)));
                    }
                    return Ok(Flow::Continue);
                }
                self.exited_tx = failed_tx;
                self.client_version = Some(*client_version.clone());
                self.phase = Phase::Connecting;
                self.output.push_back(TxOutput::Connect(handle));
                // rx needs these to check DeviceInfo and to report the connection
                self.output.push_back(TxOutput::ToRx(ToRx::TransportUp {
                    client_version,
                    connected_tx,
                }));
            }
            Command::RegisterTracer { trace_event_tx } => {
                self.tracers.register(trace_event_tx.clone());
                self.output
                    .push_back(TxOutput::ToRx(ToRx::RegisterTracer(trace_event_tx)));
            }
            Command::DisconnectKeepStreams {
                disconnected_tx,
                reason,
            } => {
                info!("disconnecting on user request (but keeping streams ready for re-use)");
                self.tracers.disconnected("client request", true);
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
                self.tracers.disconnected("client request", false);
                self.send_disconnect(now, reason);
                if let Some(tx) = disconnected_tx {
                    _ = tx.send(());
                }
                return Ok(Flow::Exit(EventLoopExitReason::DisconnectCommand));
            }
            Command::SendMessage { bytes, done_tx } => {
                self.on_send_message(now, bytes, done_tx);
            }
            Command::OnStreamEvent {
                path_kind,
                stream_event_tx,
            } => {
                self.output.push_back(TxOutput::ToRx(ToRx::OnStreamEvent {
                    path_kind,
                    stream_event_tx,
                }));
            }
            Command::LoopbackTest { progress_tx, .. } => {
                // TODO: port loopback test onto the sans-IO cores
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
        done_tx: Option<(crate::event_loop::rx_dispatcher::ResponseSender, Duration)>,
    ) {
        if self.phase != Phase::Up {
            warn!("ignoring SendMessage while disconnected");
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(Error::Disconnected));
            }
            return;
        }
        if bytes.len() > self.device_max_message_len {
            warn!(
                "request of {} bytes exceeds device max message length {}, dropping",
                bytes.len(),
                self.device_max_message_len
            );
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(Error::Other(
                    "request exceeds device max message length".into(),
                )));
            }
            return;
        }
        if let Some((done_tx, timeout)) = done_tx {
            let Some(seq) = self.next_seq() else {
                // TODO: backpressure when out of request IDs
                _ = done_tx.send(Err(Error::Other("No more request IDs available".into())));
                return;
            };
            // NOTE: this is the only use of Request in this crate, a bit unfortunate to mix it in here, but otherwise
            // every CommandSender have to get a unique seq number somehow and previous implementation that was doing that
            // was much uglier and had limitations (see the last use of it at git sha: 0113fa4)
            ww_client_server::Request::set_seq(&mut bytes, seq);
            // Expect goes out before the request bytes, so rx knows about the seq before the answer can arrive
            self.output.push_back(TxOutput::ToRx(ToRx::Expect {
                seq,
                done_tx,
                timeout,
            }));
        }
        self.tracers.request(&bytes);
        self.output.push_back(TxOutput::Send {
            kind: ww_link::Kind::Data0 as u8,
            message: bytes,
        });
        if self.unflushed_since.is_none() {
            self.unflushed_since = Some(now);
        }
    }

    fn on_transport_up(&mut self, now: Instant) -> anyhow::Result<Flow> {
        if self.phase != Phase::Connecting {
            warn!("unexpected TransportUp in {:?}", self.phase);
            return Ok(Flow::Continue);
        }
        debug!("transport up");
        self.phase = Phase::GetDeviceInfo;
        self.link_setup_retries_left = LINK_SETUP_RETRIES;
        self.next_link_setup_retry_at = Some(now + LINK_SETUP_RETRY_INTERVAL);
        self.send_get_device_info(now)?;
        Ok(Flow::Continue)
    }

    fn on_from_rx(&mut self, now: Instant, msg: ToTx) -> anyhow::Result<Flow> {
        match msg {
            ToTx::DeviceInfo(info) => {
                if self.phase != Phase::GetDeviceInfo {
                    warn!("unexpected DeviceInfo in {:?}, ignoring", self.phase);
                    return Ok(Flow::Continue);
                }
                self.frame_accumulation_time =
                    Duration::from_micros(info.packet_accumulation_time_us as u64);
                self.device_max_message_len = info.dev_max_message_len as usize;
                self.phase = Phase::LinkSetup;
                self.next_link_setup_retry_at = None;
                let client_version = self
                    .client_version
                    .clone()
                    .unwrap_or(FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)));
                self.send(&Message::LinkSetup(LinkSetup {
                    host_user_version: client_version.as_ref(),
                    host_max_message_len: DEFAULT_MAX_MESSAGE_SIZE as u32,
                }))?;
                self.flush(now);
            }
            ToTx::LinkReady => {
                if self.phase != Phase::LinkSetup {
                    warn!("unexpected LinkReady in {:?}, ignoring", self.phase);
                    return Ok(Flow::Continue);
                }
                info!("link setup complete");
                self.phase = Phase::Up;
                self.next_ping_at = Some(now + PING_INTERVAL);
                self.tracers.connected();
            }
            ToTx::Freed(seq) => {
                self.in_flight.remove(&seq);
            }
            ToTx::PeerGone(result) => {
                debug!("rx side is gone, exiting");
                // rx already knows, don't send anything, don't tell rx to stop
                self.exit(result, false);
            }
        }
        Ok(Flow::Continue)
    }

    fn on_timer(&mut self, now: Instant) -> anyhow::Result<Flow> {
        match self.phase {
            Phase::GetDeviceInfo => {
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
                let accumulated_due = is_due(
                    self.unflushed_since
                        .map(|t| t + self.frame_accumulation_time),
                    now,
                );
                if accumulated_due {
                    trace!(
                        "flushing accumulated messages after {}us",
                        self.unflushed_since
                            .map(|t| (now - t).as_micros())
                            .unwrap_or(0)
                    );
                    self.flush(now);
                } else if is_due(self.next_ping_at, now) {
                    trace!("sending ping");
                    self.send(&Message::Ping)?;
                    self.flush(now);
                }
            }
            Phase::Idle | Phase::Exited | Phase::Connecting | Phase::LinkSetup => {}
        }
        Ok(Flow::Continue)
    }

    // Helpers

    /// Number of requests waiting for an answer (seq numbers not yet freed by rx).
    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    fn next_seq(&mut self) -> Option<SeqTy> {
        for _ in 0..SeqTy::MAX {
            if self.next_seq == 0 {
                self.next_seq = 1;
            }
            let seq = self.next_seq;
            self.next_seq = seq.wrapping_add(1);
            if self.in_flight.insert(seq) {
                return Some(seq);
            }
        }
        None
    }

    /// Encode a link message and queue it for the wrapper's framer.
    fn send(&mut self, msg: &Message<'_>) -> Result<(), Error> {
        trace!("tx: {msg:?}");
        let (kind, payload) = msg
            .encode(&mut self.scratch)
            .map_err(|e| Error::Transport(format!("{e:?}")))?;
        self.output.push_back(TxOutput::Send {
            kind,
            message: payload.to_vec(),
        });
        Ok(())
    }

    /// Ask the wrapper to send the current frame; also restarts the ping timer.
    fn flush(&mut self, now: Instant) {
        self.output.push_back(TxOutput::Flush);
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
        if matches!(
            self.phase,
            Phase::GetDeviceInfo | Phase::LinkSetup | Phase::Up
        ) {
            _ = self.send(&Message::Disconnect(reason));
            self.flush(now);
        }
    }

    fn exit(&mut self, result: anyhow::Result<EventLoopExitReason>, notify_rx: bool) {
        match &result {
            Ok(r) => debug!("tx exiting with: {r:?}"),
            Err(e) => error!("tx exiting with: {e:?}"),
        }
        self.phase = Phase::Exited;
        self.unflushed_since = None;
        self.next_ping_at = None;
        self.next_link_setup_retry_at = None;
        self.in_flight.clear();
        if notify_rx {
            let err = result.as_ref().err().map(|e| format!("{e}"));
            self.output.push_back(TxOutput::ToRx(ToRx::Stop(err)));
        }
        self.output.push_back(TxOutput::Exit(result));
    }
}
