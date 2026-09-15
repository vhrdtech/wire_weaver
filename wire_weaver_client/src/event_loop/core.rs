//! Sans-IO event loop cores: all the protocol logic of a client event loop without any IO,
//! timers, channels or framing of their own.
//!
//! Rx and tx are two independent state machines ([RxCore] and [TxCore]) so that a wrapper can run
//! them as two tasks (or threads) and the receive path is never blocked by a write. Otherwise, a
//! half-duplex device (one that does not read while it is blocked writing) and a half-duplex host
//! deadlock as soon as both directions fill up.
//!
//! The two cores talk through [ToTx] / [ToRx] messages, which the wrapper ferries across whatever
//! channel it likes. Both cores speak in [ww_link] messages `(kind, payload)`; packing them into
//! frames (and choosing a framer configuration that suits the medium) is also up to the wrapper.
//!
//! ```text
//!  Commander ──cmd_rx──► TxCore ──Send/Flush──► framer ──► medium
//!                          ▲  │
//!                     ToTx │  │ ToRx
//!                          │  ▼
//!  dispatcher ◄────────── RxCore ◄── framer ◄── medium
//! ```
//!
//! Who decides what:
//! - tx: link setup retries, frame accumulation window, ping, seq allocation, all commands
//! - rx: decoding, version check, dispatcher (responses, streams, timeouts), peer timeout
//!
//! Either side may decide to exit; it tells the other ([ToRx::Stop] / [ToTx::PeerGone]) and the
//! wrapper joins both. Rx always tells the dispatcher `Disconnected` on the way out.

pub(crate) mod rx;
pub(crate) mod tx;

use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};
use tracing::trace;
use tracing::warn;
use ww_link::DeviceInfoOwned;

use crate::SeqTy;
use crate::event_loop::command::Command;
use crate::event_loop::command::EventLoopExitReason;
use crate::event_loop::rx_dispatcher::{ResponseSender, StreamUpdateSender};
use crate::event_loop::transport::{MessageRx, MessageTx, Transport};
use crate::tracing::tracing::TraceEvent;
use ww_client_server::PathKindOwned;

pub(crate) use rx::{RxCore, RxInput, RxOutput};
pub(crate) use tx::{TxCore, TxInput, TxOutput};

/// Deadlines this close to `now` are considered due, to avoid spinning on tiny sleeps.
const TIMER_TOLERANCE: Duration = Duration::from_micros(10);

/// Give the last (Disconnect) transfer a chance to actually go out before transport is dropped.
const EXIT_LINGER: Duration = Duration::from_millis(3);

/// Rx → tx
pub(crate) enum ToTx {
    /// DeviceInfo received and accepted, tx should reply with LinkSetup.
    DeviceInfo(Box<DeviceInfoOwned>),
    /// LinkReady received, data can flow.
    LinkReady,
    /// Request with this seq completed, timed out or was cancelled.
    Freed(SeqTy),
    /// Rx is exiting (remote Disconnect, peer timeout, read error, ...) and tx should too,
    /// without sending anything else.
    PeerGone(anyhow::Result<EventLoopExitReason>),
}

/// Tx → rx
pub(crate) enum ToRx {
    /// A request with this seq was sent, dispatcher should wait for the answer.
    /// Must be delivered before the request frame leaves, see [RxCore] docs.
    Expect {
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Duration,
    },
    OnStreamEvent {
        path_kind: Box<PathKindOwned>,
        stream_event_tx: StreamUpdateSender,
    },
    RegisterTracer(mpsc::UnboundedSender<TraceEvent>),
    /// Transport is up, rx should start expecting link setup messages.
    TransportUp {
        /// Used for the compatibility check against DeviceInfo, empty crate_id = skip
        client_version: Box<ww_version::FullVersionOwned>,
        connected_tx: Option<oneshot::Sender<crate::device_info::ConnectionInfo>>,
    },
    /// Tx decided to exit (command, commander dropped, setup timeout, write error), rx should too.
    /// Error, if any, is reported to a pending `connected_tx` by rx.
    Stop(Option<String>),
}

fn is_due(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_some_and(|d| d.saturating_duration_since(now) < TIMER_TOLERANCE)
}

// TODO: lookup from ww_global
fn compact_to_full(v: &ww_version::CompactVersion) -> ww_version::FullVersionOwned {
    ww_version::FullVersionOwned::new(
        format!("G{}", v.gid.id.0),
        ww_version::VersionOwned::new(v.major.0, v.minor.0, v.patch.0),
    )
}

/// Fan-out to registered tracers, dropping the ones whose receiver is gone.
#[derive(Default)]
pub(crate) struct Tracers(Vec<mpsc::UnboundedSender<TraceEvent>>);

impl Tracers {
    pub fn register(&mut self, tx: mpsc::UnboundedSender<TraceEvent>) {
        self.0.push(tx);
    }

    fn emit(&mut self, event: impl Fn() -> TraceEvent) {
        self.0.retain(|tx| tx.send(event()).is_ok());
    }

    pub fn connected(&mut self) {
        self.emit(|| TraceEvent::Connected {});
    }

    pub fn request(&mut self, bytes: &[u8]) {
        self.emit(|| TraceEvent::Request {
            bytes: bytes.to_vec(),
        });
    }

    pub fn event(&mut self, bytes: &[u8]) {
        self.emit(|| TraceEvent::Event {
            bytes: bytes.to_vec(),
        });
    }

    pub fn disconnected(&mut self, reason: &str, keep_streams: bool) {
        self.emit(|| TraceEvent::Disconnected {
            reason: reason.to_string(),
            keep_streams,
        });
    }

    pub fn error(&mut self, reason: &str) {
        self.emit(|| TraceEvent::Error {
            reason: reason.to_string(),
        });
    }
}

pub(crate) async fn worker<T: Transport + 'static>(cmd_rx: mpsc::Receiver<Command>, connector: T) {
    let (to_rx_tx, to_rx_rx) = mpsc::unbounded_channel::<ToRx>();
    let (to_tx_tx, to_tx_rx) = mpsc::unbounded_channel::<ToTx>();
    // rx endpoint is opened by the tx task (it handles Connect) and handed over
    let (msg_rx_tx, msg_rx_rx) = oneshot::channel::<T::Rx>();

    let rx_task = tokio::spawn(rx_task::<T>(msg_rx_rx, to_rx_rx, to_tx_tx));
    let (tx_core_result, cmd_rx) = tx_task(cmd_rx, connector, to_tx_rx, to_rx_tx, msg_rx_tx).await;
    let rx_core = match rx_task.await {
        Ok(rx_core) => rx_core,
        Err(e) => {
            warn!("rx task panicked: {e}");
            RxCore::new()
        }
    };

    let (tx_core, result) = tx_core_result;
    let (exited_tx, residual) = tx_core.into_residual(cmd_rx, rx_core.into_connected_tx(), result);
    if let Some(tx) = exited_tx {
        _ = tx.send(residual);
    }
}

async fn tx_task<T: Transport>(
    mut cmd_rx: mpsc::Receiver<Command>,
    mut connector: T,
    mut from_rx: mpsc::UnboundedReceiver<ToTx>,
    to_rx: mpsc::UnboundedSender<ToRx>,
    msg_rx_tx: oneshot::Sender<T::Rx>,
) -> (
    (TxCore, anyhow::Result<EventLoopExitReason>),
    mpsc::Receiver<Command>,
) {
    let mut core = TxCore::new();
    let mut msg_tx: Option<T::Tx> = None;
    let mut msg_rx_tx = Some(msg_rx_tx);

    let result = loop {
        // Drain outputs first, do not feed new inputs until done.
        let mut exit = None;
        while let Some(output) = core.poll_output() {
            match output {
                TxOutput::Connect(handle) => {
                    let Some(msg_rx_tx) = msg_rx_tx.take() else {
                        // proper way to re-connect to the same device, or another one, is to tear down this event loop
                        // then take the residual and start a new one, sending a connect command there
                        // it could even be a different transport
                        warn!("ignoring connect command, while already connected, this is a bug");
                        continue;
                    };
                    match connector.connect(handle) {
                        Ok(o) => {
                            msg_tx = Some(o.tx);
                            if msg_rx_tx.send(o.rx).is_err() {
                                core.handle(
                                    Instant::now(),
                                    TxInput::TransportError("rx task is gone".into()),
                                );
                            } else {
                                core.handle(Instant::now(), TxInput::TransportUp);
                            }
                        }
                        Err(e) => core.handle(Instant::now(), TxInput::TransportError(e)),
                    }
                }
                TxOutput::Send { kind, message } => {
                    let Some(msg_tx) = msg_tx.as_mut() else {
                        warn!("Send without transport, dropping");
                        continue;
                    };
                    trace!("tx msg kind={kind} len={}: {message:02x?}", message.len());
                    if let Err(e) = msg_tx.write_message(kind, &message).await {
                        // Core is responsible for not sending oversized messages, so this is a bug or a
                        // wrong frame size, not something the device did
                        core.handle(Instant::now(), TxInput::TransportError(e.to_string()));
                    }
                }
                TxOutput::Flush => {
                    trace!("tx msg flush");
                    if let Some(msg_tx) = msg_tx.as_mut() {
                        if let Err(e) = msg_tx.flush().await {
                            core.handle(Instant::now(), TxInput::TransportError(e));
                        }
                    }
                }
                TxOutput::ToRx(msg) => {
                    if to_rx.send(msg).is_err() {
                        // rx task exited; it would have sent PeerGone first, which is queued in from_rx
                    }
                }
                TxOutput::Exit(result) => {
                    exit = Some(result);
                    break;
                }
            }
        }
        if let Some(result) = exit {
            if let Some(mut msg_tx) = msg_tx.take() {
                _ = msg_tx.flush().await;
                tokio::time::sleep(EXIT_LINGER).await;
            }
            break result;
        }

        let deadline = core.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            msg = from_rx.recv() => match msg {
                Some(msg) => core.handle(Instant::now(), TxInput::FromRx(msg)),
                None => core.handle(Instant::now(), TxInput::TransportError("rx task is gone".into())),
            },
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => core.handle(Instant::now(), TxInput::Command(cmd)),
                None => core.handle(Instant::now(), TxInput::CommanderDropped),
            },
            _ = timer => core.handle(Instant::now(), TxInput::Timer),
        }
    };
    drop(msg_tx); // closes the device from tx side, rx read then fails or is stopped by ToRx::Stop
    ((core, result), cmd_rx)
}

async fn rx_task<T: Transport>(
    msg_rx_rx: oneshot::Receiver<T::Rx>,
    mut from_tx: mpsc::UnboundedReceiver<ToRx>,
    to_tx: mpsc::UnboundedSender<ToTx>,
) -> RxCore {
    let mut core = RxCore::new();
    let mut msg_rx_rx = Some(msg_rx_rx);
    let mut msg_rx: Option<T::Rx> = None;

    loop {
        // Everything tx told us goes in first, see RxCore docs on ordering
        let mut exit = false;
        while let Ok(msg) = from_tx.try_recv() {
            core.handle(Instant::now(), RxInput::FromTx(msg));
        }
        while let Some(output) = core.poll_output() {
            match output {
                RxOutput::ToTx(msg) => _ = to_tx.send(msg),
                RxOutput::Exit => exit = true,
            }
        }
        if exit {
            break;
        }

        let deadline = core.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        let Some(rx) = msg_rx.as_mut() else {
            // Not connected yet: only tx can change that, by handing over the rx half
            let Some(rx_rx) = msg_rx_rx.take() else {
                // transport failed earlier, nothing to read from anymore; tx will say Stop
                match from_tx.recv().await {
                    Some(msg) => core.handle(Instant::now(), RxInput::FromTx(msg)),
                    None => break,
                }
                continue;
            };
            tokio::select! {
                r = rx_rx => match r {
                    Ok(r) => msg_rx = Some(r),
                    // tx task is gone without saying Stop (should not happen), exit
                    Err(_) => break,
                },
                msg = from_tx.recv() => match msg {
                    Some(msg) => core.handle(Instant::now(), RxInput::FromTx(msg)),
                    None => break,
                },
                _ = timer => core.handle(Instant::now(), RxInput::Timer),
            }
            continue;
        };
        tokio::select! {
            msg = from_tx.recv() => match msg {
                Some(msg) => core.handle(Instant::now(), RxInput::FromTx(msg)),
                None => break,
            },
            r = rx.read_message() => {
                let now = Instant::now();
                match r {
                    Ok((kind, message)) => {
                        trace!("rx msg kind={kind} len={}: {message:02x?}", message.len());
                        // Expect for this answer may have arrived while we were reading
                        while let Ok(msg) = from_tx.try_recv() {
                            core.handle(now, RxInput::FromTx(msg));
                        }
                        core.handle(now, RxInput::Message { kind, message });
                    }
                    Err(e) => {
                        core.handle(now, RxInput::TransportError(e));
                        // rx core exits on transport error; outputs are handled at the top of the loop
                    }
                }
            }
            _ = timer => core.handle(Instant::now(), RxInput::Timer),
        }
    }
    drop(msg_rx);
    core
}

#[cfg(test)]
mod tests {
    //! Both cores driven together through an in-test bridge, no runtime, hand-advanced time.

    use super::*;
    use crate::device_info::ConnectionInfo;
    use crate::event_loop::command::Command;
    use tokio::sync::oneshot;
    use ww_link::{DeviceInfo, DisconnectReason, Kind, Message};
    use ww_version::{
        ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version,
        VersionOwned,
    };

    const RETRY: Duration = Duration::from_millis(50);
    const PING: Duration = Duration::from_millis(ww_link::PING_INTERVAL_MS);
    const PEER_TIMEOUT: Duration = Duration::from_millis(ww_link::PEER_TIMEOUT_MS);

    /// What the wrapper would have pushed into the framer
    #[derive(Debug, PartialEq, Eq)]
    struct Sent {
        kind: Kind,
        payload: Vec<u8>,
        /// Index of the flush this message went out with
        frame: usize,
    }

    struct Pair {
        tx: TxCore,
        rx: RxCore,
        sent: Vec<Sent>,
        flushes: usize,
        connects: usize,
        tx_exit: Option<anyhow::Result<EventLoopExitReason>>,
        rx_exited: bool,
    }

    impl Pair {
        fn new() -> Self {
            Pair {
                tx: TxCore::new(),
                rx: RxCore::new(),
                sent: vec![],
                flushes: 0,
                connects: 0,
                tx_exit: None,
                rx_exited: false,
            }
        }

        /// Ferry messages between cores until both are quiet, collecting tx output.
        fn settle(&mut self, now: Instant) {
            loop {
                let mut progressed = false;
                while let Some(o) = self.tx.poll_output() {
                    progressed = true;
                    match o {
                        TxOutput::Connect(_) => self.connects += 1,
                        TxOutput::Send {
                            kind,
                            message: payload,
                        } => self.sent.push(Sent {
                            kind: Kind::from_repr(kind).unwrap(),
                            payload,
                            frame: self.flushes,
                        }),
                        TxOutput::Flush => self.flushes += 1,
                        TxOutput::ToRx(m) => self.rx.handle(now, RxInput::FromTx(m)),
                        TxOutput::Exit(r) => self.tx_exit = Some(r),
                    }
                }
                while let Some(o) = self.rx.poll_output() {
                    progressed = true;
                    match o {
                        RxOutput::ToTx(m) => self.tx.handle(now, TxInput::FromRx(m)),
                        RxOutput::Exit => self.rx_exited = true,
                    }
                }
                if !progressed {
                    break;
                }
            }
        }

        fn take_sent(&mut self) -> Vec<Sent> {
            std::mem::take(&mut self.sent)
        }

        fn cmd(&mut self, now: Instant, cmd: Command) {
            self.tx.handle(now, TxInput::Command(cmd));
            self.settle(now);
        }

        fn from_device(&mut self, now: Instant, msg: &Message<'_>) {
            let mut scratch = [0u8; 256];
            let (kind, payload) = msg.encode(&mut scratch).unwrap();
            self.rx.handle(
                now,
                RxInput::Message {
                    kind,
                    message: payload,
                },
            );
            self.settle(now);
        }

        fn timers(&mut self, now: Instant) {
            if self.tx.poll_timeout().is_some_and(|t| t <= now) {
                self.tx.handle(now, TxInput::Timer);
            }
            if self.rx.poll_timeout().is_some_and(|t| t <= now) {
                self.rx.handle(now, RxInput::Timer);
            }
            self.settle(now);
        }
    }

    fn kinds(sent: &[Sent]) -> Vec<Kind> {
        sent.iter().map(|s| s.kind).collect()
    }

    fn device_info() -> Message<'static> {
        Message::DeviceInfo(DeviceInfo {
            dev_link_version: CompactVersion::new(GlobalTypeId::new(512), 0, 1, 0),
            api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
            user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 512,
            packet_accumulation_time_us: 500,
        })
    }

    fn connect(p: &mut Pair, now: Instant) -> oneshot::Receiver<ConnectionInfo> {
        let (connected_tx, connected_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "test".into(),
                    VersionOwned::new(0, 1, 0),
                )),
                connected_tx: Some(connected_tx),
                failed_tx: None,
            },
        );
        assert_eq!(p.connects, 1);
        assert!(p.take_sent().is_empty());
        assert_eq!(p.tx.poll_timeout(), None);

        p.tx.handle(now, TxInput::TransportUp);
        p.settle(now);
        let sent = p.take_sent();
        assert_eq!(kinds(&sent), [Kind::Nop, Kind::GetDeviceInfo]);
        assert_ne!(sent[0].frame, sent[1].frame, "Nop is flushed alone");
        assert_eq!(p.tx.poll_timeout(), Some(now + RETRY));

        p.from_device(now, &device_info());
        let sent = p.take_sent();
        assert_eq!(kinds(&sent), [Kind::LinkSetup]);
        let Message::LinkSetup(setup) =
            Message::decode(Kind::LinkSetup as u8, &sent[0].payload).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            setup.host_max_message_len,
            crate::DEFAULT_MAX_MESSAGE_SIZE as u32
        );
        assert_eq!(setup.host_user_version.crate_id, "test");
        assert_eq!(
            p.tx.poll_timeout(),
            None,
            "no retries while waiting for LinkReady"
        );

        p.from_device(now, &Message::LinkReady);
        assert!(p.take_sent().is_empty());
        connected_rx
    }

    #[test]
    fn link_setup_then_data_ping_and_peer_timeout() {
        let now = Instant::now();
        let mut p = Pair::new();
        let mut connected_rx = connect(&mut p, now);
        let info = connected_rx.try_recv().unwrap().result.unwrap();
        assert_eq!(info.max_message_size, 512);
        assert_eq!(p.tx.poll_timeout(), Some(now + PING));
        assert_eq!(p.rx.poll_timeout(), Some(now + PEER_TIMEOUT));

        // two requests: written to the framer right away, flushed together after the accumulation window
        for bytes in [vec![0, 0, 1], vec![0, 0, 2]] {
            p.cmd(
                now,
                Command::SendMessage {
                    bytes,
                    done_tx: None,
                },
            );
        }
        let sent = p.take_sent();
        assert_eq!(kinds(&sent), [Kind::Data0, Kind::Data0]);
        assert_eq!(sent[0].frame, sent[1].frame);
        assert_eq!(p.tx.poll_timeout(), Some(now + Duration::from_micros(500)));
        let flushes = p.flushes;

        let now = now + Duration::from_micros(500);
        p.timers(now);
        assert!(p.take_sent().is_empty());
        assert_eq!(
            p.flushes,
            flushes + 1,
            "accumulation window ended: exactly one Flush"
        );
        assert_eq!(
            p.tx.poll_timeout(),
            Some(now + PING),
            "flush restarts the ping timer"
        );

        // nothing else to send: ping
        let now = now + PING;
        p.timers(now);
        assert_eq!(kinds(&p.take_sent()), [Kind::Ping]);

        // silent device: rx times out, tells tx, both exit
        let now = now + PEER_TIMEOUT;
        p.timers(now);
        assert!(p.rx_exited);
        // the ping timer was due at the same instant and fired first, that is all that went out
        assert!(kinds(&p.take_sent()).iter().all(|k| *k == Kind::Ping));
        assert!(matches!(p.tx_exit, Some(Err(_))));
    }

    #[test]
    fn request_response_and_seq_reuse() {
        let now = Instant::now();
        let mut p = Pair::new();
        connect(&mut p, now);

        let (done_tx, mut done_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::SendMessage {
                bytes: vec![0, 0, 0xAA],
                done_tx: Some((done_tx, Duration::from_secs(1))),
            },
        );
        let sent = p.take_sent();
        let seq = u16::from_le_bytes([sent[0].payload[0], sent[0].payload[1]]);
        assert_eq!(seq, 1);

        let event = ww_client_server::Event {
            seq,
            result: Ok(ww_client_server::EventKind::Value {
                data: wire_weaver::shrink_wrap::tail_bytes::TailBytes(&[7]),
            }),
        };
        let mut buf = [0u8; 32];
        let bytes =
            wire_weaver::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(&event, &mut buf).unwrap();
        p.from_device(now, &Message::Data { channel: 0, bytes });
        assert_eq!(done_rx.try_recv().unwrap().unwrap(), vec![7]);
        assert!(p.tx.in_flight() == 0, "Freed reached tx");

        // a request that times out is freed as well
        let (done_tx, mut done_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::SendMessage {
                bytes: vec![0, 0, 0xBB],
                done_tx: Some((done_tx, Duration::from_secs(1))),
            },
        );
        p.take_sent();
        assert_eq!(p.tx.in_flight(), 1);
        p.timers(now + Duration::from_secs(1));
        assert!(matches!(
            done_rx.try_recv().unwrap(),
            Err(crate::Error::Timeout)
        ));
        assert!(p.tx.in_flight() == 0);
    }

    #[test]
    fn oversized_request_is_rejected_locally() {
        let now = Instant::now();
        let mut p = Pair::new();
        connect(&mut p, now);
        let (done_tx, mut done_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::SendMessage {
                bytes: vec![0; 513], // device said 512
                done_tx: Some((done_tx, Duration::from_secs(1))),
            },
        );
        assert!(p.take_sent().is_empty());
        assert!(done_rx.try_recv().unwrap().is_err());
        assert!(p.tx.in_flight() == 0);
    }

    #[test]
    fn disconnect_from_device() {
        let now = Instant::now();
        let mut p = Pair::new();
        connect(&mut p, now);
        p.from_device(now, &Message::Disconnect(DisconnectReason::RequestByUser));
        assert!(
            p.take_sent().is_empty(),
            "no Disconnect back to a device that disconnected"
        );
        assert!(p.rx_exited);
        assert!(matches!(
            p.tx_exit,
            Some(Ok(EventLoopExitReason::DisconnectFromDevice))
        ));
    }

    #[test]
    fn disconnect_command_sends_disconnect_then_both_exit() {
        let now = Instant::now();
        let mut p = Pair::new();
        connect(&mut p, now);
        p.cmd(
            now,
            Command::DisconnectAndExit {
                disconnected_tx: None,
                reason: DisconnectReason::RequestByUser,
            },
        );
        assert_eq!(kinds(&p.take_sent()), [Kind::Disconnect]);
        assert!(p.rx_exited);
        assert!(matches!(
            p.tx_exit,
            Some(Ok(EventLoopExitReason::DisconnectCommand))
        ));
    }

    #[test]
    fn link_setup_retries_then_fails() {
        let mut now = Instant::now();
        let mut p = Pair::new();
        let (connected_tx, mut connected_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "".into(),
                    VersionOwned::new(0, 0, 0),
                )),
                connected_tx: Some(connected_tx),
                failed_tx: None,
            },
        );
        p.tx.handle(now, TxInput::TransportUp);
        p.settle(now);
        p.take_sent();
        for _ in 0..5 {
            now += RETRY;
            p.timers(now);
            assert_eq!(kinds(&p.take_sent()), [Kind::Nop, Kind::GetDeviceInfo]);
            assert!(p.tx_exit.is_none());
        }
        now += RETRY;
        p.timers(now);
        assert!(matches!(p.tx_exit, Some(Err(_))));
        assert!(p.rx_exited);
        // rx held connected_tx and reports the failure
        assert!(connected_rx.try_recv().unwrap().result.is_err());
    }

    #[test]
    fn incompatible_version_fails_connection() {
        let now = Instant::now();
        let mut p = Pair::new();
        let (connected_tx, mut connected_rx) = oneshot::channel();
        p.cmd(
            now,
            Command::Connect {
                handle: Box::new(()),
                client_version: Box::new(FullVersionOwned::new(
                    "test".into(),
                    VersionOwned::new(0, 2, 0),
                )),
                connected_tx: Some(connected_tx),
                failed_tx: None,
            },
        );
        p.tx.handle(now, TxInput::TransportUp);
        p.settle(now);
        p.take_sent();
        p.from_device(now, &device_info()); // device is 0.1.3
        assert!(
            p.take_sent().is_empty(),
            "no LinkSetup to an incompatible device"
        );
        assert!(p.rx_exited);
        assert!(matches!(p.tx_exit, Some(Err(_))));
        assert!(connected_rx.try_recv().unwrap().result.is_err());
    }
}
