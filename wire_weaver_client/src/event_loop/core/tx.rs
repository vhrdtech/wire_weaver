//! Tx half: commands in, link messages out. Owns link setup retries, the frame accumulation
//! window, pings and seq allocation.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tracing::{debug, error, info, trace, warn};
use ww_link::{DisconnectReason, LinkSetup, Message};
use ww_version::{FullVersionOwned, VersionOwned};

use super::loopback::{LOOPBACK_HEADER_LEN, LoopbackTest, Status, TestOut};
use super::{ToRx, ToTx, Tracers, is_due};
use crate::DEFAULT_MAX_MESSAGE_SIZE;
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::{
    Command, EventLoopExitReason, EventLoopResidual, LoopbackParams, StreamOpenHook, TestProgress,
};
use crate::event_loop::rx_dispatcher::ResponseSender;
use crate::event_loop::transport::LinkSetupPacing;
use crate::{Error, SeqTy};

const PING_INTERVAL: Duration = Duration::from_millis(ww_link::PING_INTERVAL_MS);
/// Largest seq number that is serialized into 1 byte.
const SEQ_CYCLE_MAX: SeqTy = 127;

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

/// Request bytes and where to report the response, see [TxCore::held_until_up].
type HeldMessage = (
    Vec<u8>,
    Option<(ResponseSender, Duration)>,
    Option<Box<StreamOpenHook>>,
);

pub(crate) struct TxCore {
    phase: Phase,
    output: VecDeque<TxOutput>,
    scratch: Vec<u8>,
    tracers: Tracers,

    exited_tx: Option<tokio::sync::oneshot::Sender<EventLoopResidual>>,
    /// From a disconnect command, notified by the wrapper once the transport is closed, see [Self::take_disconnected_tx]
    disconnected_tx: Option<tokio::sync::oneshot::Sender<()>>,
    client_version: Option<FullVersionOwned>,
    /// Requested by a device, how long to accumulate messages into one frame before sending it out
    frame_accumulation_time: Duration,
    /// From DeviceInfo, requests bigger than this are rejected
    device_max_message_len: usize,

    // Timers
    /// Transport's default, [Command::Connect] can override it
    link_setup_timeout: Duration,
    link_setup_deadline: Option<Instant>,
    link_setup_interval: Duration,
    next_link_setup_retry_at: Option<Instant>,
    /// Set when data was sent but not yet flushed, waiting for more messages to fill the frame
    unflushed_since: Option<Instant>,
    next_ping_at: Option<Instant>,

    // Seq allocation
    next_seq: SeqTy,
    pub(crate) max_seq: SeqTy,
    in_flight: HashSet<SeqTy>,

    /// Messages sent while link setup is in progress, sent out once the link is up.
    /// RxCore reports the connection to the user before LinkReady reaches tx, so the first
    /// requests can legitimately arrive here a bit early.
    /// Also messages sent while a loopback test runs, sent out once it is done.
    held_until_up: VecDeque<HeldMessage>,
    /// Requests that wait for a seq number to be freed (all are in flight), sent in order as seqs come back.
    waiting_for_seq: VecDeque<HeldMessage>,

    /// Running [Command::LoopbackTest], only while the link is up
    loopback: Option<LoopbackTest>,
}

impl TxCore {
    /// `link_setup_timeout` is the transport's default, see [Transport::link_setup_timeout].
    pub fn new(link_setup_timeout: Duration) -> Self {
        TxCore {
            phase: Phase::Idle,
            output: VecDeque::new(),
            scratch: vec![0u8; DEFAULT_MAX_MESSAGE_SIZE],
            tracers: Tracers::default(),
            exited_tx: None,
            disconnected_tx: None,
            client_version: None,
            frame_accumulation_time: Duration::from_millis(1),
            device_max_message_len: DEFAULT_MAX_MESSAGE_SIZE,
            link_setup_timeout,
            link_setup_deadline: None,
            link_setup_interval: Duration::ZERO,
            next_link_setup_retry_at: None,
            unflushed_since: None,
            next_ping_at: None,
            next_seq: 1,
            max_seq: crate::DEFAULT_MAX_SEQ,
            in_flight: HashSet::new(),
            held_until_up: VecDeque::new(),
            waiting_for_seq: VecDeque::new(),
            loopback: None,
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
    /// Feed pending [TxInput::FromRx] messages before the timer: deadlines (e.g. loopback echo timeouts) count on
    /// them.
    pub fn poll_timeout(&self) -> Option<Instant> {
        match self.phase {
            Phase::GetDeviceInfo => self.next_link_setup_retry_at,
            Phase::Up => [
                self.unflushed_since
                    .map(|t| t + self.frame_accumulation_time),
                self.next_ping_at,
                self.loopback.as_ref().map(|t| t.poll_timeout()),
            ]
            .into_iter()
            .flatten()
            .min(),
            Phase::Idle | Phase::Exited | Phase::Connecting | Phase::LinkSetup => None,
        }
    }

    /// Sender from a disconnect command, if that is why tx exited. Must only be notified after the transport
    /// is closed: user code is waiting on it to re-connect, possibly to the same device.
    pub fn take_disconnected_tx(&mut self) -> Option<tokio::sync::oneshot::Sender<()>> {
        self.disconnected_tx.take()
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
                max_seq,
                link_setup_timeout_ms,
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
                self.max_seq = max_seq;
                if let Some(ms) = link_setup_timeout_ms {
                    self.link_setup_timeout = Duration::from_millis(ms.get().into());
                }
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
                // before rx learns that the link is down
                self.output.push_back(TxOutput::ToRx(ToRx::KeepStreams));
                self.tracers.disconnected("client request", true);
                self.send_disconnect(now, reason);
                self.disconnected_tx = disconnected_tx;
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
                self.disconnected_tx = disconnected_tx;
                return Ok(Flow::Exit(EventLoopExitReason::DisconnectCommand));
            }
            Command::SendMessage { bytes, done_tx } => {
                self.on_send_message(now, bytes, done_tx, None);
            }
            Command::OpenStream {
                bytes,
                done_tx,
                timeout,
                hook,
            } => {
                self.on_send_message(now, bytes, Some((done_tx, timeout)), Some(hook));
            }
            Command::OnStreamEvent {
                path_kind,
                stream_event_tx,
                registered_tx,
            } => {
                self.output.push_back(TxOutput::ToRx(ToRx::OnStreamEvent {
                    path_kind,
                    stream_event_tx,
                    registered_tx,
                }));
            }
            Command::ObserveRoute { hook } => {
                self.output
                    .push_back(TxOutput::ToRx(ToRx::ObserveRoute { hook }));
            }
            Command::LoopbackTest {
                test_duration,
                packet_size,
                params,
                progress_tx,
            } => self.start_loopback_test(now, test_duration, packet_size, *params, progress_tx),
        }
        Ok(Flow::Continue)
    }

    fn on_send_message(
        &mut self,
        now: Instant,
        mut bytes: Vec<u8>,
        done_tx: Option<(ResponseSender, Duration)>,
        hook: Option<Box<StreamOpenHook>>,
    ) {
        if matches!(
            self.phase,
            Phase::Connecting | Phase::GetDeviceInfo | Phase::LinkSetup
        ) || self.loopback.is_some()
        {
            trace!("holding SendMessage until link is up and no loopback test runs");
            self.held_until_up.push_back((bytes, done_tx, hook));
            return;
        }
        if self.phase != Phase::Up {
            warn!("ignoring SendMessage while disconnected");
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(Error::Disconnected));
            }
            return;
        }
        let (seq, done_tx) = match done_tx {
            Some((done_tx, timeout)) => {
                // after the ones already waiting, so requests go out in order
                let seq = match self.waiting_for_seq.is_empty() {
                    true => self.next_seq(),
                    false => None,
                };
                let Some(seq) = seq else {
                    trace!("all seq numbers in flight, the request waits for one");
                    self.waiting_for_seq
                        .push_back((bytes, Some((done_tx, timeout)), hook));
                    return;
                };
                (seq, Some((done_tx, timeout)))
            }
            None => (0, None),
        };
        // NOTE: this is the only use of Request in this crate, a bit unfortunate to mix it in here, but otherwise
        // every CommandSender have to get a unique seq number somehow and previous implementation that was doing that
        // was much uglier and had limitations (see the last use of it at git sha: 0113fa4)
        // Requests are serialized with a 5-byte seq placeholder, drop the part of it not needed for this seq.
        let error = match ww_client_server::Request::set_seq(&mut bytes, seq).map(|t| t.len()) {
            Ok(trimmed_len) => {
                bytes.drain(..bytes.len() - trimmed_len);
                if bytes.len() > self.device_max_message_len {
                    warn!(
                        "request of {} bytes exceeds device max message length {}, dropping",
                        bytes.len(),
                        self.device_max_message_len
                    );
                    Some(Error::Other(
                        "request exceeds device max message length".into(),
                    ))
                } else {
                    None
                }
            }
            Err(e) => {
                warn!("malformed request, dropping: {e:?}");
                Some(Error::Other(format!("malformed request: {e:?}")))
            }
        };
        if let Some(error) = error {
            self.in_flight.remove(&seq);
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(error));
            }
            return;
        }
        if let Some((done_tx, timeout)) = done_tx {
            // Expect goes out before the request bytes, so rx knows about the seq before the answer can arrive
            self.output.push_back(TxOutput::ToRx(ToRx::Expect {
                seq,
                done_tx,
                timeout,
                hook,
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
        let pacing = LinkSetupPacing::new(self.link_setup_timeout);
        self.link_setup_deadline = Some(now + pacing.timeout);
        self.link_setup_interval = pacing.first_interval();
        self.next_link_setup_retry_at = Some(now + self.link_setup_interval);
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
                while let Some((bytes, done_tx, hook)) = self.held_until_up.pop_front() {
                    self.on_send_message(now, bytes, done_tx, hook);
                }
            }
            ToTx::Send(bytes) => {
                if self.phase == Phase::Up {
                    self.on_send_message(now, bytes, None, None);
                }
            }
            ToTx::Loopback { seq, len, intact } => {
                if let Some(test) = self.loopback.as_mut() {
                    test.on_echo(now, seq, len, intact);
                    self.drive_loopback_test(now, Status::Running)?;
                } else {
                    debug!("loopback message while no test runs, ignoring");
                }
            }
            ToTx::Freed(seq) => {
                self.in_flight.remove(&seq);
                if self.phase == Phase::Up {
                    // the first waiting one takes the seq (or waits again, first), the rest keep waiting behind it
                    let mut waiting = std::mem::take(&mut self.waiting_for_seq);
                    if let Some((bytes, done_tx, hook)) = waiting.pop_front() {
                        self.on_send_message(now, bytes, done_tx, hook);
                    }
                    self.waiting_for_seq.extend(waiting);
                }
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
                    let deadline = self.link_setup_deadline.unwrap_or(now);
                    if now < deadline {
                        warn!("resending GetDeviceInfo after no answer received from device");
                        self.link_setup_interval = LinkSetupPacing::new(self.link_setup_timeout)
                            .next_interval(self.link_setup_interval);
                        // the last wait ends at the deadline, nothing is re-sent then
                        self.next_link_setup_retry_at =
                            Some((now + self.link_setup_interval).min(deadline));
                        self.send_get_device_info(now)?;
                    } else {
                        error!(
                            "exiting, because link setup got no answer within {:?}",
                            self.link_setup_timeout
                        );
                        return Err(anyhow!(Error::LinkSetupTimeout));
                    }
                }
            }
            Phase::Up => {
                if let Some(test) = self.loopback.as_mut()
                    && is_due(Some(test.poll_timeout()), now)
                {
                    let status = test.on_timer(now);
                    self.drive_loopback_test(now, status)?;
                }
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

    /// Seq numbers cycle through `1..=SEQ_CYCLE_MAX`, which are serialized into 1 byte, larger ones up to `max_seq`
    /// are only used while all of these are in flight. Rx keeps the seq of a timed out request in flight for a while,
    /// see [LATE_REPLY_WINDOW](crate::event_loop::rx_dispatcher::LATE_REPLY_WINDOW).
    fn next_seq(&mut self) -> Option<SeqTy> {
        let cycle_max = SEQ_CYCLE_MAX.min(self.max_seq);
        for _ in 0..cycle_max {
            let seq = self.next_seq;
            self.next_seq = if seq >= cycle_max { 1 } else { seq + 1 };
            if self.in_flight.insert(seq) {
                return Some(seq);
            }
        }
        (cycle_max + 1..=self.max_seq).find(|seq| self.in_flight.insert(*seq))
    }

    fn start_loopback_test(
        &mut self,
        now: Instant,
        duration: Duration,
        packet_size: Option<usize>,
        params: LoopbackParams,
        progress_tx: tokio::sync::mpsc::UnboundedSender<TestProgress>,
    ) {
        if self.phase != Phase::Up {
            _ = progress_tx.send(TestProgress::FatalError("not connected".into()));
            return;
        }
        if self.loopback.is_some() {
            _ = progress_tx.send(TestProgress::FatalError(
                "a loopback test is already running".into(),
            ));
            return;
        }
        // the device echoes messages back as they are, so both sides must accept them
        let max = self.device_max_message_len.min(DEFAULT_MAX_MESSAGE_SIZE);
        let message_len = packet_size.unwrap_or(max).clamp(LOOPBACK_HEADER_LEN, max);
        info!("starting loopback test: {duration:?} per stage, {message_len} bytes per message");
        // what was written so far goes out before the test
        self.flush(now);
        self.loopback = Some(LoopbackTest::new(
            now,
            duration,
            message_len,
            params,
            progress_tx,
        ));
        _ = self.drive_loopback_test(now, Status::Running);
    }

    /// Send what the loopback test wants, and end it when it is finished.
    fn drive_loopback_test(&mut self, now: Instant, status: Status) -> Result<(), Error> {
        let Some(test) = self.loopback.as_mut() else {
            return Ok(());
        };
        let mut outs = vec![];
        while let Some(o) = test.poll_output() {
            outs.push(o);
        }
        for o in outs {
            match o {
                TestOut::Send { repeat, seq, data } => {
                    self.send(&Message::Loopback {
                        repeat,
                        seq,
                        data: &data,
                    })?;
                    if self.unflushed_since.is_none() {
                        self.unflushed_since = Some(now);
                    }
                }
                TestOut::Flush => self.flush(now),
            }
        }
        if status == Status::Finished {
            info!("loopback test finished");
            // drops progress_tx, which tells the user it is over
            self.loopback = None;
            while let Some((bytes, done_tx, hook)) = self.held_until_up.pop_front() {
                self.on_send_message(now, bytes, done_tx, hook);
            }
        }
        Ok(())
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
        if let Some(test) = self.loopback.take() {
            test.abort("link is down");
        }
        self.unflushed_since = None;
        self.next_ping_at = None;
        self.next_link_setup_retry_at = None;
        self.link_setup_deadline = None;
        self.in_flight.clear();
        let waiting = self.waiting_for_seq.drain(..);
        for (_, done_tx, _) in self.held_until_up.drain(..).chain(waiting) {
            if let Some((done_tx, _)) = done_tx {
                _ = done_tx.send(Err(Error::Disconnected));
            }
        }
        if notify_rx {
            let err = result.as_ref().err().map(|e| format!("{e}"));
            self.output.push_back(TxOutput::ToRx(ToRx::Stop(err)));
        }
        self.output.push_back(TxOutput::Exit(result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_limited_by_max_seq() {
        let mut tx = TxCore::new(Duration::from_secs(1));
        tx.max_seq = 130;
        let seqs: Vec<SeqTy> = (0..130).map(|_| tx.next_seq().unwrap()).collect();
        assert_eq!(seqs, (1..=130).collect::<Vec<_>>());
        assert_eq!(tx.next_seq(), None);
        tx.in_flight.remove(&129);
        assert_eq!(tx.next_seq(), Some(129));

        let mut tx = TxCore::new(Duration::from_secs(1));
        tx.max_seq = 3;
        assert_eq!(
            (0..4).map(|_| tx.next_seq()).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3), None]
        );
        tx.in_flight.remove(&2);
        assert_eq!(tx.next_seq(), Some(2));
    }

    #[test]
    fn default_max_seq_is_3_bytes() {
        use wire_weaver::shrink_wrap::UVlq32;
        assert_eq!(UVlq32(crate::DEFAULT_MAX_SEQ).len_bytes(), 3);
        assert_eq!(UVlq32(crate::DEFAULT_MAX_SEQ + 1).len_bytes(), 4);
    }
}
