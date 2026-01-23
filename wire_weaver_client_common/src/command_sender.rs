use crate::promise::Promise;
use crate::rx_dispatcher::{DispatcherCommand, DispatcherMessage};
use crate::{Command, DeviceFilter, Error, OnError, SeqTy, StreamEvent};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::trace;
use wire_weaver::prelude::{DeserializeShrinkWrap, DeserializeShrinkWrapOwned, UNib32};
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
    scratch: [u8; 4096],
    timeout: Option<Duration>,
}

/// Self-contained struct containing all necessary information needed to perform a call:
/// * TX ends towards transport and dispatcher event loops
/// * Serialized method arguments
/// * Resource path
/// * Return type as a generic `T` argument
///
/// When obtained, user can choose how to actually execute the call:
/// * async: `call()`
/// * blocking: `blocking_call()`
/// * call-ignoring-return value: `call_forget()`
/// * turn into a `Promise<T>` useful in immediate mode UI
#[must_use = "PrepareCall does nothing, unless call(), blocking_call(), call_forget() or call_promise() is used"]
pub struct PreparedCall<T> {
    transport_cmd_tx: TransportCommander,
    dispatcher_cmd_tx: DispatcherCommander,
    seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    path_kind: Result<PathKindOwned, Error>,
    args: Result<Vec<u8>, Error>,
    timeout: Option<Duration>,
    _phantom: PhantomData<T>,
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
            scratch: [0; 4096],
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

    fn send_request(
        &mut self,
        seq: SeqTy,
        path_kind: PathKindOwned,
        kind: RequestKindOwned,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind,
        };
        let req = req.to_ww_bytes(&mut self.scratch)?; // TODO: recycle Vec (serialize via BufWriter with Vec flavor)
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };
        self.send(cmd)?;
        Ok(())
    }

    pub fn prepare_call<'i, T: DeserializeShrinkWrap<'i>>(
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

    /// Sends write request to remote device, awaits response and returns.
    pub async fn send_write_receive_reply(
        &mut self,
        path: PathKind<'_>,
        value_bytes: Vec<u8>,
    ) -> Result<(), Error> {
        let seq = self.next_seq().await?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnWriteComplete {
                seq,
                done_tx,
                timeout: self.timeout,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send write command to remote device
        self.send_request(
            seq,
            path_kind,
            RequestKindOwned::Write { data: value_bytes },
        )?;
        let _data = self.receive_reply(done_rx, "write").await?;
        Ok(())
    }

    /// Sends write request to remote device with seq == 0.
    /// No response will be generated by remote device, nor it will be awaited.
    pub fn send_write_forget(
        &mut self,
        path: PathKind<'_>,
        value_bytes: Vec<u8>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        self.send_request(0, path_kind, RequestKindOwned::Write { data: value_bytes })?;
        Ok(())
    }

    /// Sends open sideband command to remote device and setups subscribes to stream events.
    pub fn send_stream_open(
        &mut self,
        path: PathKind<'_>,
        stream_event_tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;

        // notify rx dispatcher
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnStreamEvent {
                path_kind: path_kind.clone(),
                stream_event_tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send sideband command to remote device
        self.send_request(
            0,
            path_kind,
            RequestKindOwned::StreamSideband {
                sideband_cmd: StreamSidebandCommand::Open,
            },
        )?;
        Ok(())
    }

    /// Sends read request to remote device, awaits response and returns it.
    pub async fn send_read_receive_reply(&mut self, path: PathKind<'_>) -> Result<Vec<u8>, Error> {
        // obtain next seq and correct path
        let seq = self.next_seq().await?;
        let path_kind = self.to_ww_client_server_path(path)?;

        // notify rx dispatcher
        let (done_tx, done_rx) = oneshot::channel();
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnReadValue {
                seq,
                done_tx,
                timeout: self.timeout,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send read command to remote device
        self.send_request(seq, path_kind, RequestKindOwned::Read)?;

        // await response from remote device (through rx dispatcher)
        let data = self.receive_reply(done_rx, "read").await?;

        Ok(data)
    }

    #[deprecated]
    async fn receive_reply(
        &self,
        done_rx: oneshot::Receiver<Result<Vec<u8>, Error>>,
        desc: &'static str,
    ) -> Result<Vec<u8>, Error> {
        let rx_or_timeout = tokio::time::timeout(Duration::from_millis(100), done_rx).await;
        trace!("got {desc} response: {:02x?}", rx_or_timeout);
        let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
        let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
        let data = response?;
        Ok(data)
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
}

impl DispatcherCommander {
    /// Notify rx dispatcher about a new call with seq
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
}

impl<T: DeserializeShrinkWrapOwned> PreparedCall<T> {
    /// Use provided timeout instead of default one propagated from CommandSender
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            transport_cmd_tx: self.transport_cmd_tx,
            dispatcher_cmd_tx: self.dispatcher_cmd_tx,
            seq_rx: self.seq_rx,
            path_kind: self.path_kind,
            args: self.args,
            timeout: Some(timeout),
            _phantom: PhantomData,
        }
    }

    /// Send call request, await response (or timeout) and return it.
    pub async fn call(self) -> Result<T, Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        let path_kind = self.path_kind?;
        let args = self.args?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.write().await;
            seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_call_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_call_request(seq, path_kind, args)?;

        // await return value from remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send call request, block the thread until response is received (or timeout) and return it.
    pub fn blocking_call(self) -> Result<T, Error> {
        let path_kind = self.path_kind?;
        let args = self.args?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.blocking_write();
            seq_rx
                .blocking_recv()
                .ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_call_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_call_request(seq, path_kind, args)?;

        // await return value from remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send call request with seq = 0 and immediately return without response (remote end won't send it either).
    pub fn call_forget(self) -> Result<(), Error> {
        let path_kind = self.path_kind?;
        let args = self.args?;
        self.transport_cmd_tx
            .send_call_request(0, path_kind, args)?;
        Ok(())
    }

    /// Send call request and return a Promise that can be used to await a result. Useful for immediate mode UI.
    pub fn call_promise(self, marker: &'static str) -> Promise<T> {
        let path_kind = match self.path_kind {
            Ok(p) => p,
            Err(e) => return Promise::error(e, marker),
        };
        let args = match self.args {
            Ok(a) => a,
            Err(e) => return Promise::error(e, marker),
        };

        Promise::new(
            path_kind,
            args,
            self.seq_rx,
            self.timeout,
            self.transport_cmd_tx,
            self.dispatcher_cmd_tx,
            marker,
        )
    }
}
