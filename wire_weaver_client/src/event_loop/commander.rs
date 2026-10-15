use crate::Error;
use crate::Introspect;
use crate::PreparedCall;
use crate::PreparedDisconnect;
use crate::Stream;
use crate::client::positioned::Shared;
use crate::compat::{ApiCompat, ApiMatch, Verdict};
use crate::config::IntrospectBundle;
use crate::device_info::DeviceApiInfo;
use crate::event_loop::command::{Command, StreamOpenHook};
use crate::event_loop::event_queue::event_queue;
use crate::event_loop::rx_dispatcher::ResponseReceiver;
use crate::event_loop::rx_dispatcher::ResponseSender;
use crate::event_loop::rx_dispatcher::StreamUpdateReceiver;
use crate::{
    AckMode, DEFAULT_REQUEST_TIMEOUT, PositionedStream, PreparedRead, PreparedStreamOpen,
    PreparedWrite, Sink,
};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UNib32, UVlq32Backfill};
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKind, PathKindOwned, RequestKindOwned, StreamSideband};
use ww_stream::StreamCursor;
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
    /// * None: trait addressing is sent as is, the device finds the trait in its API tree (API-20), wherever it is.
    /// * Some (empty path) for a trait implemented as the API root, trait addressing is substituted with an absolute path.
    /// * Some (non-empty path) for trait implemented at some particular path, trait addressing will be substituted with an actual path.
    ///
    /// "Some" cases are only for trait clients (`client = "trait_client"`). API root client (`client = "std_client"`) uses absolute addressing.
    base_path: Option<Vec<UNib32>>,

    /// Mapping from FullVersion (crate name as string + semver) and trait name to trait GID of a much smaller size.
    /// Optimally, should be requested from a device, so that newly assigned GIDs not yet known to a device are not used.
    /// But can also be forced to a known GID if performance is critical.
    gid_map: HashMap<(FullVersionOwned, String), CompactVersion>,
    default_timeout: Duration,
    pub(crate) connected_device: DeviceApiInfo,
    /// Client API and types, client signature. None if DynClient.
    client_introspect: Option<IntrospectBundle>,
    /// Device API and types, client signature. None if device has introspect disabled and there is no cache for it.
    device_introspect: Option<IntrospectBundle>,
    /// Whether client resources can be used with the connected device, see [Commander::resolve_api_match].
    api_match: ApiMatch,
    /// Connected with [ClientConfig::traits_only](crate::ClientConfig::traits_only): device API unknown, absolute
    /// paths are refused.
    pub(crate) traits_only: bool,
    /// Bytes of items queued per stream on the host
    stream_queue_bytes: usize,
    streams: StreamRegistry,
    consts: ConstCache,
}

#[derive(Clone)]
pub(crate) struct TransportCommander {
    cmd_tx: mpsc::Sender<Command>,
    default_timeout: Duration,
    /// Open counted and reliable streams, see [Commander::resume_streams]
    pub(crate) streams: StreamRegistry,
    /// Values of `const` items read on this connection, see [Commander::prepare_const_read]
    pub(crate) consts: ConstCache,
}

/// Values of `const` items read on a connection (API-36), by request path (serialized [PathKindOwned]): response
/// bytes, decoded again on each read. Shared by a [Commander] and its clones, a new connection starts empty.
pub(crate) type ConstCache = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

/// Counted and reliable streams opened through a [Commander] (and its clones), to resume them after a reconnect.
pub(crate) type StreamRegistry = Arc<Mutex<Vec<Weak<Shared>>>>;

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
            api_match: ApiMatch::NoClientApi,
            traits_only: false,
            stream_queue_bytes: crate::DEFAULT_STREAM_QUEUE_BYTES,
            streams: StreamRegistry::default(),
            consts: ConstCache::default(),
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

    /// `E` is [Error] for methods without `Result`, [CallError](crate::CallError) for methods returning one;
    /// `resource` names the method in [CallError::Unimplemented](crate::CallError::Unimplemented).
    pub fn prepare_call<T: DeserializeShrinkWrapOwned, E>(
        &self,
        path: PathKind<'_>,
        args: Result<Vec<u8>, Error>,
        resource: &'static str,
    ) -> PreparedCall<T, E> {
        // postpone error return to have a better syntax (one ? instead of two)
        let (postpone_err, path_kind, args) = match (self.resolve_path(path), args) {
            (Ok(path_kind), Ok(args)) => (Ok(()), path_kind, args),
            (Err(e), _) | (_, Err(e)) => (Err(e), PathKindOwned::Absolute { path: vec![] }, vec![]),
        };
        PreparedCall {
            postpone_err,
            transport_cmd_tx: self.transport(),
            path_kind,
            args,
            timeout_override: None,
            resource,
            error_list: None,
            _phantom: PhantomData,
        }
    }

    pub fn prepare_read<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> PreparedRead<T> {
        // postpone error return to have a better syntax
        let path_kind = self.to_ww_client_server_path(path);
        let version_check = match &path_kind {
            Ok(path_kind) => self.check_resource(path_kind),
            Err(_) => Ok(()),
        };
        PreparedRead {
            transport_cmd_tx: self.transport(),
            version_check,
            path_kind,
            timeout_override: None,
            cache: false,
            _phantom: PhantomData,
        }
    }

    /// Read of a `const` item (API-36): the device is asked once per connection, later reads (through this
    /// Commander or its clones) return the value received then, without a request. Errors are not cached.
    pub fn prepare_const_read<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> PreparedRead<T> {
        PreparedRead {
            cache: true,
            ..self.prepare_read(path)
        }
    }

    /// Forget the values of `const` items read so far, the next read of each asks the device again.
    pub fn clear_const_cache(&self) {
        self.consts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// `E` is [Error] for properties without a user error type, [CallError](crate::CallError) for those with one;
    /// `resource` names the property in [CallError::Unimplemented](crate::CallError::Unimplemented).
    pub fn prepare_write<E>(
        &self,
        path: PathKind<'_>,
        value: Result<Vec<u8>, Error>,
        resource: &'static str,
    ) -> PreparedWrite<E> {
        let (postpone_err, path_kind, value) = match (self.resolve_path(path), value) {
            (Ok(path_kind), Ok(value)) => (Ok(()), path_kind, value),
            (Err(e), _) | (_, Err(e)) => (Err(e), PathKindOwned::Absolute { path: vec![] }, vec![]),
        };
        PreparedWrite {
            postpone_err,
            transport_cmd_tx: self.transport(),
            path_kind,
            value,
            timeout_override: None,
            resource,
            error_list: None,
            _phantom_err: PhantomData,
        }
    }

    fn transport(&self) -> TransportCommander {
        TransportCommander {
            cmd_tx: self.transport_cmd_tx.clone(),
            default_timeout: self.default_timeout,
            streams: self.streams.clone(),
            consts: self.consts.clone(),
        }
    }

    /// Bytes of items queued per stream, see [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes).
    pub fn set_stream_queue_bytes(&mut self, bytes: usize) {
        self.stream_queue_bytes = bytes;
    }

    /// Open a stream by calling its stream function (generated `name(args)`): `delivery` is None for a plain
    /// stream (handle `S` = [Stream](crate::Stream)), the kind of a counted or reliable one (`S` =
    /// [PositionedStream](crate::PositionedStream)); `args` are the serialized arguments like
    /// [Self::prepare_call]'s; `R` is the stream function's result, see [StreamOpenResult](crate::StreamOpenResult).
    pub fn prepare_stream_open<R, S>(
        &self,
        path: PathKind<'_>,
        delivery: Option<ww_self::Delivery>,
        args: Result<Vec<u8>, Error>,
    ) -> PreparedStreamOpen<R, S> {
        let (postpone_err, path_kind, args) = match (self.resolve_path(path), args) {
            (Ok(path_kind), Ok(args)) => (Ok(()), path_kind, args),
            (Err(e), _) | (_, Err(e)) => (Err(e), PathKindOwned::Absolute { path: vec![] }, vec![]),
        };
        PreparedStreamOpen {
            postpone_err,
            transport_cmd_tx: self.transport(),
            path_kind,
            delivery,
            args,
            resume: None,
            ack_mode: AckMode::default(),
            window: crate::DEFAULT_WINDOW,
            queue_bytes: self.stream_queue_bytes,
            timeout_override: None,
            raw_fallible: false,
            error_check: None,
            _phantom: PhantomData,
        }
    }

    /// Resume a counted or reliable stream from `cursor` (generated `name_resume(cursor)`), with the arguments
    /// the device keeps for the session: after a reconnect or a restart of the host program. A cursor of another
    /// session than the device's opens the device's (`StreamEvent::Resumed { ended, .. }`).
    pub fn prepare_stream_resume<T>(
        &self,
        path: PathKind<'_>,
        delivery: ww_self::Delivery,
        cursor: StreamCursor,
    ) -> PreparedStreamOpen<(), PositionedStream<T>> {
        let mut open = self.prepare_stream_open(path, Some(delivery), Ok(vec![]));
        open.resume = Some(cursor);
        open
    }

    /// Resume every counted and reliable stream opened through this commander (and its clones) that is still
    /// open, each from its cursor, after a reconnect that followed `disconnect().keep_streams()`. Their handles
    /// go on as before and see [StreamEvent::Resumed](crate::StreamEvent::Resumed), and gaps if the device didn't
    /// keep everything. A stream the device can't resume ends (its handle returns `StreamError::Closed` once
    /// drained). Returns how many were resumed.
    // TODO(EX-10): the reconnect itself calls this once it exists
    pub async fn resume_streams(&self) -> Result<usize, Error> {
        let mut resumed = 0;
        for (shared, cmd, done_rx) in self.resume_commands()? {
            self.transport().send_command(cmd).await?;
            let ok = matches!(done_rx.await, Ok(Ok(_)));
            resumed += self.resumed(&shared, ok);
        }
        Ok(resumed)
    }

    /// Blocking variant of [Self::resume_streams].
    pub fn resume_streams_blocking(&self) -> Result<usize, Error> {
        let mut resumed = 0;
        for (shared, cmd, done_rx) in self.resume_commands()? {
            self.transport().send_command_blocking(cmd)?;
            let ok = matches!(done_rx.blocking_recv(), Ok(Ok(_)));
            resumed += self.resumed(&shared, ok);
        }
        Ok(resumed)
    }

    fn resume_commands(&self) -> Result<Vec<(Arc<Shared>, Command, ResponseReceiver)>, Error> {
        let streams: Vec<Arc<Shared>> = {
            let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
            streams.retain(|s| s.strong_count() > 0);
            streams.iter().filter_map(Weak::upgrade).collect()
        };
        let mut commands = vec![];
        for shared in streams {
            let inner = shared.lock();
            let Some(cursor) = inner.cursor() else {
                continue;
            };
            if inner.is_finished() {
                continue;
            }
            let path = inner.path.clone();
            drop(inner);
            let bytes = ww_client_server::RequestOwned {
                seq: UVlq32Backfill(0),
                path_kind: PathKindOwned::Absolute { path },
                kind: RequestKindOwned::StreamOpen {
                    resume: Some(cursor),
                    args: TailBytesOwned(vec![]),
                },
            }
            .to_ww_bytes_owned()?;
            let (done_tx, done_rx) = oneshot::channel();
            let cmd = Command::OpenStream {
                bytes,
                done_tx,
                timeout: self.default_timeout,
                hook: Box::new(StreamOpenHook::new::<()>(shared.clone(), Some(cursor))),
            };
            commands.push((shared, cmd, done_rx));
        }
        Ok(commands)
    }

    fn resumed(&self, shared: &Shared, ok: bool) -> usize {
        if ok {
            return 1;
        }
        warn!("a stream could not be resumed, it ends");
        shared.lock().ended();
        shared.wake();
        0
    }

    pub async fn prepare_stream<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Stream<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream(&path_kind, self.stream_queue_bytes)
            .await?;
        Ok(Stream {
            transport_cmd_tx: self.transport(),
            path_kind,
            rx,
            _phantom: PhantomData,
        })
    }

    pub fn prepare_stream_blocking<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Stream<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream_blocking(&path_kind, self.stream_queue_bytes)?;
        Ok(Stream {
            transport_cmd_tx: self.transport(),
            path_kind,
            rx,
            _phantom: PhantomData,
        })
    }

    pub async fn prepare_sink<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream(&path_kind, self.stream_queue_bytes)
            .await?;
        Ok(self.sink(path_kind, rx, false))
    }

    pub fn prepare_sink_blocking<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream_blocking(&path_kind, self.stream_queue_bytes)?;
        Ok(self.sink(path_kind, rx, false))
    }

    /// Sink with credit-based flow control (`#[credit] sink!(..)`), see `docs/api/streams.md`.
    pub async fn prepare_credit_sink<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream(&path_kind, self.stream_queue_bytes)
            .await?;
        Ok(self.sink(path_kind, rx, true))
    }

    /// Sink with credit-based flow control (`#[credit] sink!(..)`), see `docs/api/streams.md`.
    pub fn prepare_credit_sink_blocking<T: DeserializeShrinkWrapOwned>(
        &self,
        path: PathKind<'_>,
    ) -> Result<Sink<T>, Error> {
        let path_kind = self.resolve_path(path)?;
        let rx = self
            .transport()
            .register_stream_blocking(&path_kind, self.stream_queue_bytes)?;
        Ok(self.sink(path_kind, rx, true))
    }

    fn sink<T>(&self, path_kind: PathKindOwned, rx: StreamUpdateReceiver, credit: bool) -> Sink<T> {
        Sink::new(self.transport(), path_kind, rx, credit)
    }

    pub fn introspect(&self) -> Introspect {
        Introspect::new(
            self.transport(),
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

    /// Decide how client resources relate to the connected device's, once both introspection bundles are known:
    /// * no client API (DynClient) - nothing to check,
    /// * same API hash - all resources are the same by definition,
    /// * different hash - every method, property, stream and trait is compared with the device's one,
    /// * different hash and device API unknown - only `#[since]` is checked against the device's API version.
    pub(crate) fn resolve_api_match(&mut self) {
        let Some(client) = &self.client_introspect else {
            self.api_match = ApiMatch::NoClientApi;
            return;
        };
        self.api_match = if client.api_hash.no_docs == self.connected_device.user_api_hash.no_docs {
            ApiMatch::Identical
        } else if let Some(device) = &self.device_introspect {
            let compat = ApiCompat::new(&client.api_bundle, &device.api_bundle);
            let mismatches = compat.mismatches();
            if mismatches.is_empty() {
                debug!(
                    "device API differs from the client's, but all client resources are compatible"
                );
            } else {
                let list = mismatches
                    .iter()
                    .map(|(name, verdict)| match verdict {
                        Verdict::Incompatible(reason) => format!("\n  {name}: {reason}"),
                        _ => format!("\n  {name}: not implemented"),
                    })
                    .collect::<String>();
                warn!(
                    "device API {:?} differs from the client's, these resources can't be used:{list}",
                    self.connected_device.user_api_version
                );
            }
            ApiMatch::Checked(Arc::new(compat))
        } else {
            debug!(
                "device API differs from the client's and is unknown, only #[since] will be checked"
            );
            ApiMatch::DeviceApiUnknown(Arc::new(ApiCompat::client_only(&client.api_bundle)))
        };
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
                trait_name,
                path_from_trait,
            } => {
                if let Some(base) = &self.base_path {
                    let mut path = base.clone();
                    for n in path_from_trait.iter() {
                        path.push(n?);
                    }
                    PathKindOwned::Absolute { path }
                } else if let Some(compact) = self
                    .gid_map
                    .get(&(gid.make_owned(), trait_name.to_string()))
                {
                    // NOTE: lookup needs an owned key, FullVersionOwned can't implement Borrow<FullVersion<'_>> (it borrows &str)
                    PathKindOwned::GlobalCompact {
                        gid: *compact,
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                } else {
                    PathKindOwned::GlobalFull {
                        gid: gid.make_owned(),
                        trait_name: trait_name.to_string(),
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
        };
        if self.traits_only
            && let PathKindOwned::Absolute { path } = &path_kind
        {
            return Err(Error::AbsolutePathInTraitsOnly(format!("{path:?}")));
        }
        Ok(path_kind)
    }

    fn resolve_path(&self, path: PathKind<'_>) -> Result<PathKindOwned, Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        self.check_resource(&path_kind)?;
        Ok(path_kind)
    }

    /// Check that the connected device has a compatible resource at the path, see [Commander::resolve_api_match].
    /// Only absolute paths are checked, trait paths without a base path can't be mapped onto the client's API.
    fn check_resource(&self, path_kind: &PathKindOwned) -> Result<(), Error> {
        let PathKindOwned::Absolute { path } = path_kind else {
            return Ok(());
        };
        let compat = match &self.api_match {
            ApiMatch::NoClientApi | ApiMatch::Identical => return Ok(()),
            ApiMatch::Checked(compat) | ApiMatch::DeviceApiUnknown(compat) => compat,
        };
        let Some(resource) = compat.resolve(path) else {
            return Ok(());
        };
        let device = || Box::new(self.connected_device.user_api_version.clone());
        match resource.verdict {
            Verdict::Compatible => self.check_version(resource.since),
            Verdict::Missing => {
                // prefer a more specific error if the resource was added later than device's version
                self.check_version(resource.since)?;
                Err(Error::NotImplementedByDevice {
                    resource: resource.name,
                    device: device(),
                })
            }
            // an optional item the device leaves out, known from its introspection data
            Verdict::Omitted => Err(Error::NotImplementedByDevice {
                resource: resource.name,
                device: device(),
            }),
            Verdict::Incompatible(reason) => Err(Error::IncompatibleResource {
                resource: resource.name,
                device: device(),
                reason: reason.clone(),
            }),
        }
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
        // a reconnect through the same Commander (`keep_streams`) may find other firmware
        self.clear_const_cache();
        PreparedDisconnect::new(self.transport_cmd_tx.clone())
    }
}

impl TransportCommander {
    /// Cached response bytes of a `const` item read, see [Commander::prepare_const_read].
    pub(crate) fn cached_const(&self, path_kind: &PathKindOwned) -> Option<Vec<u8>> {
        let key = path_kind.to_ww_bytes_owned().ok()?;
        self.consts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned()
    }

    pub(crate) fn cache_const(&self, path_kind: &PathKindOwned, response: &[u8]) {
        if let Ok(key) = path_kind.to_ww_bytes_owned() {
            self.consts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key, response.to_vec());
        }
    }

    pub(crate) fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    pub(crate) async fn send_command(&self, cmd: Command) -> Result<(), Error> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| Error::EventLoopNotRunning)
    }

    pub(crate) fn send_command_blocking(&self, cmd: Command) -> Result<(), Error> {
        self.cmd_tx
            .blocking_send(cmd)
            .map_err(|_| Error::EventLoopNotRunning)
    }

    /// Subscribe to events on `path_kind`, returns once the event loop routes them to the returned receiver, so that
    /// nothing the device sends after this is lost.
    pub(crate) async fn register_stream(
        &self,
        path_kind: &PathKindOwned,
        queue_bytes: usize,
    ) -> Result<StreamUpdateReceiver, Error> {
        let (tx, rx) = event_queue(queue_bytes);
        let (registered_tx, registered_rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
                registered_tx,
            })
            .await
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        registered_rx
            .await
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(rx)
    }

    /// Blocking variant of [Self::register_stream].
    pub(crate) fn register_stream_blocking(
        &self,
        path_kind: &PathKindOwned,
        queue_bytes: usize,
    ) -> Result<StreamUpdateReceiver, Error> {
        let (tx, rx) = event_queue(queue_bytes);
        let (registered_tx, registered_rx) = oneshot::channel();
        self.cmd_tx
            .blocking_send(Command::OnStreamEvent {
                path_kind: Box::new(path_kind.clone()),
                stream_event_tx: tx,
                registered_tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        registered_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        Ok(rx)
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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

    /// A place in the command queue, None if it is full, for [Self::write_forget_command] and
    /// [Self::sideband_command].
    pub(crate) fn try_reserve(&self) -> Result<Option<mpsc::Permit<'_, Command>>, Error> {
        match self.cmd_tx.try_reserve() {
            Ok(permit) => Ok(Some(permit)),
            Err(mpsc::error::TrySendError::Full(())) => Ok(None),
            Err(mpsc::error::TrySendError::Closed(())) => Err(Error::EventLoopNotRunning),
        }
    }

    /// Write to a sink, without a reply.
    pub(crate) fn write_forget_command(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
    ) -> Result<Command, Error> {
        let req = ww_client_server::RequestOwned {
            seq: UVlq32Backfill(0),
            path_kind,
            kind: RequestKindOwned::Write {
                data: TailBytesOwned(value),
            },
        };
        Ok(Command::SendMessage {
            bytes: req.to_ww_bytes_owned()?,
            done_tx: None,
        })
    }

    /// Stream sideband request, nobody waits for the reply.
    pub(crate) fn sideband_command(
        &self,
        path_kind: PathKindOwned,
        sideband: StreamSideband,
    ) -> Result<Command, Error> {
        let req = ww_client_server::RequestOwned {
            seq: UVlq32Backfill(0),
            path_kind,
            kind: RequestKindOwned::StreamSideband { sideband },
        };
        let (done_tx, _done_rx) = oneshot::channel();
        Ok(Command::SendMessage {
            bytes: req.to_ww_bytes_owned()?,
            done_tx: Some((done_tx, self.default_timeout)),
        })
    }

    pub(crate) fn send_write_request_forget_blocking(
        &self,
        path_kind: PathKindOwned,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        let req = ww_client_server::RequestOwned {
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
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
            seq: UVlq32Backfill(0),
            path_kind: PathKindOwned::Absolute { path: vec![] },
            kind: RequestKindOwned::Introspect,
        };
        let req = req.to_ww_bytes_owned()?;
        // introspection data must not be dropped
        let (stream_event_tx, stream_event_rx) = event_queue(usize::MAX);
        self.cmd_tx
            .send(Command::OnStreamEvent {
                path_kind: Box::new(PathKindOwned::Absolute { path: vec![] }),
                stream_event_tx,
                // the request is sent after this, its events can't overtake the registration
                registered_tx: oneshot::channel().0,
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
            seq: UVlq32Backfill(0),
            path_kind: PathKindOwned::Absolute { path: vec![] },
            kind: RequestKindOwned::Introspect,
        };
        let req = req.to_ww_bytes_owned()?;
        // introspection data must not be dropped
        let (stream_event_tx, stream_event_rx) = event_queue(usize::MAX);
        self.cmd_tx
            .blocking_send(Command::OnStreamEvent {
                path_kind: Box::new(PathKindOwned::Absolute { path: vec![] }),
                stream_event_tx,
                // the request is sent after this, its events can't overtake the registration
                registered_tx: oneshot::channel().0,
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
