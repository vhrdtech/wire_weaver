use crate::{DEFAULT_REQUEST_TIMEOUT, Error, SeqTy, StreamEvent};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, trace, warn};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UNib32};
use ww_client_server::{EventKind, PathKindOwned};

pub type ResponseSender = oneshot::Sender<Result<Vec<u8>, Error>>;
pub type StreamUpdateSender = mpsc::UnboundedSender<StreamEvent>;

const IGNORE_TIMER_DURATION: Duration = Duration::from_millis(1);

pub enum DispatcherMessage {
    Connected,
    MessageBytes(Vec<u8>),
    Disconnected,
}

#[derive(Debug)]
pub enum DispatcherCommand {
    RegisterSeqSource {
        seq_tx: mpsc::Sender<SeqTy>,
    },
    OnCallReturn {
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Option<Duration>,
    },
    OnWriteComplete {
        seq: SeqTy,
        // Vec is always empty here, but allows for common code
        done_tx: ResponseSender,
        timeout: Option<Duration>,
    },
    OnReadValue {
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Option<Duration>,
    },
    OnStreamEvent {
        path_kind: PathKindOwned,
        stream_event_tx: StreamUpdateSender,
        // stop_rx: oneshot::Receiver<()>,
    },
}

pub async fn rx_dispatcher(
    mut commands: mpsc::UnboundedReceiver<DispatcherCommand>,
    mut messages: mpsc::UnboundedReceiver<DispatcherMessage>,
) {
    debug!("started");
    let mut state = DispatcherState::default();
    loop {
        state.replenish_seq_receivers();
        let next_timeout = state.prune_next_timeout();
        let timer = tokio::time::sleep(next_timeout);
        tokio::select! {
            cmd = commands.recv() => {
                let Some(cmd) = cmd else {
                    debug!("cmd channel closed, exiting");
                    break;
                };
                state.handle_cmd(cmd);
            }
            msg = messages.recv() => {
                let Some(msg) = msg else {
                    debug!("message channel closed, exiting");
                    break;
                };
                state.handle_msg(msg);
            }
            _ = timer => {
                state.prune_next_timeout();
            }
        }
    }
    debug!("exited");
}

#[derive(Default)]
struct DispatcherState {
    is_connected: bool,
    response_map: HashMap<SeqTy, (Option<ResponseSender>, Instant)>, // Option to use retain in prune_next_timeout
    stream_handlers: HashMap<Vec<UNib32>, Vec<StreamUpdateSender>>,

    last_seq_used: SeqTy,
    seq_allocated: HashSet<SeqTy>,
    seq_receivers: Vec<mpsc::Sender<SeqTy>>,
}

impl DispatcherState {
    fn handle_cmd(&mut self, cmd: DispatcherCommand) {
        trace!("cmd: {:?}", cmd);
        match cmd {
            DispatcherCommand::RegisterSeqSource { seq_tx: seq_rx } => {
                self.seq_receivers.push(seq_rx);
                self.replenish_seq_receivers();
            }
            DispatcherCommand::OnCallReturn {
                seq,
                done_tx,
                timeout,
            } => {
                self.respond_later(seq, done_tx, timeout);
            }
            DispatcherCommand::OnWriteComplete {
                seq,
                done_tx,
                timeout,
            } => {
                self.respond_later(seq, done_tx, timeout);
            }
            DispatcherCommand::OnReadValue {
                seq,
                done_tx,
                timeout,
            } => {
                self.respond_later(seq, done_tx, timeout);
            }
            DispatcherCommand::OnStreamEvent {
                path_kind,
                stream_event_tx,
            } => {
                if let PathKindOwned::Absolute { path } = path_kind {
                    _ = stream_event_tx.send(self.is_connected_as_stream_event());
                    let listeners = self.stream_handlers.entry(path).or_default();
                    listeners.push(stream_event_tx);
                }
                // TODO: other path kinds
            }
        }
    }

    fn respond_later(&mut self, seq: SeqTy, done_tx: ResponseSender, timeout: Option<Duration>) {
        if seq == 0 {
            _ = done_tx.send(Err(Error::User(
                "Requests with seq == 0 will not be answered".into(),
            )));
            return;
        }
        if !self.is_connected {
            _ = done_tx.send(Err(Error::Disconnected));
            return;
        }
        let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
        let prune_at = Instant::now() + timeout;
        let replaced = self.response_map.insert(seq, (Some(done_tx), prune_at));
        if let Some((Some(done_tx), _)) = replaced {
            _ = done_tx.send(Err(Error::User(
                "Seq used for this request was used again".into(),
            )));
        }
    }

    fn prune_next_timeout(&mut self) -> Duration {
        let now = Instant::now();
        let mut min: Option<Duration> = None;
        self.response_map.retain(|seq, (done_tx, prune_at)| {
            let till_prune = prune_at
                .checked_duration_since(now)
                .unwrap_or(Duration::from_millis(0));
            if till_prune < IGNORE_TIMER_DURATION {
                if let Some(tx) = done_tx.take() {
                    _ = tx.send(Err(Error::Timeout));
                }
                trace!("pruned {seq:?}");
                return false;
            }
            if let Some(prev_min) = &min {
                if till_prune < *prev_min {
                    min = Some(till_prune);
                }
            } else {
                min = Some(till_prune);
            }
            true
        });
        min.unwrap_or(Duration::from_secs(1))
    }

    fn handle_msg(&mut self, msg: DispatcherMessage) {
        let msg_bytes = match msg {
            DispatcherMessage::Connected | DispatcherMessage::Disconnected => {
                match msg {
                    DispatcherMessage::Connected => {
                        self.is_connected = true;
                    }
                    DispatcherMessage::Disconnected => {
                        self.is_connected = false;
                    }
                    _ => unreachable!(),
                }
                if !self.is_connected {
                    self.cancel_all_requests();
                }
                self.notify_streams();
                return;
            }
            DispatcherMessage::MessageBytes(msg) => msg,
        };
        if msg_bytes.is_empty() {
            warn!("empty ww_client_server::Event received, ignoring");
            return;
        }
        let event = match ww_client_server::Event::from_ww_bytes(&msg_bytes) {
            Ok(event) => event,
            Err(e) => {
                warn!(
                    "received malformed ww_client_server::Event: {}:{:02x?} {e:?}",
                    msg_bytes.len(),
                    msg_bytes
                );
                return;
            }
        };
        trace!("received event: {:?}", event);
        self.seq_allocated.remove(&event.seq);
        match event.result {
            Ok(event_kind) => match event_kind {
                EventKind::ReturnValue { data } | EventKind::ReadValue { data } => {
                    if let Some((Some(done_tx), _)) = self.response_map.remove(&event.seq) {
                        let return_or_value_bytes = data.as_slice().to_vec();
                        let r = done_tx.send(Ok(return_or_value_bytes));
                        if r.is_err() {
                            warn!("failed to send done notification: {:?}", &event.seq);
                        }
                    } else {
                        warn!("unknown seq: {:?}", &event.seq);
                    }
                }
                EventKind::Written => {
                    if let Some((Some(done_tx), _)) = self.response_map.remove(&event.seq) {
                        let r = done_tx.send(Ok(Vec::new()));
                        if r.is_err() {
                            warn!("failed to send written notification: {:?}", &event.seq);
                        }
                    } else {
                        warn!("unknown seq: {:?}", &event.seq);
                    }
                }
                EventKind::StreamData { ref path, .. }
                | EventKind::StreamSideband { ref path, .. } => {
                    let ev = match event_kind {
                        EventKind::StreamData { data, .. } => {
                            StreamEvent::Data(data.as_slice().to_vec())
                        }
                        EventKind::StreamSideband { sideband_event, .. } => {
                            StreamEvent::Sideband(sideband_event)
                        }
                        _ => unreachable!(),
                    };
                    let path = path.iter().map(|p| p.unwrap()).collect::<Vec<_>>();
                    if let Some(listeners) = self.stream_handlers.get_mut(&path) {
                        listeners.retain(|tx| {
                            let keep = tx.send(ev.clone()).is_ok();
                            if !keep {
                                debug!("dropped subscriber for stream at {:?}", path);
                            }
                            keep
                        });
                        if listeners.is_empty() {
                            self.stream_handlers.remove(&path);
                        }
                    }
                }
                _ => {}
            },
            Err(e) => {
                if let Some((Some(done_tx), _)) = self.response_map.remove(&event.seq) {
                    let _ = done_tx.send(Err(Error::RemoteError(e.make_owned())));
                } else {
                    warn!("unknown seq {:?} for remote err {e:?}", &event.seq);
                }
            }
        }
    }

    fn cancel_all_requests(&mut self) {
        trace!("canceling all requests");
        for (_, (mut done_tx, _)) in self.response_map.drain() {
            if let Some(done_tx) = done_tx.take() {
                _ = done_tx.send(Err(Error::Disconnected));
            }
        }
    }

    fn is_connected_as_stream_event(&self) -> StreamEvent {
        if self.is_connected {
            StreamEvent::Connected
        } else {
            StreamEvent::Disconnected
        }
    }

    fn notify_streams(&mut self) {
        let event = self.is_connected_as_stream_event();
        trace!("notifying all streams: {event:?}");
        for (path, listeners) in &mut self.stream_handlers {
            listeners.retain(|tx| {
                let keep = tx.send(event.clone()).is_ok();
                if !keep {
                    debug!("dropped subscriber for stream at {:?}", path);
                }
                keep
            });
        }
    }

    fn replenish_seq_receivers(&mut self) {
        for seq_tx in &mut self.seq_receivers {
            'inner: while seq_tx.capacity() > 0 {
                let mut potential_seq = self.last_seq_used.wrapping_add(1);
                if potential_seq == 0 {
                    potential_seq = 1;
                }

                let mut iterations_left = SeqTy::MAX as usize;
                while (self.response_map.contains_key(&potential_seq)
                    || self.seq_allocated.contains(&potential_seq))
                    && iterations_left > 0
                {
                    potential_seq = potential_seq.wrapping_add(1);
                    if potential_seq == 0 {
                        potential_seq = 1;
                    }
                    iterations_left -= 1;
                }

                trace!("allocated seq: {}", potential_seq);
                if seq_tx.try_send(potential_seq).is_err() {
                    break 'inner;
                }
                self.seq_allocated.insert(potential_seq);
                self.last_seq_used = potential_seq;
            }
        }
    }
}
