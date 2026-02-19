use crate::introspect::Introspect;
use crate::prepared_call::PreparedCall;
use crate::rx_dispatcher::{DispatcherCommand, DispatcherMessage};
use crate::stream::Stream;
use crate::{Command, DeviceFilter, Error, OnError, PreparedRead, PreparedWrite, SeqTy, Sink};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UNib32};
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use ww_client_server::{PathKind, PathKindOwned, RequestKindOwned, StreamSidebandCommand};
use ww_version::{CompactVersion, FullVersionOwned};

/// Entry point for an API root or API trait implementation. Inside - wrapper over a channel sender half (currently tokio::mpsc::UnboundedSender).
///
/// Commands sent through this channel are received by a worker thread (e.g., USB or WebSocket clients) and forwarded to a connected device.
/// Replies are received through one-shot channels created on the fly when requests are sent.
#[derive(Clone)]
pub struct CommandSender {
    transport_cmd_tx: mpsc::UnboundedSender<Command>,
    // TODO: in tests this can arrive later than event with an answer (fixed with delay?), even though cmd are sent first, happens on real hw?
    dispatcher_cmd_tx: mpsc::UnboundedSender<DispatcherCommand>,
    seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    // TODO: CommandSender outstanding request limit?
    /// * None for command sender attached to API root, trait addressing will result in an error.
    /// * Some(empty path) for trait implemented at root level (unknown path), trait addressing will be used.
    /// * Some(non-empty path) for trait implemented at some particular path, trait addressing will be substituted with an actual path.
    ///
    /// "Some" cases are only for trait clients (client = "trait_client"). API root client (client = "full_client") uses absolute addressing.
    base_path: Option<Vec<UNib32>>,

    /// Mapping from FullVersion (crate name as string + semver) to trait GID of much smaller size.
    /// Optimally, should be requested from a device, so that newly assigned GIDs not yet known to a device are not used.
    /// But can also be forced to a known GID if performance is critical.
    gid_map: HashMap<FullVersionOwned, CompactVersion>,
    timeout: Option<Duration>,
}

pub(crate) struct TransportCommander {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

pub(crate) struct DispatcherCommander {
    cmd_tx: mpsc::UnboundedSender<DispatcherCommand>,
}

impl CommandSender {
    pub fn new(
        transport_cmd_tx: mpsc::UnboundedSender<Command>,
        dispatcher_msg_rx: mpsc::UnboundedReceiver<DispatcherMessage>,
    ) -> Self {
        let (seq_tx, seq_rx) = mpsc::channel(16); // TODO: increase to 1024?
        let (dispatcher_cmd_tx, dispatcher_cmd_rx) = mpsc::unbounded_channel(); // TODO: bounded cmd channel?
        tokio::spawn(async move {
            crate::rx_dispatcher::rx_dispatcher(dispatcher_cmd_rx, dispatcher_msg_rx).await;
        });
        _ = dispatcher_cmd_tx.send(DispatcherCommand::RegisterSeqSource { seq_tx });
        Self {
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx: Arc::new(RwLock::new(seq_rx)),
            base_path: None,
            gid_map: HashMap::new(),
            timeout: None,
        }
    }

    /// Set timeout that is used by default by this CommandSender.
    /// Default is None, in which case transport layer will use its own default timeout.
    /// Individual timeouts for each action are also supported, for example see [PreparedCall::with_timeout].
    pub fn set_local_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    pub async fn connect(
        &mut self,
        filter: DeviceFilter,
        user_protocol_version: FullVersionOwned,
        on_error: OnError,
    ) -> Result<(), Error> {
        let connected_rx = self.connect_inner(filter, user_protocol_version, on_error)?;
        let connection_result = connected_rx.await.map_err(|_| Error::EventLoopNotRunning)?;
        connection_result?;
        Ok(())
    }

    pub fn connect_blocking(
        &mut self,
        filter: DeviceFilter,
        user_protocol_version: FullVersionOwned,
        on_error: OnError,
    ) -> Result<(), Error> {
        let connected_rx = self.connect_inner(filter, user_protocol_version, on_error)?;
        let connection_result = connected_rx
            .blocking_recv()
            .map_err(|_| Error::EventLoopNotRunning)?;
        connection_result?;
        Ok(())
    }

    fn connect_inner(
        &mut self,
        filter: DeviceFilter,
        user_protocol_version: FullVersionOwned,
        on_error: OnError,
    ) -> Result<oneshot::Receiver<Result<(), Error>>, Error> {
        let (connected_tx, connected_rx) = oneshot::channel();
        self.transport_cmd_tx
            .send(Command::Connect {
                filter,
                user_protocol_version,
                on_error,
                connected_tx: Some(connected_tx),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(connected_rx)
    }

    pub fn send(&self, command: Command) -> Result<(), Error> {
        // TODO: Add command tx limit?
        self.transport_cmd_tx
            .send(command)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub fn prepare_call<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
        args: Result<Vec<u8>, Error>,
    ) -> PreparedCall<T> {
        let path_kind = self.to_ww_client_server_path(path); // postpone error return to have a better syntax
        PreparedCall {
            transport_cmd_tx: TransportCommander {
                cmd_tx: self.transport_cmd_tx.clone(),
            },
            dispatcher_cmd_tx: DispatcherCommander {
                cmd_tx: self.dispatcher_cmd_tx.clone(),
            },
            seq_rx: self.seq_rx.clone(),
            path_kind,
            args,
            timeout: self.timeout,
            _phantom: PhantomData,
        }
    }

    pub fn prepare_read<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> PreparedRead<T> {
        let path_kind = self.to_ww_client_server_path(path); // postpone error return to have a better syntax
        PreparedRead {
            transport_cmd_tx: TransportCommander {
                cmd_tx: self.transport_cmd_tx.clone(),
            },
            dispatcher_cmd_tx: DispatcherCommander {
                cmd_tx: self.dispatcher_cmd_tx.clone(),
            },
            seq_rx: self.seq_rx.clone(),
            path_kind,
            timeout: self.timeout,
            _phantom: PhantomData,
        }
    }

    pub fn prepare_write<E: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
        value: Result<Vec<u8>, Error>,
    ) -> PreparedWrite<E> {
        let path_kind = self.to_ww_client_server_path(path); // postpone error return to have a better syntax
        PreparedWrite {
            transport_cmd_tx: TransportCommander {
                cmd_tx: self.transport_cmd_tx.clone(),
            },
            dispatcher_cmd_tx: DispatcherCommander {
                cmd_tx: self.dispatcher_cmd_tx.clone(),
            },
            seq_rx: self.seq_rx.clone(),
            path_kind,
            value,
            timeout: self.timeout,
            _phantom_err: PhantomData,
        }
    }

    pub fn prepare_stream<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Stream<T>, Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        // notify rx dispatcher
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnStreamEvent {
                path_kind: path_kind.clone(),
                stream_event_tx: tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Stream {
            transport_cmd_tx: TransportCommander {
                cmd_tx: self.transport_cmd_tx.clone(),
            },
            // dispatcher_cmd_tx: DispatcherCommander {
            //     cmd_tx: self.dispatcher_cmd_tx.clone(),
            // },
            // seq_rx: self.seq_rx.clone(),
            path_kind,
            rx,
            _phantom: PhantomData,
        })
    }

    pub fn prepare_sink<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        // notify rx dispatcher
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnStreamEvent {
                path_kind: path_kind.clone(),
                stream_event_tx: tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Sink {
            transport_cmd_tx: TransportCommander {
                cmd_tx: self.transport_cmd_tx.clone(),
            },
            // dispatcher_cmd_tx: DispatcherCommander {
            //     cmd_tx: self.dispatcher_cmd_tx.clone(),
            // },
            // seq_rx: self.seq_rx.clone(),
            path_kind,
            _sideband_rx: rx,
            _phantom: PhantomData,
            scratch: [0u8; 1024],
        })
    }

    pub async fn next_seq(&mut self) -> Result<SeqTy, Error> {
        let mut seq_rx = self.seq_rx.write().await;
        let seq = seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?;
        Ok(seq)
    }

    pub fn next_seq_blocking(&mut self) -> Result<SeqTy, Error> {
        let mut seq_rx = self.seq_rx.blocking_write();
        let seq = seq_rx
            .blocking_recv()
            .ok_or(Error::RxDispatcherNotRunning)?;
        Ok(seq)
    }

    pub fn base_path(&self) -> Option<&Vec<UNib32>> {
        self.base_path.as_ref()
    }

    pub fn set_base_path(&mut self, base_path: Vec<UNib32>) {
        self.base_path = Some(base_path);
    }

    pub fn introspect_blocking(&mut self) -> Result<Introspect, Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        let seq = self.next_seq_blocking()?;
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            kind: RequestKindOwned::Introspect,
        };
        let mut scratch = [0u8; 1024]; // TODO: use Vec flavor or recycle?
        let req = req.to_ww_bytes(&mut scratch)?;
        self.transport_cmd_tx
            .send(Command::SendMessage {
                bytes: req.to_vec(),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        // notify rx dispatcher
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnIntrospect { bytes_chunk_tx: tx })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Introspect::new(rx))
    }

    fn to_ww_client_server_path(&self, path: PathKind<'_>) -> Result<PathKindOwned, Error> {
        if matches!(path, PathKind::Absolute { .. }) && self.base_path.is_some() {
            return Err(Error::User(
                "CommandSender configured as trait attachment, but used with absolute path".into(),
            ));
        }
        let path_kind = match path {
            PathKind::Absolute { path } => PathKindOwned::Absolute {
                path: path.iter().collect::<Result<Vec<_>, _>>()?,
            },
            PathKind::GlobalCompact {
                gid,
                path_from_trait,
            } => {
                if let Some(base) = &self.base_path {
                    let mut path = base.clone();
                    for n in path_from_trait.iter() {
                        path.push(n?);
                    }
                    PathKindOwned::Absolute { path }
                } else {
                    PathKindOwned::GlobalCompact {
                        gid,
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
            PathKind::GlobalFull {
                gid,
                path_from_trait,
            } => {
                if let Some(base) = &self.base_path {
                    let mut path = base.clone();
                    for n in path_from_trait.iter() {
                        path.push(n?);
                    }
                    PathKindOwned::Absolute { path }
                } else if let Some(compact) = self.gid_map.get(&gid.make_owned()) {
                    // TODO: actually not possible to implement Borrow for FullVersionOwned?
                    PathKindOwned::GlobalCompact {
                        gid: *compact,
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                } else {
                    PathKindOwned::GlobalFull {
                        gid: gid.make_owned(),
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
        };
        Ok(path_kind)
    }
}

impl TransportCommander {
    pub(crate) fn send_call_request(
        &self,
        seq: SeqTy,
        path_kind: PathKindOwned,
        args: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Call { args },
        };
        let mut scratch = [0u8; 1024]; // TODO: use Vec flavor or recycle?
        let req = req.to_ww_bytes(&mut scratch)?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req.to_vec(),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) fn send_read_request(
        &self,
        seq: SeqTy,
        path_kind: PathKindOwned,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Read,
        };
        let mut scratch = [0u8; 1024]; // TODO: use Vec flavor or recycle?
        let req = req.to_ww_bytes(&mut scratch)?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req.to_vec(),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) fn send_write_request(
        &self,
        seq: SeqTy,
        path_kind: PathKindOwned,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Write { data: value },
        };
        let mut scratch = [0u8; 1024]; // TODO: use Vec flavor or recycle?
        let req = req.to_ww_bytes(&mut scratch)?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req.to_vec(),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) fn send_stream_sideband(
        &self,
        seq: SeqTy,
        path_kind: PathKindOwned,
        sideband_cmd: StreamSidebandCommand,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::StreamSideband { sideband_cmd },
        };
        let mut scratch = [0u8; 1024]; // TODO: use Vec flavor or recycle?
        let req = req.to_ww_bytes(&mut scratch)?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req.to_vec(),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }
}

impl DispatcherCommander {
    /// Notify rx dispatcher about a new call request with seq
    pub(crate) fn on_call_return(
        &self,
        seq: SeqTy,
        timeout: Option<Duration>,
    ) -> Result<oneshot::Receiver<Result<Vec<u8>, Error>>, Error> {
        let (done_tx, done_rx) = oneshot::channel();
        self.cmd_tx
            .send(DispatcherCommand::OnCallReturn {
                seq,
                done_tx,
                timeout,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(done_rx)
    }

    /// Notify rx dispatcher about a new read request with seq
    pub(crate) fn on_read_return(
        &self,
        seq: SeqTy,
        timeout: Option<Duration>,
    ) -> Result<oneshot::Receiver<Result<Vec<u8>, Error>>, Error> {
        let (done_tx, done_rx) = oneshot::channel();
        self.cmd_tx
            .send(DispatcherCommand::OnReadValue {
                seq,
                done_tx,
                timeout,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(done_rx)
    }

    /// Notify rx dispatcher about a new write request with seq
    pub(crate) fn on_write_return(
        &self,
        seq: SeqTy,
        timeout: Option<Duration>,
    ) -> Result<oneshot::Receiver<Result<Vec<u8>, Error>>, Error> {
        let (done_tx, done_rx) = oneshot::channel();
        self.cmd_tx
            .send(DispatcherCommand::OnWriteComplete {
                seq,
                done_tx,
                timeout,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(done_rx)
    }
}
