use crate::command_sender::TransportCommander;
use crate::rx_dispatcher::StreamUpdateReceiver;
use crate::{Error, StreamEvent};
use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::{ErrorKindOwned, PathKindOwned, StreamSidebandEvent};

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
    },
    WaitingForReply(oneshot::Receiver<Result<Vec<u8>, Error>>),
    WaitingForMultiReply(StreamUpdateReceiver, Vec<u8>),
    Future(oneshot::Receiver<Result<T, Error>>),
    Done(Option<T>), // Option used here to make Drop and take() work
    Err(Error),
}

pub enum PromiseState<'i, T> {
    Empty,
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

    pub(crate) fn new_introspect(
        transport_cmd_tx: TransportCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: StateInner::WaitingForIntrospect { transport_cmd_tx },
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

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        self.sync_poll();
        if let StateInner::Done(response) = &mut self.state {
            response.as_mut()
        } else {
            None
        }
    }

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

    pub fn peek_done(&self) -> Option<&T> {
        if let StateInner::Done(response) = &self.state {
            response.as_ref()
        } else {
            None
        }
    }

    pub fn peek_error(&self) -> Option<&Error> {
        if let StateInner::Err(e) = &self.state {
            Some(e)
        } else {
            None
        }
    }

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
            StateInner::WaitingForMultiReply(rx, acc) => match rx.try_recv() {
                Ok(chunk) => match chunk {
                    StreamEvent::Data(chunk) => {
                        acc.extend_from_slice(&chunk);
                    }
                    StreamEvent::Sideband(StreamSidebandEvent::Closed) => {
                        self.state = StateInner::from_ww_bytes_owned(acc);
                    }
                    StreamEvent::Connected => {}
                    o => {
                        self.state = StateInner::Err(Error::Other(format!(
                            "unexpected stream event: {:?}",
                            o
                        )));
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.state = StateInner::Err(Error::RxDispatcherNotRunning);
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
            match transport_cmd_tx.send_call_request(path_kind, args, *timeout) {
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
            match transport_cmd_tx.send_read_request(path_kind, *timeout) {
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
            match transport_cmd_tx.send_write_request(path_kind, value, *timeout) {
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
        if let StateInner::WaitingForIntrospect { transport_cmd_tx } = &mut self.state {
            // send introspect request to a remote device through transport layer
            match transport_cmd_tx.send_introspect(None) {
                // TODO: introspect: add timeout
                Ok(chunks_rx) => {
                    self.state = StateInner::WaitingForMultiReply(chunks_rx, vec![]);
                }
                Err(e) => {
                    self.state = StateInner::Err(e);
                    return true;
                }
            }
        }
        false
    }

    pub fn state(&self) -> PromiseState<'_, T> {
        match &self.state {
            StateInner::None => PromiseState::Empty,
            StateInner::WaitingForSeqCall { .. }
            | StateInner::WaitingForSeqRead { .. }
            | StateInner::WaitingForSeqWrite { .. }
            | StateInner::WaitingForIntrospect { .. }
            | StateInner::WaitingForReply(_)
            | StateInner::WaitingForMultiReply(_, _) => PromiseState::Waiting,
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
            StateInner::WaitingForMultiReply(_, _) => write!(f, "WaitingMulti"),
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
