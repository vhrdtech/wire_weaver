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

pub(crate) enum DispatcherMessage<'i> {
    Connected,
    MessageBytes(&'i [u8]),
    Disconnected,
}

#[derive(Debug)]
pub(crate) enum DispatcherCommand {
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
pub(crate) struct RxDispatcher {
    is_connected: bool,
    response_map: HashMap<SeqTy, (ResponseSenderWrapper, Instant)>,
    stream_handlers: HashMap<Vec<UNib32>, Vec<StreamUpdateSender>>,
    /// Seq numbers whose requests completed, timed out or were cancelled since the last [Self::take_freed].
    /// The tx side allocates seq numbers and needs to know when they can be reused.
    freed: Vec<SeqTy>,
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
    pub fn handle_cmd(&mut self, now: Instant, cmd: DispatcherCommand) {
        trace!("cmd: {:?}", cmd);
        match cmd {
            DispatcherCommand::OnReturn {
                seq,
                done_tx,
                timeout,
            } => {
                self.respond_later(now, seq, done_tx, timeout);
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

    /// Seq numbers that became reusable since the last call.
    pub fn take_freed(&mut self) -> Vec<SeqTy> {
        std::mem::take(&mut self.freed)
    }

    fn respond_later(
        &mut self,
        now: Instant,
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Duration,
    ) {
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
        let prune_at = now + timeout;
        let replaced = self
            .response_map
            .insert(seq, (ResponseSenderWrapper(Some(done_tx)), prune_at));
        if let Some((mut done_tx, _)) = replaced {
            _ = done_tx.send(Err(Error::User(
                "Seq used for this request was used again".into(),
            )));
            // the seq is still in use by the new request, so it is not freed here
        }
    }

    /// Time out all requests that are due at `now` (or within [IGNORE_TIMER_DURATION] of it).
    pub fn prune(&mut self, now: Instant) {
        let due: Vec<SeqTy> = self
            .response_map
            .iter()
            .filter(|(_, (_, prune_at))| {
                prune_at.saturating_duration_since(now) < IGNORE_TIMER_DURATION
            })
            .map(|(seq, _)| *seq)
            .collect();
        for seq in due {
            if let Some((mut done_tx, _)) = self.response_map.remove(&seq) {
                _ = done_tx.send(Err(Error::Timeout));
                trace!("pruned {seq:?}");
                self.freed.push(seq);
            }
        }
    }

    /// Earliest instant at which [Self::prune] has something to do, None if no requests are outstanding.
    pub fn next_prune_at(&self) -> Option<Instant> {
        self.response_map
            .values()
            .map(|(_, prune_at)| *prune_at)
            .min()
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
                EventKind::Value { data } => {
                    if let Some((mut done_tx, _)) = self.take_response(event.seq) {
                        let return_or_value_bytes = data.as_slice().to_vec();
                        if done_tx.send(Ok(return_or_value_bytes)).is_err() {
                            warn!("failed to send done notification: {:?}", &event.seq);
                        }
                    } else {
                        warn!("unknown seq: {:?}", &event.seq);
                    }
                }
                EventKind::Written => {
                    if let Some((mut done_tx, _)) = self.take_response(event.seq) {
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
                        EventKind::StreamSideband { sideband, .. } => {
                            StreamEvent::Sideband(sideband)
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
            },
            Err(e) => {
                if let Some((mut done_tx, _)) = self.take_response(event.seq) {
                    _ = done_tx.send(Err(Error::RemoteError(e.make_owned())));
                } else {
                    warn!("unknown seq {:?} for remote err {e:?}", &event.seq);
                }
            }
        }
    }

    fn take_response(&mut self, seq: SeqTy) -> Option<(ResponseSenderWrapper, Instant)> {
        let r = self.response_map.remove(&seq);
        if r.is_some() {
            self.freed.push(seq);
        }
        r
    }

    fn cancel_all_requests(&mut self) {
        trace!("canceling all requests");
        for (seq, (mut done_tx, _)) in self.response_map.drain() {
            _ = done_tx.send(Err(Error::Disconnected));
            self.freed.push(seq);
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
