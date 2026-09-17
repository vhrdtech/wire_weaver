use crate::Error;
use crate::Introspect;
use crate::PreparedCall;
use crate::PreparedDisconnect;
use crate::Stream;
use crate::config::IntrospectBundle;
use crate::device_info::DeviceApiInfo;
use crate::event_loop::command::Command;
use crate::event_loop::rx_dispatcher::ResponseReceiver;
use crate::event_loop::rx_dispatcher::ResponseSender;
use crate::event_loop::rx_dispatcher::StreamUpdateReceiver;
use crate::{DEFAULT_REQUEST_TIMEOUT, PreparedRead, PreparedWrite, Sink};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UNib32};
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKind, PathKindOwned, RequestKindOwned, StreamSideband};
use ww_version::{CompactVersion, FullVersionOwned, VersionOwned};

// TODO: in tests dispatcher command can arrive later than event with an answer (fixed with delay?), even though cmd are sent first, happens on real hw?
// TODO: CommandSender outstanding request limit?
/// Entry point for an API root or API trait implementation. Inside - wrapper over a channel sender half (currently tokio::mpsc::UnboundedSender).
///
/// Commands sent through this channel are received by a worker thread (e.g., USB or WebSocket clients) and forwarded to a connected device.
/// Replies are received through one-shot channels created on the fly when requests are sent.
#[derive(Clone)]
pub struct Commander {
    transport_cmd_tx: mpsc::Sender<Command>,
    /// * None for command sender attached to API root, trait addressing will result in an error.
    /// * Some (empty path) for trait implemented at root level (unknown path), trait addressing will be used.
    /// * Some (non-empty path) for trait implemented at some particular path, trait addressing will be substituted with an actual path.
    ///
    /// "Some" cases are only for trait clients (`client = "trait_client"`). API root client (`client = "full_client"`) uses absolute addressing.
    base_path: Option<Vec<UNib32>>,

    /// Mapping from FullVersion (crate name as string + semver) to trait GID of a much smaller size.
    /// Optimally, should be requested from a device, so that newly assigned GIDs not yet known to a device are not used.
    /// But can also be forced to a known GID if performance is critical.
    gid_map: HashMap<FullVersionOwned, CompactVersion>,
    default_timeout: Duration,
    pub(crate) connected_device: DeviceApiInfo,
    /// Client API and types, client signature. None if DynClient.
    client_introspect: Option<IntrospectBundle>,
    /// Device API and types, client signature. None if device has introspect disabled and there is no cache for it.
    device_introspect: Option<IntrospectBundle>,
}

pub(crate) struct TransportCommander {
    cmd_tx: mpsc::Sender<Command>,
    default_timeout: Duration,
}

impl Commander {
    pub fn new(transport_cmd_tx: mpsc::Sender<Command>) -> Self {
        Self {
            transport_cmd_tx,
            base_path: None,
            gid_map: HashMap::new(),
            default_timeout: DEFAULT_REQUEST_TIMEOUT,
            connected_device: DeviceApiInfo::empty(),
            client_introspect: None,
            device_introspect: None,
        }
    }

    /// Set the timeout used by default by this CommandSender.
    /// Default is None, in which case the transport layer will use its own default timeout.
    /// Individual timeouts for each action are also supported, for example, see [PreparedCall::with_timeout].
    pub fn set_local_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }

    pub async fn send(&self, command: Command) -> Result<(), Error> {
        // TODO: Add command tx limit?
        self.transport_cmd_tx
            .send(command)
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub fn blocking_send(&self, command: Command) -> Result<(), Error> {
        // TODO: Add command tx limit?
        self.transport_cmd_tx
            .blocking_send(command)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub fn prepare_call<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
        args: Result<Vec<u8>, Error>,
    ) -> PreparedCall<T> {
        // postpone error return to have a better syntax (one ? instead of two)
        let since = None; // TODO: fix
        let (postpone_err, args) = match (self.check_version(since), args) {
            (Ok(_), Ok(args)) => (Ok(()), args),
            (Err(e), _) => (Err(e), vec![]),
            (_, Err(e)) => (Err(e), vec![]),
        };
        let (postpone_err, path_kind) = if postpone_err.is_ok() {
            match self.to_ww_client_server_path(path) {
                Ok(path_kind) => (Ok(()), path_kind),
                Err(e) => (Err(e), PathKindOwned::Absolute { path: vec![] }),
            }
        } else {
            (postpone_err, PathKindOwned::Absolute { path: vec![] })
        };
        PreparedCall {
            postpone_err,
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            args,
            timeout_override: None,
            _phantom: PhantomData,
        }
    }

    pub fn prepare_read<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> PreparedRead<T> {
        let since = None; // TODO: fix
        let version_check = self.check_version(since);
        let path_kind = self.to_ww_client_server_path(path); // postpone error return to have a better syntax
        PreparedRead {
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            version_check,
            path_kind,
            timeout_override: None,
            _phantom: PhantomData,
        }
    }

    pub fn prepare_write<E: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
        value: Result<Vec<u8>, Error>,
    ) -> PreparedWrite<E> {
        let since = None; // TODO: fix
        let (postpone_err, value) = match (self.check_version(since), value) {
            (Ok(_), Ok(value)) => (Ok(()), value),
            (Err(e), _) => (Err(e), vec![]),
            (_, Err(e)) => (Err(e), vec![]),
        };
        let (postpone_err, path_kind) = if postpone_err.is_ok() {
            match self.to_ww_client_server_path(path) {
                Ok(path_kind) => (Ok(()), path_kind),
                Err(e) => (Err(e), PathKindOwned::Absolute { path: vec![] }),
            }
        } else {
            (postpone_err, PathKindOwned::Absolute { path: vec![] })
        };
        PreparedWrite {
            postpone_err,
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            value,
            timeout_override: None,
            _phantom_err: PhantomData,
        }
    }

    pub async fn prepare_stream<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Stream<T>, Error> {
        let since = None; // TODO: fix
        self.check_version(since)?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        self.transport_cmd_tx
            .send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
            })
            .await
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Stream {
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            rx,
            _phantom: PhantomData,
        })
    }

    pub fn prepare_stream_blocking<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Stream<T>, Error> {
        let since = None; // TODO: fix
        self.check_version(since)?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        self.transport_cmd_tx
            .blocking_send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Stream {
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            rx,
            _phantom: PhantomData,
        })
    }

    pub async fn prepare_sink<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let since = None; // TODO: fix
        self.check_version(since)?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        self.transport_cmd_tx
            .send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
            })
            .await
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Sink {
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            _sideband_rx: rx,
            _phantom: PhantomData,
            scratch: [0u8; 1024],
        })
    }

    pub fn prepare_sink_blocking<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let since = None; // TODO: fix
        self.check_version(since)?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (tx, rx) = mpsc::unbounded_channel();
        self.transport_cmd_tx
            .blocking_send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(Sink {
            transport_cmd_tx: TransportCommander::new(
                self.transport_cmd_tx.clone(),
                self.default_timeout,
            ),
            path_kind,
            _sideband_rx: rx,
            _phantom: PhantomData,
            scratch: [0u8; 1024],
        })
    }

    pub fn introspect(&self) -> Introspect {
        Introspect::new(
            TransportCommander::new(self.transport_cmd_tx.clone(), self.default_timeout),
            self.connected_device.user_api_hash.clone(),
        )
    }

    pub fn base_path(&self) -> Option<&Vec<UNib32>> {
        self.base_path.as_ref()
    }

    pub fn set_base_path(&mut self, base_path: Vec<UNib32>) {
        self.base_path = Some(base_path);
    }

    pub fn info(&self) -> &DeviceApiInfo {
        &self.connected_device
    }

    pub(crate) fn set_client_introspect(&mut self, client_bundle: IntrospectBundle) {
        self.client_introspect = Some(client_bundle);
    }

    pub fn client_introspect(&self) -> Option<&IntrospectBundle> {
        self.client_introspect.as_ref()
    }

    pub(crate) fn set_device_introspect(&mut self, device_bundle: IntrospectBundle) {
        self.device_introspect = Some(device_bundle);
    }

    pub fn device_introspect(&self) -> Option<&IntrospectBundle> {
        self.device_introspect.as_ref()
    }

    // pub fn set_client_introspect_bytes(&mut self, ww_bytes: &[u8], api_signature: &[u8]) {
    //     self.client_api = Some((
    //         ApiBundleOwned::from_ww_bytes_owned(ww_bytes),
    //         api_signature.to_vec(),
    //     ));
    // }

    pub fn device_api_info(&self) -> &DeviceApiInfo {
        &self.connected_device
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

    fn check_version(&self, since: Option<(u32, u32, u32)>) -> Result<(), Error> {
        let Some(since) = since else { return Ok(()) };
        // assuming the protocol name matches, a device will refuse to connect if it is not, host as well checks it and won't try to connect
        let dev_user_api = &self.connected_device.user_api_version.version;
        // #[since = ""] only makes sense within compatible protocols, if a user annotated a resource with a different major version, it must be a mistake
        // this is checked in ww_trait macro
        let is_compatible = if dev_user_api.major.0 == 0 {
            dev_user_api.patch.0 >= since.2
        } else {
            dev_user_api.minor.0 >= since.1
        };
        if is_compatible {
            Ok(())
        } else {
            Err(Error::OlderProtocol(
                Box::new(self.connected_device.user_api_version.clone()),
                Box::new(FullVersionOwned::new(
                    self.connected_device.user_api_version.crate_id.clone(),
                    VersionOwned::new(since.0, since.1, since.2),
                )),
            ))
        }
    }

    pub fn disconnect(&self) -> PreparedDisconnect {
        PreparedDisconnect::new(self.transport_cmd_tx.clone())
    }
}

impl TransportCommander {
    fn new(cmd_tx: mpsc::Sender<Command>, default_timeout: Duration) -> Self {
        Self {
            cmd_tx,
            default_timeout,
        }
    }

    pub(crate) async fn send_message_expect_response(
        &self,
        bytes: Vec<u8>,
        done_tx: ResponseSender,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        self.cmd_tx
            .send(Command::SendMessage {
                bytes,
                done_tx: Some((done_tx, timeout.unwrap_or(self.default_timeout))),
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)
    }

    pub(crate) fn send_message_expect_response_blocking(
        &self,
        bytes: Vec<u8>,
        done_tx: ResponseSender,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        self.cmd_tx
            .blocking_send(Command::SendMessage {
                bytes,
                done_tx: Some((done_tx, timeout.unwrap_or(self.default_timeout))),
            })
            .map_err(|_| Error::EventLoopNotRunning)
    }

    pub(crate) async fn send_call_request(
        &self,
        path_kind: PathKindOwned,
        args: Vec<u8>,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Call {
                args: TailBytesOwned(args),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response(req, done_tx, timeout)
            .await?;
        Ok(done_rx)
    }

    pub(crate) fn send_call_request_blocking(
        &self,
        path_kind: PathKindOwned,
        args: Vec<u8>,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Call {
                args: TailBytesOwned(args),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response_blocking(req, done_tx, timeout)?;
        Ok(done_rx)
    }

    pub(crate) async fn send_call_request_forget(
        &self,
        path_kind: PathKindOwned,
        args: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Call {
                args: TailBytesOwned(args),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) fn send_call_request_forget_blocking(
        &self,
        path_kind: PathKindOwned,
        args: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Call {
                args: TailBytesOwned(args),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        self.cmd_tx
            .blocking_send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) async fn send_read_request(
        &self,
        path_kind: PathKindOwned,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Read,
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response(req, done_tx, timeout)
            .await?;
        Ok(done_rx)
    }

    pub(crate) fn send_read_request_blocking(
        &self,
        path_kind: PathKindOwned,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Read,
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response_blocking(req, done_tx, timeout)?;
        Ok(done_rx)
    }

    pub(crate) async fn send_write_request(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Write {
                data: TailBytesOwned(value),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response(req, done_tx, timeout)
            .await?;
        Ok(done_rx)
    }

    pub(crate) fn send_write_request_blocking(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Write {
                data: TailBytesOwned(value),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response_blocking(req, done_tx, timeout)?;
        Ok(done_rx)
    }

    pub(crate) async fn send_write_request_forget(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Write {
                data: TailBytesOwned(value),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) fn send_write_request_forget_blocking(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Write {
                data: TailBytesOwned(value),
            },
        };
        let req = req.to_ww_bytes_owned()?;
        self.cmd_tx
            .blocking_send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub(crate) async fn send_stream_sideband(
        &self,
        path_kind: PathKindOwned,
        sideband: StreamSideband,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::StreamSideband { sideband },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response(req, done_tx, timeout)
            .await?;
        Ok(done_rx)
    }

    pub(crate) fn send_stream_sideband_blocking(
        &self,
        path_kind: PathKindOwned,
        sideband: StreamSideband,
        timeout: Option<Duration>,
    ) -> Result<ResponseReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::StreamSideband { sideband },
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        self.send_message_expect_response_blocking(req, done_tx, timeout)?;
        Ok(done_rx)
    }

    // pub(crate) async fn send_stream_sideband_forget(
    //     &self,
    //     path_kind: PathKindOwned,
    //     sideband_cmd: StreamSidebandCommand,
    // ) -> Result<(), Error> {
    //     let req = ww_client_server::RequestOwned {
    //         seq: 0,
    //         path_kind,
    //         kind: RequestKindOwned::StreamSideband { sideband_cmd },
    //     };
    //     let req = req.to_ww_bytes_owned()?;
    //     self.cmd_tx
    //         .send(Command::SendMessage {
    //             bytes: req,
    //             done_tx: None,
    //         })
    //         .await
    //         .map_err(|_| Error::EventLoopNotRunning)?;
    //     Ok(())
    // }

    // pub(crate) fn send_stream_sideband_forget_blocking(
    //     &self,
    //     path_kind: PathKindOwned,
    //     sideband_cmd: StreamSidebandCommand,
    // ) -> Result<(), Error> {
    //     let req = ww_client_server::RequestOwned {
    //         seq: 0,
    //         path_kind,
    //         kind: RequestKindOwned::StreamSideband { sideband_cmd },
    //     };
    //     let req = req.to_ww_bytes_owned()?;
    //     self.cmd_tx
    //         .blocking_send(Command::SendMessage {
    //             bytes: req,
    //             done_tx: None,
    //         })
    //         .map_err(|_| Error::EventLoopNotRunning)?;
    //     Ok(())
    // }

    pub(crate) async fn send_introspect(
        &self,
        _timeout: Option<Duration>,
    ) -> Result<StreamUpdateReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            kind: RequestKindOwned::Introspect,
        };
        let req = req.to_ww_bytes_owned()?;
        let (stream_event_tx, stream_event_rx) = mpsc::unbounded_channel();
        self.cmd_tx
            .send(Command::OnStreamEvent {
                path_kind: Box::new(PathKindOwned::Absolute { path: vec![] }),
                stream_event_tx,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        self.cmd_tx
            .send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(stream_event_rx)
    }

    pub(crate) fn send_introspect_blocking(
        &self,
        _timeout: Option<Duration>,
    ) -> Result<StreamUpdateReceiver, Error> {
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            kind: RequestKindOwned::Introspect,
        };
        let req = req.to_ww_bytes_owned()?;
        let (stream_event_tx, stream_event_rx) = mpsc::unbounded_channel();
        self.cmd_tx
            .blocking_send(Command::OnStreamEvent {
                path_kind: Box::new(PathKindOwned::Absolute { path: vec![] }),
                stream_event_tx,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        self.cmd_tx
            .blocking_send(Command::SendMessage {
                bytes: req,
                done_tx: None,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(stream_event_rx)
    }
}

// this will panic due to trying to block a runtime thread
// impl Drop for DynClient {
//     fn drop(&mut self) {
//         _ = self.disconnect_blocking();
//     }
// }

impl Drop for Commander {
    fn drop(&mut self) {
        _ = self
            .disconnect()
            .reason(wire_weaver::DisconnectReason::CommanderDropped)
            .forget();
        std::thread::sleep(Duration::from_millis(50));
    }
}
