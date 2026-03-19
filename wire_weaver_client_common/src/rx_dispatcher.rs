use crate::{Error, SeqTy, StreamEvent};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, trace, warn};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UNib32};
use ww_client_server::{EventKind, PathKindOwned};

pub(crate) type ResponseSender = oneshot::Sender<Result<Vec<u8>, Error>>;
pub(crate) type ResponseReceiver = oneshot::Receiver<Result<Vec<u8>, Error>>;

pub(crate) type StreamUpdateSender = mpsc::UnboundedSender<StreamEvent>;
pub(crate) type StreamUpdateReceiver = mpsc::UnboundedReceiver<StreamEvent>;

const IGNORE_TIMER_DURATION: Duration = Duration::from_millis(1);

pub enum DispatcherMessage<'i> {
    Connected,
    MessageBytes(&'i [u8]),
    Disconnected,
}

#[derive(Debug)]
pub enum DispatcherCommand {
    OnReturn {
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Duration,
    },
    OnStreamEvent {
        path_kind: PathKindOwned,
        stream_event_tx: StreamUpdateSender,
    },
}

// pub async fn rx_dispatcher(
//     mut commands: mpsc::UnboundedReceiver<DispatcherCommand>,
//     mut messages: mpsc::UnboundedReceiver<DispatcherMessage>,
// ) {
//     debug!("started");
//     let mut state = DispatcherState::default();
//     loop {
//         let next_timeout = state.prune_next_timeout();
//         let timer = tokio::time::sleep(next_timeout);
//         tokio::select! {
//             cmd = commands.recv() => {
//                 let Some(cmd) = cmd else {
//                     debug!("cmd channel closed, exiting");
//                     break;
//                 };
//                 state.handle_cmd(cmd);
//             }
//             msg = messages.recv() => {
//                 let Some(msg) = msg else {
//                     debug!("message channel closed, exiting");
//                     break;
//                 };
//                 state.handle_msg(msg);
//             }
//             _ = timer => {
//                 state.prune_next_timeout();
//             }
//         }
//     }
//     debug!("exited");
// }

#[derive(Default)]
pub struct RxDispatcher {
    is_connected: bool,
    response_map: HashMap<SeqTy, (ResponseSenderWrapper, Instant)>,
    stream_handlers: HashMap<Vec<UNib32>, Vec<StreamUpdateSender>>,
    next_seq: SeqTy,
}

struct ResponseSenderWrapper(Option<ResponseSender>);

impl ResponseSenderWrapper {
    fn send(&mut self, r: Result<Vec<u8>, Error>) -> Result<(), ()> {
        if let Some(tx) = self.0.take() {
            tx.send(r).map_err(|_| ())
        } else {
            Err(())
        }
    }
}

impl RxDispatcher {
    pub fn handle_cmd(&mut self, cmd: DispatcherCommand) {
        trace!("cmd: {:?}", cmd);
        match cmd {
            DispatcherCommand::OnReturn {
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
                    // TODO: send Connected/Disconnected only on actual connect/disconnect, send status here instead
                    _ = stream_event_tx.send(self.is_connected_as_stream_event());
                    let listeners = self.stream_handlers.entry(path).or_default();
                    listeners.push(stream_event_tx);
                }
                // TODO: other path kinds
            }
        }
    }

    pub fn next_seq(&mut self) -> Option<SeqTy> {
        for _ in 0..SeqTy::MAX {
            if self.next_seq == 0 {
                self.next_seq = 1;
            }
            let seq = self.next_seq;
            self.next_seq = seq.wrapping_add(1);
            if self.response_map.contains_key(&seq) {
                continue;
            }
            return Some(seq);
        }
        None
    }

    fn respond_later(&mut self, seq: SeqTy, done_tx: ResponseSender, timeout: Duration) {
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
        let prune_at = Instant::now() + timeout;
        let replaced = self
            .response_map
            .insert(seq, (ResponseSenderWrapper(Some(done_tx)), prune_at));
        if let Some((mut done_tx, _)) = replaced {
            _ = done_tx.send(Err(Error::User(
                "Seq used for this request was used again".into(),
            )));
        }
    }

    pub fn prune_next_timeout(&mut self) -> Duration {
        let now = Instant::now();
        let mut min: Option<Duration> = None;
        self.response_map.retain(|seq, (done_tx, prune_at)| {
            let till_prune = prune_at
                .checked_duration_since(now)
                .unwrap_or(Duration::from_millis(0));
            if till_prune < IGNORE_TIMER_DURATION {
                _ = done_tx.send(Err(Error::Timeout));
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

    pub fn handle_msg(&mut self, msg: DispatcherMessage) {
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
        let event = match ww_client_server::Event::from_ww_bytes(msg_bytes) {
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
        match event.result {
            Ok(event_kind) => match event_kind {
                EventKind::ReturnValue { data } | EventKind::ReadValue { data } => {
                    if let Some((mut done_tx, _)) = self.response_map.remove(&event.seq) {
                        let return_or_value_bytes = data.as_slice().to_vec();
                        if done_tx.send(Ok(return_or_value_bytes)).is_err() {
                            warn!("failed to send done notification: {:?}", &event.seq);
                        }
                    } else {
                        warn!("unknown seq: {:?}", &event.seq);
                    }
                }
                EventKind::Written => {
                    if let Some((mut done_tx, _)) = self.response_map.remove(&event.seq) {
                        if done_tx.send(Ok(vec![])).is_err() {
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
                if let Some((mut done_tx, _)) = self.response_map.remove(&event.seq) {
                    _ = done_tx.send(Err(Error::RemoteError(e.make_owned())));
                } else {
                    warn!("unknown seq {:?} for remote err {e:?}", &event.seq);
                }
            }
        }
    }

    fn cancel_all_requests(&mut self) {
        trace!("canceling all requests");
        for (_, (mut done_tx, _)) in self.response_map.drain() {
            _ = done_tx.send(Err(Error::Disconnected));
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
}
