use crate::command_sender::{DispatcherCommander, TransportCommander};
use crate::{Error, SeqTy};
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::{ErrorKindOwned, PathKindOwned};

pub struct Promise<T> {
    state: PromiseState<T>,
    marker: &'static str,
    seen: bool,
}

#[derive(Default)]
enum PromiseState<T> {
    #[default]
    None,
    WaitingForSeqCall {
        path_kind: Option<PathKindOwned>,
        args: Option<Vec<u8>>,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
    },
    WaitingForSeqRead {
        path_kind: Option<PathKindOwned>,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
    },
    WaitingForSeqWrite {
        path_kind: Option<PathKindOwned>,
        value: Option<Vec<u8>>,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
    },
    WaitingForIntrospect {
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
    },
    WaitingForReply(SeqTy, oneshot::Receiver<Result<Vec<u8>, Error>>),
    WaitingForMultiReply(SeqTy, mpsc::UnboundedReceiver<Vec<u8>>, Vec<u8>),
    Done(Option<T>), // Option used here to make Drop and take() work
    Err(Error),
}

macro_rules! obtain_next_seq_or_return {
    ($seq:ident, $seq_rx:ident, $self:ident) => {
        let mut seq_rx = $seq_rx.blocking_write();
        let $seq = match seq_rx.try_recv() {
            Ok(seq) => {
                drop(seq_rx);
                seq
            }
            Err(_) => {
                drop(seq_rx);
                $self.state = PromiseState::Err(Error::RxDispatcherNotRunning);
                return true;
            }
        };
    };
}

// Debug: only needed to deserialize ww_client_server::ErrorKind::UserBytes into user error
impl<T: DeserializeShrinkWrapOwned + Debug> Promise<T> {
    pub fn empty(marker: &'static str) -> Self {
        Self {
            state: PromiseState::None,
            marker,
            seen: false,
        }
    }

    pub fn error(error: Error, marker: &'static str) -> Self {
        Self {
            state: PromiseState::Err(error),
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_call(
        path_kind: PathKindOwned,
        args: Vec<u8>,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: PromiseState::WaitingForSeqCall {
                path_kind: Some(path_kind),
                args: Some(args),
                timeout,
                transport_cmd_tx,
                dispatcher_cmd_tx,
                seq_rx,
            },
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_read(
        path_kind: PathKindOwned,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: PromiseState::WaitingForSeqRead {
                path_kind: Some(path_kind),
                timeout,
                transport_cmd_tx,
                dispatcher_cmd_tx,
                seq_rx,
            },
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_write(
        path_kind: PathKindOwned,
        value: Vec<u8>,
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        timeout: Option<Duration>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: PromiseState::WaitingForSeqWrite {
                path_kind: Some(path_kind),
                value: Some(value),
                timeout,
                transport_cmd_tx,
                dispatcher_cmd_tx,
                seq_rx,
            },
            marker,
            seen: false,
        }
    }

    pub(crate) fn new_introspect(
        seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
        marker: &'static str,
    ) -> Self {
        Self {
            state: PromiseState::WaitingForIntrospect {
                transport_cmd_tx,
                dispatcher_cmd_tx,
                seq_rx,
            },
            marker,
            seen: false,
        }
    }

    /// Polls the promise and either returns a reference to the data or [None] if still pending.
    /// Note that error is also an option, but this method ignores it.
    pub fn ready(&mut self) -> Option<&T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &self.state {
            response.as_ref()
        } else {
            None
        }
    }

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &mut self.state {
            response.as_mut()
        } else {
            None
        }
    }

    pub fn ready_if_unseen(&mut self) -> Option<&T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &self.state {
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
        if !matches!(self.state, PromiseState::Done(_)) {
            return None;
        }
        if let PromiseState::Done(ref mut response) = core::mem::take(&mut self.state) {
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
            self.state = PromiseState::None;
            Err(e)
        } else {
            Ok(None)
        }
    }

    pub fn peek_error(&mut self) -> Option<String> {
        self.sync_poll();
        if let PromiseState::Err(e) = &self.state {
            Some(format!("{e:?}"))
        } else {
            None
        }
    }

    pub fn sync_poll(&mut self) {
        match &self.state {
            PromiseState::WaitingForSeqCall { .. } => {
                let no_more_work = self.send_call();
                if no_more_work {
                    return;
                }
            }
            PromiseState::WaitingForSeqRead { .. } => {
                let no_more_work = self.send_read();
                if no_more_work {
                    return;
                }
            }
            PromiseState::WaitingForSeqWrite { .. } => {
                let no_more_work = self.send_write();
                if no_more_work {
                    return;
                }
            }
            PromiseState::WaitingForIntrospect { .. } => {
                let no_more_work = self.send_introspect();
                if no_more_work {
                    return;
                }
            }
            _ => {}
        }
        match &mut self.state {
            PromiseState::WaitingForReply(_seq, rx) => match rx.try_recv() {
                Ok(response) => match response {
                    Ok(bytes) => match T::from_ww_bytes_owned(&bytes) {
                        Ok(reply) => {
                            self.state = PromiseState::Done(Some(reply));
                        }
                        Err(e) => {
                            self.state = PromiseState::Err(e.into());
                        }
                    },
                    Err(e) => {
                        if let Error::RemoteError(remote) = &e
                            && let ErrorKindOwned::UserBytes(bytes) = &remote.kind
                        {
                            match T::from_ww_bytes_owned(bytes) {
                                Ok(err) => {
                                    self.state = PromiseState::Err(Error::RemoteErrorDes(format!(
                                        "Error {{ err_seq: {}, user error: {:?} }}",
                                        remote.err_seq, err
                                    )))
                                }
                                Err(e) => {
                                    self.state = PromiseState::Err(Error::RemoteErrorDes(format!(
                                        "Error {{ err_seq: {}, failed to deserialize user error: {:?} }}",
                                        remote.err_seq, e
                                    )))
                                }
                            }
                        } else {
                            self.state = PromiseState::Err(e);
                        }
                    }
                },
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.state = PromiseState::Err(Error::RxDispatcherNotRunning);
                }
            },
            PromiseState::WaitingForMultiReply(_seq, rx, acc) => match rx.try_recv() {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        match T::from_ww_bytes_owned(acc) {
                            Ok(reply) => {
                                self.state = PromiseState::Done(Some(reply));
                            }
                            Err(e) => {
                                self.state = PromiseState::Err(e.into());
                            }
                        }
                    } else {
                        acc.extend_from_slice(&chunk);
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.state = PromiseState::Err(Error::RxDispatcherNotRunning);
                }
            },
            _ => {}
        }
    }

    // noinspection DuplicatedCode
    // Extracting methods or macros takes same amount of lines and makes things more confusing.
    fn send_call(&mut self) -> bool {
        if let PromiseState::WaitingForSeqCall {
            path_kind,
            args,
            timeout,
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
        } = &mut self.state
        {
            obtain_next_seq_or_return!(seq, seq_rx, self);

            // notify rx dispatcher & send call to remote device through transport layer
            let done_rx = match dispatcher_cmd_tx.on_call_return(seq, *timeout) {
                Ok(done_rx) => done_rx,
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            };
            let (Some(path_kind), Some(args)) = (path_kind.take(), args.take()) else {
                self.state = PromiseState::Err(Error::Other("internal state error".into()));
                return true;
            };
            match transport_cmd_tx.send_call_request(seq, path_kind, args) {
                Ok(_) => {
                    self.state = PromiseState::WaitingForReply(seq, done_rx);
                }
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            }
        }
        false
    }

    // noinspection DuplicatedCode
    fn send_read(&mut self) -> bool {
        if let PromiseState::WaitingForSeqRead {
            path_kind,
            timeout,
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
        } = &mut self.state
        {
            obtain_next_seq_or_return!(seq, seq_rx, self);

            // notify rx dispatcher & send call to remote device through transport layer
            let done_rx = match dispatcher_cmd_tx.on_read_return(seq, *timeout) {
                Ok(done_rx) => done_rx,
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            };
            let Some(path_kind) = path_kind.take() else {
                self.state = PromiseState::Err(Error::Other("internal state error".into()));
                return true;
            };
            match transport_cmd_tx.send_read_request(seq, path_kind) {
                Ok(_) => {
                    self.state = PromiseState::WaitingForReply(seq, done_rx);
                }
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            }
        }
        false
    }

    // noinspection DuplicatedCode
    fn send_write(&mut self) -> bool {
        if let PromiseState::WaitingForSeqWrite {
            path_kind,
            value,
            timeout,
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
        } = &mut self.state
        {
            obtain_next_seq_or_return!(seq, seq_rx, self);

            // notify rx dispatcher & send call to remote device through transport layer
            let done_rx = match dispatcher_cmd_tx.on_write_return(seq, *timeout) {
                Ok(done_rx) => done_rx,
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            };
            let (Some(path_kind), Some(value)) = (path_kind.take(), value.take()) else {
                self.state = PromiseState::Err(Error::Other("internal state error".into()));
                return true;
            };
            match transport_cmd_tx.send_write_request(seq, path_kind, value) {
                Ok(_) => {
                    self.state = PromiseState::WaitingForReply(seq, done_rx);
                }
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            }
        }
        false
    }

    fn send_introspect(&mut self) -> bool {
        if let PromiseState::WaitingForIntrospect {
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
        } = &mut self.state
        {
            obtain_next_seq_or_return!(seq, seq_rx, self);

            // notify rx dispatcher & send introspect request to a remote device through transport layer
            let chunks_rx = match dispatcher_cmd_tx.on_introspect_chunk() {
                Ok(chunks_rx) => chunks_rx,
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            };
            match transport_cmd_tx.send_introspect(seq) {
                Ok(_) => {
                    self.state = PromiseState::WaitingForMultiReply(seq, chunks_rx, vec![]);
                }
                Err(e) => {
                    self.state = PromiseState::Err(e);
                    return true;
                }
            }
        }
        false
    }
}

impl<T> Promise<T> {
    pub fn is_waiting(&self) -> bool {
        matches!(self.state, PromiseState::WaitingForReply(_, _))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.state, PromiseState::None)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, PromiseState::Done(_))
    }

    pub fn marker(&self) -> &str {
        self.marker
    }
}

impl<T> Display for Promise<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Promise('{}')::", self.marker)?;
        match &self.state {
            PromiseState::None => write!(f, "None"),
            PromiseState::WaitingForSeqCall { .. } => write!(f, "WaitingForSeqCall"),
            PromiseState::WaitingForSeqRead { .. } => write!(f, "WaitingForSeqRead"),
            PromiseState::WaitingForSeqWrite { .. } => write!(f, "WaitingForSeqWrite"),
            PromiseState::WaitingForIntrospect { .. } => write!(f, "WaitingForIntrospect"),
            PromiseState::WaitingForReply(seq, _) => write!(f, "Waiting(seq={seq})"),
            PromiseState::WaitingForMultiReply(seq, _, _) => write!(f, "WaitingMulti(seq={seq})"),
            PromiseState::Done(_) => write!(f, "Done"),
            PromiseState::Err(e) => write!(f, "Err({e:?})"),
        }
    }
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        if matches!(self.state, PromiseState::WaitingForSeqCall { .. }) {
            tracing::warn!(
                "Dropping Promise(marker='{}')::WaitingForSeq(T), likely an error",
                self.marker
            );
        }
        if let PromiseState::WaitingForReply(seq, _) = &self.state {
            tracing::warn!(
                "Dropping Promise(seq={}, marker='{}')::Waiting(T), likely an error",
                seq,
                self.marker
            );
        }
    }
}
