use crate::event_loop::commander::TransportCommander;
use crate::event_loop::rx_dispatcher::StreamUpdateReceiver;
use crate::{Error, StreamEvent};
use std::fmt::{Debug, Display, Formatter};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::{ErrorKindOwned, PathKindOwned, StreamSideband};

/// Called with the deserialized value and its bytes once a multi-chunk reply is received, e.g. to cache it.
pub(crate) type OnDone<T> = Box<dyn FnOnce(&mut T, &[u8]) + Send + Sync>;

/// Result of a call, property read/write or introspection request, polled from synchronous code.
///
/// Main use case is immediate mode UI (e.g. egui): a promise is created once (see
/// [PreparedCall::call_promise](crate::PreparedCall::call_promise),
/// [PreparedRead::read_promise](crate::PreparedRead::read_promise),
/// [PreparedWrite::write_promise](crate::PreparedWrite::write_promise) and
/// [Introspect::get_promise](crate::Introspect::get_promise)), stored in the UI state and then polled on every frame
/// with [ready](Self::ready), [take_ready](Self::take_ready), [sync_poll](Self::sync_poll) + [state](Self::state), etc.
/// Polling never waits for the device response, it only checks whether one has arrived.
///
/// Despite the name, this is **not** a [Future] and must **not** be polled from async code (e.g. from a tokio task):
/// * The request is sent lazily on the first poll, using tokio's `blocking_send`, which panics when called from
///   within an async runtime context. It can also block the thread briefly if the command queue is full.
/// * Nothing wakes the task up when a response arrives, a promise only makes progress when it is polled again.
///
/// In async code use `call()`, `read()`, `write()` or `Introspect::get()` instead.
///
/// Nothing is sent until the promise is polled at least once. Dropping a promise that is still waiting logs a warning,
/// the request is not cancelled, but its response is discarded.
///
/// `marker` is a user-provided name only used in [Display] output and log messages.
pub struct Promise<T> {
    state: StateInner<T>,
    marker: &'static str, // TODO: change to enum Marker { Static, Owned } with into
    seen: bool,
    // TODO: Add instant
}

impl<T> Default for Promise<T> {
    fn default() -> Self {
        Promise {
            state: StateInner::None,
            marker: "",
            seen: false,
        }
    }
}

#[derive(Default)]
enum StateInner<T> {
    #[default]
    None,
    WaitingForSeqCall {
        path_kind: Option<PathKindOwned>,
        args: Option<Vec<u8>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
    },
    WaitingForSeqRead {
        path_kind: Option<PathKindOwned>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
    },
    WaitingForSeqWrite {
        path_kind: Option<PathKindOwned>,
        value: Option<Vec<u8>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
    },
    WaitingForIntrospect {
        transport_cmd_tx: TransportCommander,
        idle_timeout: Duration,
        on_done: Option<OnDone<T>>,
    },
    WaitingForReply(oneshot::Receiver<Result<Vec<u8>, Error>>),
    ReceivingIntrospect {
        rx: StreamUpdateReceiver,
        bytes: Vec<u8>,
        idle_timeout: Duration,
        deadline: Instant,
        on_done: Option<OnDone<T>>,
    },
    Future(oneshot::Receiver<Result<T, Error>>),
    Done(Option<T>), // Option used here to make Drop and take() work
    Err(Error),
}

/// Snapshot of a [Promise] state, returned by [Promise::state].
pub enum PromiseState<'i, T> {
    /// Created with [Promise::empty] or [Default], or data/error was already taken out.
    Empty,
    /// Request not yet sent (promise was never polled) or response not yet received.
    Waiting,
    Done(&'i T),
    Err(&'i Error),
}

// Debug: only needed to deserialize ww_client_server::ErrorKind::UserBytes into user error
impl<T: DeserializeShrinkWrapOwned + Debug> Promise<T> {
    pub fn empty(marker: &'static str) -> Self {
        Self {
            state: StateInner::None,
            marker,
            seen: false,
        }
    }

    pub fn done(value: T, marker: &'static str) -> Self {
        Self {
            state: StateInner::Done(Some(value)),
            marker,
            seen: false,
        }
    }

    pub fn error(error: Error, marker: &'static str) -> Self {
        Self {
            state: StateInner::Err(error),
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_call(
        path_kind: PathKindOwned,
        args: Vec<u8>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: StateInner::WaitingForSeqCall {
                path_kind: Some(path_kind),
                args: Some(args),
                timeout,
                transport_cmd_tx,
            },
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_read(
        path_kind: PathKindOwned,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: StateInner::WaitingForSeqRead {
                path_kind: Some(path_kind),
                timeout,
                transport_cmd_tx,
            },
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_write(
        path_kind: PathKindOwned,
        value: Vec<u8>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: StateInner::WaitingForSeqWrite {
                path_kind: Some(path_kind),
                value: Some(value),
                timeout,
                transport_cmd_tx,
            },
            marker,
            seen: false,
        }
    }

    /// Introspect data is received in chunks, each [Self::sync_poll] takes all the chunks received so far.
    /// Fails if no chunk is received within `idle_timeout`.
    pub(crate) fn new_introspect(
        transport_cmd_tx: TransportCommander,
        idle_timeout: Duration,
        on_done: OnDone<T>,
        marker: &'static str,
    ) -> Self {
        Self {
            state: StateInner::WaitingForIntrospect {
                transport_cmd_tx,
                idle_timeout,
                on_done: Some(on_done),
            },
            marker,
            seen: false,
        }
    }

    pub fn new_future(done_rx: oneshot::Receiver<Result<T, Error>>, marker: &'static str) -> Self {
        Self {
            state: StateInner::Future(done_rx),
            marker,
            seen: false,
        }
    }

    /// Polls the promise and either returns a reference to the data or [None] if still pending.
    /// Note that error is also an option, but this method ignores it.
    pub fn ready(&mut self) -> Option<&T> {
        self.sync_poll();
        if let StateInner::Done(response) = &self.state {
            response.as_ref()
        } else {
            None
        }
    }

    /// Same as [Self::ready], but returns a mutable reference.
    pub fn ready_mut(&mut self) -> Option<&mut T> {
        self.sync_poll();
        if let StateInner::Done(response) = &mut self.state {
            response.as_mut()
        } else {
            None
        }
    }

    /// Polls the promise and returns a reference to the data only once, the first time it is ready.
    /// Useful to react to a response exactly once, e.g. to copy it into UI state.
    pub fn ready_if_unseen(&mut self) -> Option<&T> {
        self.sync_poll();
        if let StateInner::Done(response) = &self.state {
            if self.seen {
                None
            } else {
                self.seen = true;
                response.as_ref()
            }
        } else {
            None
        }
    }

    /// Polls the promise and moves the data out if it is ready, leaving the promise empty.
    /// An error is ignored and stays in the promise.
    pub fn take_ready(&mut self) -> Option<T> {
        self.sync_poll();
        if !matches!(self.state, StateInner::Done(_)) {
            return None;
        }
        if let StateInner::Done(ref mut response) = core::mem::take(&mut self.state) {
            response.take()
            // Some(response)
        } else {
            None
        }
    }

    /// Polls the promise and moves the data or the error out, leaving the promise empty.
    /// Returns `Ok(None)` if still pending or already empty.
    pub fn take(&mut self) -> Result<Option<T>, String> {
        if let Some(r) = self.take_ready() {
            Ok(Some(r))
        } else if let Some(e) = self.peek_error() {
            let e = format!("{e}");
            self.state = StateInner::None;
            Err(e)
        } else {
            Ok(None)
        }
    }

    /// Returns a reference to the data if it is ready, without polling.
    pub fn peek_done(&self) -> Option<&T> {
        if let StateInner::Done(response) = &self.state {
            response.as_ref()
        } else {
            None
        }
    }

    /// Returns a reference to the error if the request failed, without polling.
    pub fn peek_error(&self) -> Option<&Error> {
        if let StateInner::Err(e) = &self.state {
            Some(e)
        } else {
            None
        }
    }

    /// Makes progress: sends the request on the first call, afterward checks whether a response has arrived.
    ///
    /// Must be called from synchronous code, see [Promise] docs. Doesn't wait for a response,
    /// but might block briefly on the first call if the command queue is full.
    pub fn sync_poll(&mut self) {
        match &self.state {
            StateInner::WaitingForSeqCall { .. } => {
                let no_more_work = self.send_call();
                if no_more_work {
                    return;
                }
            }
            StateInner::WaitingForSeqRead { .. } => {
                let no_more_work = self.send_read();
                if no_more_work {
                    return;
                }
            }
            StateInner::WaitingForSeqWrite { .. } => {
                let no_more_work = self.send_write();
                if no_more_work {
                    return;
                }
            }
            StateInner::WaitingForIntrospect { .. } => {
                let no_more_work = self.send_introspect();
                if no_more_work {
                    return;
                }
            }
            _ => {}
        }
        match &mut self.state {
            StateInner::WaitingForReply(rx) => match rx.try_recv() {
                Ok(response) => match response {
                    Ok(bytes) => {
                        self.state = StateInner::from_ww_bytes_owned(&bytes);
                    }
                    Err(e) => {
                        if let Error::RemoteError(remote) = &e
                            && let ErrorKindOwned::UserBytes(bytes) = &remote.kind
                        {
                            match T::from_ww_bytes_owned(bytes) {
                                Ok(err) => {
                                    self.state = StateInner::Err(Error::RemoteErrorDes(format!(
                                        "Error {{ err_seq: {}, user error: {:?} }}",
                                        remote.err_seq, err
                                    )))
                                }
                                Err(e) => {
                                    self.state = StateInner::Err(Error::RemoteErrorDes(format!(
                                        "Error {{ err_seq: {}, failed to deserialize user error: {:?} }}",
                                        remote.err_seq, e
                                    )))
                                }
                            }
                        } else {
                            self.state = StateInner::Err(e);
                        }
                    }
                },
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.state = StateInner::Err(Error::RxDispatcherNotRunning);
                }
            },
            StateInner::ReceivingIntrospect {
                rx,
                bytes,
                idle_timeout,
                deadline,
                on_done,
            } => loop {
                match rx.try_recv() {
                    Ok(StreamEvent::Data(chunk)) => {
                        bytes.extend_from_slice(&chunk);
                        *deadline = Instant::now() + *idle_timeout;
                    }
                    Ok(StreamEvent::Connected) => {}
                    Ok(StreamEvent::Sideband(StreamSideband::Close)) => {
                        self.state = if bytes.is_empty() {
                            StateInner::Err(Error::Other(
                                "device did not provide introspection data".into(),
                            ))
                        } else {
                            match T::from_ww_bytes_owned(bytes) {
                                Ok(mut value) => {
                                    if let Some(on_done) = on_done.take() {
                                        on_done(&mut value, bytes);
                                    }
                                    StateInner::Done(Some(value))
                                }
                                Err(e) => StateInner::Err(e.into()),
                            }
                        };
                        break;
                    }
                    Ok(o) => {
                        self.state = StateInner::Err(Error::Other(format!(
                            "unexpected stream event: {o:?}"
                        )));
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        if Instant::now() >= *deadline {
                            self.state = StateInner::Err(Error::Timeout);
                        }
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.state = StateInner::Err(Error::RxDispatcherNotRunning);
                        break;
                    }
                }
            },
            StateInner::Future(rx) => {
                if let Ok(rx) = rx.try_recv() {
                    match rx {
                        Ok(val) => self.state = StateInner::Done(Some(val)),
                        Err(e) => self.state = StateInner::Err(e),
                    }
                }
            }
            _ => {}
        }
    }

    // noinspection DuplicatedCode
    // Extracting methods or macros takes the same number of lines and makes things more confusing.
    fn send_call(&mut self) -> bool {
        if let StateInner::WaitingForSeqCall {
            path_kind,
            args,
            timeout,
            transport_cmd_tx,
        } = &mut self.state
        {
            // late error return
            let (Some(path_kind), Some(args)) = (path_kind.take(), args.take()) else {
                self.state = StateInner::Err(Error::Other("internal state error".into()));
                return true;
            };
            // send call to a remote device through transport layer
            // this should not actually block, unless there are huge number of requests being generated
            match transport_cmd_tx.send_call_request_blocking(path_kind, args, *timeout) {
                Ok(done_rx) => {
                    self.state = StateInner::WaitingForReply(done_rx);
                }
                Err(e) => {
                    self.state = StateInner::Err(e);
                    return true;
                }
            }
        }
        false
    }

    // noinspection DuplicatedCode
    fn send_read(&mut self) -> bool {
        if let StateInner::WaitingForSeqRead {
            path_kind,
            timeout,
            transport_cmd_tx,
        } = &mut self.state
        {
            let Some(path_kind) = path_kind.take() else {
                self.state = StateInner::Err(Error::Other("internal state error".into()));
                return true;
            };
            // send call to a remote device through transport layer
            // this should not actually block, unless there are huge number of requests being generated
            match transport_cmd_tx.send_read_request_blocking(path_kind, *timeout) {
                Ok(done_rx) => {
                    self.state = StateInner::WaitingForReply(done_rx);
                }
                Err(e) => {
                    self.state = StateInner::Err(e);
                    return true;
                }
            }
        }
        false
    }

    // noinspection DuplicatedCode
    fn send_write(&mut self) -> bool {
        if let StateInner::WaitingForSeqWrite {
            path_kind,
            value,
            timeout,
            transport_cmd_tx,
        } = &mut self.state
        {
            let (Some(path_kind), Some(value)) = (path_kind.take(), value.take()) else {
                self.state = StateInner::Err(Error::Other("internal state error".into()));
                return true;
            };
            // send call to a remote device through transport layer
            // this should not actually block, unless there are huge number of requests being generated
            match transport_cmd_tx.send_write_request_blocking(path_kind, value, *timeout) {
                Ok(done_rx) => {
                    self.state = StateInner::WaitingForReply(done_rx);
                }
                Err(e) => {
                    self.state = StateInner::Err(e);
                    return true;
                }
            }
        }
        false
    }

    fn send_introspect(&mut self) -> bool {
        if let StateInner::WaitingForIntrospect {
            transport_cmd_tx,
            idle_timeout,
            on_done,
        } = &mut self.state
        {
            // send introspect request to a remote device through transport layer
            // this should not actually block, unless there are huge number of requests being generated
            match transport_cmd_tx.send_introspect_blocking(None) {
                Ok(rx) => {
                    self.state = StateInner::ReceivingIntrospect {
                        rx,
                        bytes: vec![],
                        idle_timeout: *idle_timeout,
                        deadline: Instant::now() + *idle_timeout,
                        on_done: on_done.take(),
                    };
                }
                Err(e) => {
                    self.state = StateInner::Err(e);
                    return true;
                }
            }
        }
        false
    }

    /// Returns the current state, without polling. Call [Self::sync_poll] first to make progress.
    pub fn state(&self) -> PromiseState<'_, T> {
        match &self.state {
            StateInner::None => PromiseState::Empty,
            StateInner::WaitingForSeqCall { .. }
            | StateInner::WaitingForSeqRead { .. }
            | StateInner::WaitingForSeqWrite { .. }
            | StateInner::WaitingForIntrospect { .. }
            | StateInner::WaitingForReply(_)
            | StateInner::ReceivingIntrospect { .. } => PromiseState::Waiting,
            StateInner::Future(_) => PromiseState::Waiting,
            StateInner::Done(value) => value
                .as_ref()
                .map(PromiseState::Done)
                .unwrap_or(PromiseState::Empty),
            StateInner::Err(e) => PromiseState::Err(e),
        }
    }
}

impl<T> Promise<T> {
    pub fn is_waiting(&self) -> bool {
        matches!(self.state, StateInner::WaitingForReply(_))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.state, StateInner::None)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, StateInner::Done(_))
    }

    pub fn is_err(&self) -> bool {
        matches!(self.state, StateInner::Err(_))
    }

    pub fn marker(&self) -> &str {
        self.marker
    }
}

impl<T: DeserializeShrinkWrapOwned> StateInner<T> {
    fn from_ww_bytes_owned(bytes: &[u8]) -> Self {
        match T::from_ww_bytes_owned(bytes) {
            Ok(reply) => StateInner::Done(Some(reply)),
            Err(e) => StateInner::Err(e.into()),
        }
    }
}

impl<T> Display for Promise<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Promise('{}')::", self.marker)?;
        match &self.state {
            StateInner::None => write!(f, "None"),
            StateInner::WaitingForSeqCall { .. } => write!(f, "WaitingForSeqCall"),
            StateInner::WaitingForSeqRead { .. } => write!(f, "WaitingForSeqRead"),
            StateInner::WaitingForSeqWrite { .. } => write!(f, "WaitingForSeqWrite"),
            StateInner::WaitingForIntrospect { .. } => write!(f, "WaitingForIntrospect"),
            StateInner::WaitingForReply(_) => write!(f, "Waiting"),
            StateInner::ReceivingIntrospect { .. } => write!(f, "ReceivingIntrospect"),
            StateInner::Future(_) => write!(f, "Future"),
            StateInner::Done(_) => write!(f, "Done"),
            StateInner::Err(e) => write!(f, "Err({e:?})"),
        }
    }
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        if matches!(self.state, StateInner::WaitingForSeqCall { .. }) {
            tracing::warn!(
                "Dropping Promise(marker='{}')::WaitingForSeq(T), likely an error",
                self.marker
            );
        }
        if let StateInner::WaitingForReply(_) = &self.state {
            tracing::warn!(
                "Dropping Promise(marker='{}')::Waiting(T), likely an error",
                self.marker
            );
        }
    }
}
