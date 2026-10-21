use crate::client::observe::{LevelRoute, ObserveRouteHook};
use crate::client::positioned::Shared;
use crate::client::prepared_stream_open::decode_opened;
use crate::client::sink::SinkLink;
use crate::event_loop::command::{Forwarded, StreamOpenHook};
use crate::event_loop::core::FullRetry;
use crate::event_loop::event_queue::{EventReceiver, EventSender};
use crate::{Error, RawStreamEvent, SeqTy};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, trace, warn};
use wire_weaver::prelude::UVlq32Backfill;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrapOwned, UNib32};
use ww_client_server::{ErrorKind, EventKind, PathKindOwned, RequestKindOwned};

pub(crate) type ResponseSender = oneshot::Sender<Result<Vec<u8>, Error>>;
pub(crate) type ResponseReceiver = oneshot::Receiver<Result<Vec<u8>, Error>>;

pub(crate) type StreamUpdateSender = EventSender;
pub(crate) type StreamUpdateReceiver = EventReceiver;

const IGNORE_TIMER_DURATION: Duration = Duration::from_millis(1);

/// How often reliable streams' receivers are polled for `Ack` / `Resend` (they decide when to send).
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The seq of a timed out request stays in use until a reply to it arrives or this long after it was sent
/// (or until its own timeout, if longer), so that a late reply is not taken for the answer to a new request
/// with the same seq.
pub(crate) const LATE_REPLY_WINDOW: Duration = Duration::from_secs(10);

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
        /// The request opens a counted or reliable stream
        hook: Option<Box<StreamOpenHook>>,
        /// The request has a `GlobalCompact` path: sent again with this one on `TraitNotFound` (API-20)
        retry: Option<Box<FullRetry>>,
    },
    OnStreamEvent {
        path_kind: PathKindOwned,
        stream_event_tx: StreamUpdateSender,
        registered_tx: oneshot::Sender<()>,
        sink: Option<Arc<SinkLink>>,
    },
    ObserveRoute {
        hook: Box<ObserveRouteHook>,
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
    /// Waiting for an answer: response sender, timeout instant and [LATE_REPLY_WINDOW] end.
    response_map: HashMap<SeqTy, (ResponseSenderWrapper, Instant, Instant)>,
    /// Requests that open a counted or reliable stream, by seq
    stream_opens: HashMap<SeqTy, Box<StreamOpenHook>>,
    /// Requests with a `GlobalCompact` path, by seq: the same request with a `GlobalFull` path and the timeout
    full_retries: HashMap<SeqTy, (Box<FullRetry>, Duration)>,
    /// Requests the device answered `TraitNotFound` to with a compact path, to be sent again by tx with
    /// [Self::take_retries]
    retries: Vec<Retry>,
    /// Open counted and reliable streams, by path
    positioned: HashMap<Vec<UNib32>, Arc<Shared>>,
    /// Time base of the streams' receivers, and when they were polled last
    epoch: Option<Instant>,
    last_stream_poll: Option<Instant>,
    /// Disconnecting with `keep_streams`: counted and reliable streams wait for a resume on the next connection
    pub(crate) keep_streams: bool,
    /// Timed out requests, whose seq is not freed until a late reply arrives or [LATE_REPLY_WINDOW] ends.
    timed_out: HashMap<SeqTy, Instant>,
    stream_handlers: HashMap<Vec<UNib32>, Vec<StreamUpdateSender>>,
    /// Sinks registered on this connection, told when it goes and whether they are kept
    sinks: Vec<Weak<SinkLink>>,
    /// Observed trait levels, by path (CLT-28)
    observe_routes: HashMap<Vec<UNib32>, Arc<dyn LevelRoute>>,
    /// Forwarded requests waiting for their (last) answer (`Command::Forward`): tag, timeout instant and
    /// [LATE_REPLY_WINDOW] end, by seq.
    forwards: HashMap<SeqTy, (u64, Instant, Instant)>,
    /// `Command::ForwardSink`: answers to forwarded requests and events with seq 0 go here, raw
    pub(crate) forward_sink: Option<tokio::sync::mpsc::UnboundedSender<Forwarded>>,
    /// Seq numbers whose requests completed, were cancelled or timed out and passed [LATE_REPLY_WINDOW] since the
    /// last [Self::take_freed].
    /// The tx side allocates seq numbers and needs to know when they can be reused.
    freed: Vec<SeqTy>,
}

struct ResponseSenderWrapper(Option<ResponseSender>);

/// A request to send again with a `GlobalFull` path: the request, its response sender, timeout and stream hook.
pub(crate) type Retry = (
    Box<FullRetry>,
    ResponseSender,
    Duration,
    Option<Box<StreamOpenHook>>,
);

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
                hook,
                retry,
            } => {
                if self.respond_later(now, seq, done_tx, timeout) {
                    if let Some(hook) = hook {
                        self.stream_opens.insert(seq, hook);
                    }
                    if let Some(retry) = retry {
                        self.full_retries.insert(seq, (retry, timeout));
                    }
                }
            }
            DispatcherCommand::OnStreamEvent {
                path_kind,
                stream_event_tx,
                registered_tx,
                sink,
            } => {
                if let Some(sink) = sink {
                    self.sinks.retain(|s| s.strong_count() > 0);
                    self.sinks.push(Arc::downgrade(&sink));
                }
                if let PathKindOwned::Absolute { path } = path_kind {
                    // TODO: send Connected/Disconnected only on actual connect/disconnect, send status here instead
                    _ = stream_event_tx.send(self.is_connected_as_stream_event());
                    let listeners = self.stream_handlers.entry(path).or_default();
                    listeners.push(stream_event_tx);
                }
                // TODO: other path kinds
                _ = registered_tx.send(());
            }
            DispatcherCommand::ObserveRoute { hook } => {
                self.observe_routes.insert(hook.path, hook.route);
            }
        }
    }

    /// A forwarded request with this seq was sent, its answers go to the forward sink.
    pub fn expect_forward(&mut self, now: Instant, seq: SeqTy, tag: u64, timeout: Duration) {
        if !self.is_connected {
            self.forward_failed(tag, Error::Disconnected);
            self.freed.push(seq);
            return;
        }
        let prune_at = now + timeout;
        let release_at = prune_at.max(now + LATE_REPLY_WINDOW);
        self.forwards.insert(seq, (tag, prune_at, release_at));
    }

    /// Tell the forward sink that the request with `tag` won't be answered.
    pub fn forward_failed(&mut self, tag: u64, error: Error) {
        if let Some(sink) = &self.forward_sink {
            _ = sink.send(Forwarded::Failed { tag, error });
        }
    }

    /// An event for the forward sink: an answer to a forwarded request, or anything with seq 0. Whether it was.
    fn forward(&mut self, msg_bytes: &[u8], event: &ww_client_server::Event<'_>) -> bool {
        let Some(sink) = &self.forward_sink else {
            return false;
        };
        let seq = event.seq.0;
        if seq == 0 {
            _ = sink.send(Forwarded::Event(msg_bytes.to_vec()));
            return true;
        }
        let Some((tag, _, _)) = self.forwards.get(&seq) else {
            return false;
        };
        // stream data under the request's seq (introspection) comes before the answer
        let last = !matches!(
            event.result,
            Ok(EventKind::StreamData { .. }
                | EventKind::StreamItem { .. }
                | EventKind::PropertyUpdate { .. })
        );
        _ = sink.send(Forwarded::Reply {
            tag: *tag,
            event: msg_bytes.to_vec(),
            last,
        });
        if last {
            self.forwards.remove(&seq);
            self.freed.push(seq);
        }
        true
    }

    /// Seq numbers that became reusable since the last call.
    pub fn take_freed(&mut self) -> Vec<SeqTy> {
        std::mem::take(&mut self.freed)
    }

    /// Requests to send again with a `GlobalFull` path, since the last call.
    pub fn take_retries(&mut self) -> Vec<Retry> {
        std::mem::take(&mut self.retries)
    }

    fn respond_later(
        &mut self,
        now: Instant,
        seq: SeqTy,
        done_tx: ResponseSender,
        timeout: Duration,
    ) -> bool {
        if seq == 0 {
            _ = done_tx.send(Err(Error::User(
                "Requests with seq == 0 will not be answered".into(),
            )));
            return false;
        }
        if !self.is_connected {
            _ = done_tx.send(Err(Error::Disconnected));
            return false;
        }
        let prune_at = now + timeout;
        let release_at = prune_at.max(now + LATE_REPLY_WINDOW);
        let replaced = self.response_map.insert(
            seq,
            (ResponseSenderWrapper(Some(done_tx)), prune_at, release_at),
        );
        if let Some((mut done_tx, _, _)) = replaced {
            _ = done_tx.send(Err(Error::User(
                "Seq used for this request was used again".into(),
            )));
            // the seq is still in use by the new request, so it is not freed here
        }
        self.stream_opens.remove(&seq);
        self.full_retries.remove(&seq);
        true
    }

    /// Time out all requests that are due at `now` (or within [IGNORE_TIMER_DURATION] of it) and free the seq numbers
    /// of timed out requests that passed [LATE_REPLY_WINDOW].
    pub fn prune(&mut self, now: Instant) {
        let is_due = |at: &Instant| at.saturating_duration_since(now) < IGNORE_TIMER_DURATION;
        let due: Vec<SeqTy> = self
            .response_map
            .iter()
            .filter(|(_, (_, prune_at, _))| is_due(prune_at))
            .map(|(seq, _)| *seq)
            .collect();
        for seq in due {
            self.stream_opens.remove(&seq);
            self.full_retries.remove(&seq);
            if let Some((mut done_tx, _, release_at)) = self.response_map.remove(&seq) {
                _ = done_tx.send(Err(Error::Timeout));
                trace!("pruned {seq:?}");
                if is_due(&release_at) {
                    self.freed.push(seq);
                } else {
                    self.timed_out.insert(seq, release_at);
                }
            }
        }
        let due: Vec<SeqTy> = self
            .forwards
            .iter()
            .filter(|(_, (_, prune_at, _))| is_due(prune_at))
            .map(|(seq, _)| *seq)
            .collect();
        for seq in due {
            if let Some((tag, _, release_at)) = self.forwards.remove(&seq) {
                self.forward_failed(tag, Error::Timeout);
                if is_due(&release_at) {
                    self.freed.push(seq);
                } else {
                    self.timed_out.insert(seq, release_at);
                }
            }
        }
        self.timed_out.retain(|seq, release_at| {
            let keep = !is_due(release_at);
            if !keep {
                self.freed.push(*seq);
            }
            keep
        });
    }

    /// Earliest instant at which [Self::prune] has something to do, None if no requests are outstanding.
    pub fn next_prune_at(&self) -> Option<Instant> {
        self.response_map
            .values()
            .map(|(_, prune_at, _)| *prune_at)
            .chain(self.forwards.values().map(|(_, prune_at, _)| *prune_at))
            .chain(self.timed_out.values().copied())
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
                    for shared in self.positioned.values() {
                        shared.lock().disconnected(self.keep_streams);
                        shared.wake();
                    }
                    // kept ones are registered again when they are resumed
                    self.positioned.clear();
                    for route in self.observe_routes.values() {
                        route.on_disconnected(self.keep_streams);
                    }
                    self.observe_routes.clear();
                    // before their handles see `Disconnected`
                    for sink in self.sinks.drain(..).filter_map(|s| s.upgrade()) {
                        sink.disconnected(self.keep_streams);
                    }
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
        if self.forward(msg_bytes, &event) {
            return;
        }
        match event.result {
            Ok(event_kind) => match event_kind {
                EventKind::Value { data } => {
                    if let Some(hook) = self.stream_opens.remove(&event.seq.0) {
                        self.stream_opened(*hook, data.as_slice());
                    }
                    if let Some(mut done_tx) = self.take_response(event.seq.0) {
                        let return_or_value_bytes = data.as_slice().to_vec();
                        if done_tx.send(Ok(return_or_value_bytes)).is_err() {
                            debug!("nobody is waiting for the reply to {:?}", &event.seq);
                        }
                    } else {
                        self.unexpected_reply(event.seq.0);
                    }
                }
                EventKind::Written => {
                    if let Some(mut done_tx) = self.take_response(event.seq.0) {
                        if done_tx.send(Ok(vec![])).is_err() {
                            debug!("nobody is waiting for the reply to {:?}", &event.seq);
                        }
                    } else {
                        self.unexpected_reply(event.seq.0);
                    }
                }
                EventKind::StreamData { ref path, .. }
                | EventKind::StreamSideband { ref path, .. } => {
                    if event.seq.0 != 0 && matches!(event_kind, EventKind::StreamSideband { .. }) {
                        // reply to a stream sideband request, routed to the stream, releases the seq here
                        if let Some(mut done_tx) = self.take_response(event.seq.0) {
                            _ = done_tx.send(Ok(vec![]));
                        } else {
                            self.unexpected_reply(event.seq.0);
                        }
                    }
                    let ev = match event_kind {
                        EventKind::StreamData { data, .. } => {
                            RawStreamEvent::Data(data.as_slice().to_vec())
                        }
                        EventKind::StreamSideband { sideband, .. } => {
                            RawStreamEvent::Sideband(sideband)
                        }
                        _ => unreachable!(),
                    };
                    let path = path.iter().map(|p| p.unwrap()).collect::<Vec<_>>();
                    if let RawStreamEvent::Sideband(sideband) = &ev
                        && let Some(shared) = self.positioned.get(&path)
                    {
                        shared.lock().on_sideband(*sideband);
                        shared.wake();
                        return;
                    }
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
                EventKind::StreamItem { path, head, data } => {
                    let path = path.iter().map(|p| p.unwrap()).collect::<Vec<_>>();
                    match self.positioned.get(&path) {
                        Some(shared) => {
                            shared.lock().on_item(head, data.as_slice());
                            shared.wake();
                        }
                        None => trace!("stream event for {path:?}, which is not open"),
                    }
                }
                EventKind::PropertyUpdate {
                    path,
                    t,
                    missed,
                    sample_times,
                    data,
                } => {
                    let path = path.iter().map(|p| p.unwrap()).collect::<Vec<_>>();
                    match self.observe_routes.get(&path) {
                        Some(route) => route.on_update(
                            crate::event_loop::core::now(),
                            t,
                            sample_times.map(|times| times.iter().collect()),
                            missed.map_or(0, |m| m.0),
                            data.as_slice(),
                        ),
                        None => trace!("property update for {path:?}, nobody observes"),
                    }
                }
            },
            Err(e) => {
                let seq = event.seq.0;
                if matches!(e.kind(), ErrorKind::TraitNotFound)
                    && let Some((retry, timeout)) = self.full_retries.remove(&seq)
                    && let Some(mut done_tx) = self.take_response(seq)
                    && let Some(done_tx) = done_tx.0.take()
                {
                    // a device built before the trait got its ww_global id: ask again by crate and trait name
                    debug!(
                        "TraitNotFound for compact {:?}, asking again with GlobalFull",
                        retry.compact
                    );
                    let hook = self.stream_opens.remove(&seq);
                    self.retries.push((retry, done_tx, timeout, hook));
                } else if let Some(mut done_tx) = self.take_response(seq) {
                    _ = done_tx.send(Err(Error::RemoteError(e.make_owned())));
                } else {
                    self.unexpected_reply(event.seq.0);
                    debug!("remote err: {e:?}");
                }
            }
        }
    }

    /// The device answered a request that opens a counted or reliable stream.
    fn stream_opened(&mut self, hook: StreamOpenHook, bytes: &[u8]) {
        if !(hook.is_open)(bytes) {
            return;
        }
        let Ok(decoded) = decode_opened(bytes) else {
            return;
        };
        let mut inner = hook.shared.lock();
        if inner.closed {
            return;
        }
        inner.opened(hook.resume, &decoded.opened, decoded.codec);
        let path = inner.path.clone();
        drop(inner);
        hook.shared.wake();
        if let Some(old) = self.positioned.insert(path, hook.shared) {
            // opened again: the old handle gets nothing more
            old.lock().disconnected(false);
            old.wake();
        }
    }

    /// Sideband the streams' receivers want to send now (`Ack`, `Resend`), as requests.
    pub fn poll_streams(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let epoch = *self.epoch.get_or_insert(now);
        let now_ms = now.saturating_duration_since(epoch).as_millis() as u64;
        self.last_stream_poll = Some(now);
        let mut out = vec![];
        self.positioned.retain(|path, shared| {
            let mut inner = shared.lock();
            if inner.closed {
                return false;
            }
            while let Some(sideband) = inner.poll(now_ms) {
                let req = ww_client_server::RequestOwned {
                    seq: UVlq32Backfill(0),
                    path_kind: PathKindOwned::Absolute { path: path.clone() },
                    kind: RequestKindOwned::StreamSideband { sideband },
                };
                match req.to_ww_bytes_owned() {
                    Ok(bytes) => out.push(bytes),
                    Err(e) => warn!("stream sideband not serialized: {e:?}"),
                }
            }
            true
        });
        out
    }

    /// When [Self::poll_streams] should run next: reliable streams acknowledge and ask for resends on time.
    pub fn next_stream_poll_at(&self) -> Option<Instant> {
        if !self.is_connected || !self.positioned.values().any(|s| s.lock().is_reliable()) {
            return None;
        }
        Some(
            self.last_stream_poll
                .map_or_else(Instant::now, |t| t + STREAM_POLL_INTERVAL),
        )
    }

    fn take_response(&mut self, seq: SeqTy) -> Option<ResponseSenderWrapper> {
        self.full_retries.remove(&seq);
        let (done_tx, _, _) = self.response_map.remove(&seq)?;
        self.freed.push(seq);
        Some(done_tx)
    }

    /// Reply to a request that is not waited for: either a late reply to a timed out one, whose seq can be reused
    /// now, or an unknown seq.
    fn unexpected_reply(&mut self, seq: SeqTy) {
        if self.timed_out.remove(&seq).is_some() {
            debug!("late reply to timed out request {seq}, dropping");
            self.freed.push(seq);
        } else {
            warn!("unknown seq: {seq}");
        }
    }

    fn cancel_all_requests(&mut self) {
        trace!("canceling all requests");
        self.stream_opens.clear();
        self.full_retries.clear();
        for (seq, (mut done_tx, _, _)) in self.response_map.drain() {
            _ = done_tx.send(Err(Error::Disconnected));
            self.freed.push(seq);
        }
        let forwards: Vec<_> = self.forwards.drain().collect();
        for (seq, (tag, _, _)) in forwards {
            self.forward_failed(tag, Error::Disconnected);
            self.freed.push(seq);
        }
        self.freed
            .extend(self.timed_out.drain().map(|(seq, _)| seq));
    }

    fn is_connected_as_stream_event(&self) -> RawStreamEvent {
        if self.is_connected {
            RawStreamEvent::Connected
        } else {
            RawStreamEvent::Disconnected
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

impl Drop for RxDispatcher {
    fn drop(&mut self) {
        // the event loop is gone: handles see the end once their queue is empty
        for shared in self.positioned.values() {
            shared.lock().attached = false;
            shared.wake();
        }
        for route in self.observe_routes.values() {
            route.on_detached();
        }
    }
}
