//! Resources of an API known only at runtime (from introspection data or a saved bundle), without generated code:
//! walked by names and indices, with values as [ValueOwned], (de)serialized according to their types in the
//! [ApiBundleOwned]. Bytes on the wire are the same as the ones produced by generated clients.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use wire_weaver_client::{DynClient, DynResource};
//! # use wire_weaver_client::ww_self::ValueOwned;
//! # use wire_weaver_client::ww_numeric::NumericValue;
//! # fn f() -> anyhow::Result<()> {
//! let client = DynClient::new().connect_blocking()?;
//! let bundle = client.cmd().introspect().get_blocking()?.expect("introspection is enabled");
//! let root = DynResource::root(Arc::new(bundle));
//! let gain = root.child("periph")?.index(0)?.child("channel")?.index(1)?.child("gain")?;
//! gain.write_blocking(client.cmd(), &ValueOwned::Numeric(NumericValue::F32(0.5)), None)?;
//! # Ok(())
//! # }
//! ```

use crate::client::dyn_positioned::{DynPositionedStream, DynStreamOpen};
use crate::client::prepared_stream_open::RawOpenResult;
use crate::{
    AckMode, Commander, CreditConfig, Error, PositionedStream, PreparedStreamOpen, Sink, Stream,
    StreamCursor, StreamError,
};
use std::sync::Arc;
use std::time::Duration;
use wire_weaver::ValidIndicesOwned;
use wire_weaver::prelude::UNib32;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::PathKind;
use ww_numeric::NumericValue;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Delivery,
    FlowControl, Multiplicity, PropertyAccess, TypeOwned, ValueOwned,
};

/// API root, trait, method, property, stream or an array of them, found by walking an [ApiBundleOwned].
#[derive(Clone, Debug)]
pub struct DynResource {
    bundle: Arc<ApiBundleOwned>,
    index_chain: Vec<UNib32>,
    /// Human-readable path from the API root, e.g. `periph[1].channel[0].gain`.
    path: String,
    /// None for the API root
    item: Option<ApiItemOwned>,
    node: Node,
}

#[derive(Clone, Debug)]
enum Node {
    /// API root (None) or a trait
    Level { trait_idx: Option<u32> },
    /// Array of resources, not yet indexed
    Array,
    /// Method, property or stream
    Item,
}

/// What a [DynResource] is.
#[derive(Debug)]
pub enum DynResourceKind<'a> {
    /// API root or a trait, see [DynResource::children].
    Level(&'a ApiLevelOwned),
    /// Array of resources, see [DynResource::index].
    Array(&'a ApiItemOwned),
    Method {
        args: &'a [ArgumentOwned],
        return_ty: Option<&'a TypeOwned>,
    },
    Property {
        ty: &'a TypeOwned,
        access: PropertyAccess,
    },
    /// Stream from a device (`is_up = true`), or a sink on it, with flow control if it has one.
    Stream {
        ty: &'a TypeOwned,
        is_up: bool,
        flow: Option<FlowControl>,
    },
}

impl DynResource {
    /// Root of the API.
    pub fn root(bundle: Arc<ApiBundleOwned>) -> Self {
        DynResource {
            bundle,
            index_chain: vec![],
            path: String::new(),
            item: None,
            node: Node::Level { trait_idx: None },
        }
    }

    pub fn bundle(&self) -> &Arc<ApiBundleOwned> {
        &self.bundle
    }

    /// Human-readable path from the API root, e.g. `periph[1].channel[0].gain`, empty for the root.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Resource IDs and indices from the API root, as sent to a device.
    pub fn index_chain(&self) -> &[UNib32] {
        &self.index_chain
    }

    /// API item definition (with docs), None for the API root.
    pub fn item(&self) -> Option<&ApiItemOwned> {
        self.item.as_ref()
    }

    pub fn kind(&self) -> Result<DynResourceKind<'_>, Error> {
        let item = self.item.as_ref();
        Ok(match (&self.node, item) {
            (Node::Level { .. }, _) => DynResourceKind::Level(self.level()?),
            (Node::Array, Some(item)) => DynResourceKind::Array(item),
            (Node::Item, Some(item)) => match &item.kind {
                ApiItemKindOwned::Method { args, return_ty } => DynResourceKind::Method {
                    args,
                    return_ty: return_ty.as_ref(),
                },
                ApiItemKindOwned::Property { ty, access, .. } => DynResourceKind::Property {
                    ty,
                    access: *access,
                },
                ApiItemKindOwned::Stream { ty, is_up } => DynResourceKind::Stream {
                    ty,
                    is_up: *is_up,
                    flow: None,
                },
                ApiItemKindOwned::FlowSink { ty, flow } => DynResourceKind::Stream {
                    ty,
                    is_up: false,
                    flow: Some(*flow),
                },
                // TODO(CLT-23): positions, gaps and acks of counted and reliable streams
                ApiItemKindOwned::StreamFn { ty, .. } => DynResourceKind::Stream {
                    ty,
                    is_up: true,
                    flow: None,
                },
                ApiItemKindOwned::Trait { .. } => unreachable!("traits are levels"),
            },
            (_, None) => unreachable!("only the API root has no item"),
        })
    }

    /// Resources of the API root or a trait, empty for anything else.
    pub fn children(&self) -> &[ApiItemOwned] {
        match self.level() {
            Ok(level) => &level.items,
            Err(_) => &[],
        }
    }

    /// Resource named `name` in the API root or a trait.
    pub fn child(&self, name: &str) -> Result<DynResource, Error> {
        let level = self.level()?;
        let Some(item) = level.items.iter().find(|i| i.ident == name) else {
            let names: Vec<_> = level.items.iter().map(|i| i.ident.as_str()).collect();
            return Err(Error::Other(format!(
                "'{}' has no resource '{name}', available: {}",
                self.display_path(),
                names.join(", ")
            )));
        };
        let mut index_chain = self.index_chain.clone();
        index_chain.push(item.id);
        let path = if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}.{name}", self.path)
        };
        let node = if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            Node::Array
        } else {
            self.resolve(item)?
        };
        Ok(DynResource {
            bundle: self.bundle.clone(),
            index_chain,
            path,
            item: Some(item.clone()),
            node,
        })
    }

    /// Element of an array of resources.
    pub fn index(&self, index: u32) -> Result<DynResource, Error> {
        let (Node::Array, Some(item)) = (&self.node, &self.item) else {
            return Err(Error::Other(format!(
                "'{}' is not an array",
                self.display_path()
            )));
        };
        let mut index_chain = self.index_chain.clone();
        index_chain.push(UNib32(index));
        Ok(DynResource {
            bundle: self.bundle.clone(),
            index_chain,
            path: format!("{}[{index}]", self.path),
            item: Some(item.clone()),
            node: self.resolve(item)?,
        })
    }

    fn resolve(&self, item: &ApiItemOwned) -> Result<Node, Error> {
        Ok(match &item.kind {
            ApiItemKindOwned::Trait { trait_idx } => {
                // check that it is present, not skipped
                self.bundle.get_trait(trait_idx.0).map_err(other)?;
                Node::Level {
                    trait_idx: Some(trait_idx.0),
                }
            }
            _ => Node::Item,
        })
    }

    fn level(&self) -> Result<&ApiLevelOwned, Error> {
        match &self.node {
            Node::Level { trait_idx: None } => Ok(&self.bundle.root),
            Node::Level {
                trait_idx: Some(idx),
            } => self.bundle.get_trait(*idx).map_err(other),
            _ => Err(Error::Other(format!(
                "'{}' is not a trait",
                self.display_path()
            ))),
        }
    }

    fn display_path(&self) -> &str {
        if self.path.is_empty() {
            "API root"
        } else {
            &self.path
        }
    }

    fn path_kind(&self) -> PathKind<'_> {
        PathKind::absolute(&self.index_chain)
    }

    fn method(&self) -> Result<(&[ArgumentOwned], Option<&TypeOwned>), Error> {
        match self.kind()? {
            DynResourceKind::Method { args, return_ty } => Ok((args, return_ty)),
            _ => Err(self.not_a("method")),
        }
    }

    fn property(&self) -> Result<(&TypeOwned, PropertyAccess), Error> {
        match self.kind()? {
            DynResourceKind::Property { ty, access } => Ok((ty, access)),
            _ => Err(self.not_a("property")),
        }
    }

    fn stream_ty(&self, want_up: bool) -> Result<(&TypeOwned, Option<FlowControl>), Error> {
        match self.kind()? {
            DynResourceKind::Stream { ty, is_up, flow } if is_up == want_up => Ok((ty, flow)),
            _ => Err(self.not_a(if want_up {
                "stream from a device"
            } else {
                "sink on a device"
            })),
        }
    }

    fn not_a(&self, what: &str) -> Error {
        Error::Other(format!("'{}' is not a {what}", self.display_path()))
    }

    fn call_args(&self, args: &[ValueOwned]) -> Result<Vec<u8>, Error> {
        let (defs, _) = self.method()?;
        if args.len() != defs.len() {
            return Err(Error::Other(format!(
                "'{}' takes {} arguments, got {}",
                self.path,
                defs.len(),
                args.len()
            )));
        }
        ValueOwned::ser_shrink_wrap_vec_dyn(args, defs.iter().map(|a| &a.ty), &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' arguments: {e:#}", self.path)))
    }

    fn decode_return(&self, bytes: &[u8]) -> Result<Option<ValueOwned>, Error> {
        let (_, return_ty) = self.method()?;
        let Some(return_ty) = return_ty else {
            return Ok(None);
        };
        ValueOwned::des_shrink_wrap_dyn(bytes, return_ty, &self.bundle)
            .map(Some)
            .map_err(|e| Error::Other(format!("'{}' return value: {e:#}", self.path)))
    }

    /// Call a method with arguments in the order they are defined in, returns None if it returns nothing.
    pub async fn call(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
    ) -> Result<Option<ValueOwned>, Error> {
        let bytes = self.call_args(args)?;
        let reply = cmd
            .prepare_call::<TailBytesOwned, Error>(self.path_kind(), Ok(bytes), "")
            .call()
            .await?;
        self.decode_return(&reply)
    }

    /// Call a method with arguments in the order they are defined in, returns None if it returns nothing.
    pub fn call_blocking(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
        timeout: Option<Duration>,
    ) -> Result<Option<ValueOwned>, Error> {
        let bytes = self.call_args(args)?;
        let mut call = cmd.prepare_call::<TailBytesOwned, Error>(self.path_kind(), Ok(bytes), "");
        if let Some(timeout) = timeout {
            call = call.with_timeout(timeout);
        }
        let reply = call.blocking_call()?;
        self.decode_return(&reply)
    }

    fn decode_property(&self, bytes: &[u8]) -> Result<ValueOwned, Error> {
        let (ty, _) = self.property()?;
        ValueOwned::des_shrink_wrap_dyn(bytes, ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' value: {e:#}", self.path)))
    }

    fn encode_property(&self, value: &ValueOwned) -> Result<Vec<u8>, Error> {
        let (ty, _) = self.property()?;
        value
            .ser_shrink_wrap_dyn(ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' value: {e:#}", self.path)))
    }

    /// Read a property.
    pub async fn read(&self, cmd: &Commander) -> Result<ValueOwned, Error> {
        self.property()?;
        let bytes = cmd
            .prepare_read::<TailBytesOwned>(self.path_kind())
            .read()
            .await?;
        self.decode_property(&bytes)
    }

    /// Read a property.
    pub fn read_blocking(
        &self,
        cmd: &Commander,
        timeout: Option<Duration>,
    ) -> Result<ValueOwned, Error> {
        self.property()?;
        let mut read = cmd.prepare_read::<TailBytesOwned>(self.path_kind());
        if let Some(timeout) = timeout {
            read = read.with_timeout(timeout);
        }
        let bytes = read.blocking_read()?;
        self.decode_property(&bytes)
    }

    /// Write a property.
    pub async fn write(&self, cmd: &Commander, value: &ValueOwned) -> Result<(), Error> {
        let bytes = self.encode_property(value)?;
        cmd.prepare_write::<Error>(self.path_kind(), Ok(bytes), "")
            .write()
            .await
    }

    /// Write a property.
    pub fn write_blocking(
        &self,
        cmd: &Commander,
        value: &ValueOwned,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let bytes = self.encode_property(value)?;
        let mut write = cmd.prepare_write::<Error>(self.path_kind(), Ok(bytes), "");
        if let Some(timeout) = timeout {
            write = write.with_timeout(timeout);
        }
        write.blocking_write()
    }

    /// Read which indices are valid for an array of resources.
    pub async fn valid_indices(&self, cmd: &Commander) -> Result<ValidIndicesOwned, Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        cmd.prepare_read::<ValidIndicesOwned>(self.path_kind())
            .read()
            .await
    }

    /// Read which indices are valid for an array of resources.
    pub fn valid_indices_blocking(&self, cmd: &Commander) -> Result<ValidIndicesOwned, Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        cmd.prepare_read::<ValidIndicesOwned>(self.path_kind())
            .blocking_read()
    }

    /// Open a plain stream from a device (a `stream!` without arguments): the device starts it.
    // TODO(CLT-23): stream functions with arguments, counted and reliable streams
    pub async fn stream(&self, cmd: &Commander) -> Result<DynStream, Error> {
        let ty = self.plain_stream_ty()?;
        let stream = cmd
            .prepare_stream_open::<(), Stream<TailBytesOwned>>(self.path_kind(), None, Ok(vec![]))
            .call()
            .await?;
        Ok(DynStream::new(self, stream, ty))
    }

    /// Blocking variant of [Self::stream].
    pub fn stream_blocking(&self, cmd: &Commander) -> Result<DynStream, Error> {
        let ty = self.plain_stream_ty()?;
        let stream = cmd
            .prepare_stream_open::<(), Stream<TailBytesOwned>>(self.path_kind(), None, Ok(vec![]))
            .blocking_call()?;
        Ok(DynStream::new(self, stream, ty))
    }

    /// Item type of a plain stream without arguments and result.
    fn plain_stream_ty(&self) -> Result<TypeOwned, Error> {
        let ty = self.stream_ty(true)?.0.clone();
        match &self.item {
            Some(item) if matches!(item.kind, ApiItemKindOwned::StreamFn { .. }) => {
                Err(Error::Other(format!(
                    "'{}' is a stream function or a counted or reliable stream, not supported by the dynamic \
                     client yet",
                    self.display_path()
                )))
            }
            _ => Ok(ty),
        }
    }

    /// Delivery kind of a counted or reliable stream, None for anything else.
    pub fn delivery(&self) -> Option<Delivery> {
        match self.item.as_ref().map(|i| &i.kind) {
            Some(ApiItemKindOwned::StreamFn { delivery, .. }) if *delivery != Delivery::Plain => {
                Some(*delivery)
            }
            _ => None,
        }
    }

    /// Arguments and result type of a stream function.
    pub fn stream_fn(&self) -> Option<(&[ArgumentOwned], Option<&TypeOwned>)> {
        match self.item.as_ref().map(|i| &i.kind) {
            Some(ApiItemKindOwned::StreamFn {
                args, return_ty, ..
            }) => Some((args, return_ty.as_ref())),
            _ => None,
        }
    }

    fn stream_fn_args(&self, args: &[ValueOwned]) -> Result<Vec<u8>, Error> {
        let defs = self.stream_fn().map(|(a, _)| a).unwrap_or_default();
        if args.len() != defs.len() {
            return Err(Error::Other(format!(
                "'{}' takes {} arguments, got {}",
                self.path,
                defs.len(),
                args.len()
            )));
        }
        ValueOwned::ser_shrink_wrap_vec_dyn(args, defs.iter().map(|a| &a.ty), &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' arguments: {e:#}", self.path)))
    }

    fn dyn_positioned(&self, stream: PositionedStream<TailBytesOwned>) -> DynPositionedStream {
        let ty = self
            .stream_ty(true)
            .map(|(ty, _)| ty.clone())
            .expect("checked when opening");
        DynPositionedStream {
            stream,
            is_bytes: ty.is_byte_slice(&self.bundle).unwrap_or(false),
            ty,
            bundle: self.bundle.clone(),
        }
    }

    /// The stream function's result as sent with the open answer: the stream is open unless it is an `Err`.
    fn stream_opened(
        &self,
        result: Vec<u8>,
        stream: PositionedStream<TailBytesOwned>,
    ) -> Result<DynStreamOpen, Error> {
        let result = match self.stream_fn().and_then(|(_, ty)| ty) {
            Some(ty) if !result.is_empty() => Some(
                ValueOwned::des_shrink_wrap_dyn(&result, ty, &self.bundle)
                    .map_err(|e| Error::Other(format!("'{}' result: {e:#}", self.path)))?,
            ),
            _ => None,
        };
        Ok(match result {
            Some(ValueOwned::Result(Err(e))) => DynStreamOpen::Rejected(*e),
            Some(ValueOwned::Result(Ok(v))) => DynStreamOpen::Open {
                result: Some(*v),
                stream: Box::new(self.dyn_positioned(stream)),
            },
            result => DynStreamOpen::Open {
                result,
                stream: Box::new(self.dyn_positioned(stream)),
            },
        })
    }

    fn prepare_positioned(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
        ack_mode: AckMode,
        resume: Option<StreamCursor>,
    ) -> Result<PreparedStreamOpen<RawOpenResult, PositionedStream<TailBytesOwned>>, Error> {
        let delivery = self
            .delivery()
            .ok_or_else(|| self.not_a("counted or reliable stream"))?;
        self.stream_ty(true)?;
        let fallible = matches!(
            self.stream_fn()
                .and_then(|(_, ty)| ty)
                .map(|ty| ty.get_in_line(&self.bundle)),
            Some(Ok(TypeOwned::Result { .. }))
        );
        let mut open = match resume {
            // the device keeps the session's arguments
            Some(cursor) => {
                let mut open =
                    cmd.prepare_stream_open(self.path_kind(), Some(delivery), Ok(vec![]));
                open.resume = Some(cursor);
                open
            }
            None => {
                cmd.prepare_stream_open(self.path_kind(), Some(delivery), self.stream_fn_args(args))
            }
        };
        open.raw_fallible = fallible;
        Ok(open.ack_mode(ack_mode))
    }

    /// Open a counted or reliable stream by calling its stream function with `args` (in the order they are defined
    /// in), or resume it from `resume` (`args` empty then), see [PositionedStream].
    pub async fn positioned_stream(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
        ack_mode: AckMode,
        resume: Option<StreamCursor>,
    ) -> Result<DynStreamOpen, Error> {
        let open = self.prepare_positioned(cmd, args, ack_mode, resume)?;
        let (result, stream) = open.call().await?;
        self.stream_opened(result.0, stream)
    }

    /// Blocking variant of [Self::positioned_stream].
    pub fn positioned_stream_blocking(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
        ack_mode: AckMode,
        resume: Option<StreamCursor>,
    ) -> Result<DynStreamOpen, Error> {
        let open = self.prepare_positioned(cmd, args, ack_mode, resume)?;
        let (result, stream) = open.blocking_call()?;
        self.stream_opened(result.0, stream)
    }

    /// Sink on a device.
    pub async fn sink(&self, cmd: &Commander) -> Result<DynSink, Error> {
        let (ty, flow) = self.stream_ty(false)?;
        let ty = ty.clone();
        let sink = match flow {
            None => cmd.prepare_sink(self.path_kind()).await?,
            Some(FlowControl::Credit) => cmd.prepare_credit_sink(self.path_kind()).await?,
        };
        Ok(DynSink::new(self, sink, ty))
    }

    /// Sink on a device.
    pub fn sink_blocking(&self, cmd: &Commander) -> Result<DynSink, Error> {
        let (ty, flow) = self.stream_ty(false)?;
        let ty = ty.clone();
        let sink = match flow {
            None => cmd.prepare_sink_blocking(self.path_kind())?,
            Some(FlowControl::Credit) => cmd.prepare_credit_sink_blocking(self.path_kind())?,
        };
        Ok(DynSink::new(self, sink, ty))
    }
}

/// Stream from a device with items as [ValueOwned].
pub struct DynStream {
    stream: Stream<TailBytesOwned>,
    ty: TypeOwned,
    is_bytes: bool,
    bundle: Arc<ApiBundleOwned>,
}

impl DynStream {
    fn new(resource: &DynResource, stream: Stream<TailBytesOwned>, ty: TypeOwned) -> Self {
        let is_bytes = ty.is_byte_slice(&resource.bundle).unwrap_or(false);
        DynStream {
            stream,
            ty,
            is_bytes,
            bundle: resource.bundle.clone(),
        }
    }

    /// Stream item type.
    pub fn ty(&self) -> &TypeOwned {
        &self.ty
    }

    pub async fn close(&self) -> Result<(), StreamError> {
        self.stream.close().await
    }

    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.stream.close_blocking()
    }

    /// Receive one item, see [Stream::recv].
    pub async fn recv(&mut self) -> Result<ValueOwned, StreamError> {
        let bytes = self.stream.recv().await?;
        self.decode(bytes)
    }

    /// Receive one item, see [Stream::recv_blocking].
    pub fn recv_blocking(&mut self) -> Result<ValueOwned, StreamError> {
        let bytes = self.stream.recv_blocking()?;
        self.decode(bytes)
    }

    /// Receive one item if available, see [Stream::try_recv].
    pub fn try_recv(&mut self) -> Result<Option<ValueOwned>, StreamError> {
        match self.stream.try_recv()? {
            Some(bytes) => self.decode(bytes).map(Some),
            None => Ok(None),
        }
    }

    fn decode(&self, bytes: TailBytesOwned) -> Result<ValueOwned, StreamError> {
        decode_item(self.is_bytes, &self.ty, &self.bundle, bytes)
    }
}

/// A stream item as [ValueOwned]: byte streams are sent as is, see TailBytes.
pub(crate) fn decode_item(
    is_bytes: bool,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
    bytes: TailBytesOwned,
) -> Result<ValueOwned, StreamError> {
    if is_bytes {
        return Ok(ValueOwned::Vec(
            bytes
                .0
                .into_iter()
                .map(|b| ValueOwned::Numeric(NumericValue::U8(b)))
                .collect(),
        ));
    }
    ValueOwned::des_shrink_wrap_dyn(&bytes, ty, bundle)
        .map_err(|e| StreamError::Other(Error::Other(format!("{e:#}"))))
}

/// Sink on a device taking items as [ValueOwned].
pub struct DynSink {
    sink: Sink<TailBytesOwned>,
    ty: TypeOwned,
    is_bytes: bool,
    bundle: Arc<ApiBundleOwned>,
}

impl DynSink {
    fn new(resource: &DynResource, sink: Sink<TailBytesOwned>, ty: TypeOwned) -> Self {
        let is_bytes = ty.is_byte_slice(&resource.bundle).unwrap_or(false);
        DynSink {
            sink,
            ty,
            is_bytes,
            bundle: resource.bundle.clone(),
        }
    }

    /// Sink item type.
    pub fn ty(&self) -> &TypeOwned {
        &self.ty
    }

    /// True for a sink with credit-based flow control, see [Sink].
    pub fn is_credit(&self) -> bool {
        self.sink.is_credit()
    }

    /// See [Sink::set_credit_config].
    pub fn set_credit_config(&mut self, config: CreditConfig) {
        self.sink.set_credit_config(config)
    }

    /// See [Sink::credit_progress].
    pub fn credit_progress(&self) -> Option<(u32, u32)> {
        self.sink.credit_progress()
    }

    pub async fn open(&mut self) -> Result<(), StreamError> {
        self.sink.open().await
    }

    pub fn open_blocking(&mut self) -> Result<(), StreamError> {
        self.sink.open_blocking()
    }

    /// See [Sink::close].
    pub async fn close(&mut self) -> Result<(), StreamError> {
        self.sink.close().await
    }

    /// See [Sink::close_blocking].
    pub fn close_blocking(&mut self) -> Result<(), StreamError> {
        self.sink.close_blocking()
    }

    /// See [Sink::flush].
    pub async fn flush(&mut self) -> Result<(), StreamError> {
        self.sink.flush().await
    }

    /// See [Sink::flush_blocking].
    pub fn flush_blocking(&mut self) -> Result<(), StreamError> {
        self.sink.flush_blocking()
    }

    /// See [Sink::try_flush].
    pub fn try_flush(&mut self) -> Result<bool, StreamError> {
        self.sink.try_flush()
    }

    pub async fn send(&mut self, value: &ValueOwned) -> Result<(), StreamError> {
        let bytes = self.encode(value)?;
        self.sink.send_bytes(&bytes).await
    }

    pub fn send_blocking(&mut self, value: &ValueOwned) -> Result<(), StreamError> {
        let bytes = self.encode(value)?;
        self.sink.send_bytes_blocking(&bytes)
    }

    /// See [Sink::try_send].
    pub fn try_send(&mut self, value: &ValueOwned) -> Result<bool, StreamError> {
        let bytes = self.encode(value)?;
        self.sink.try_send_bytes(&bytes)
    }

    fn encode(&self, value: &ValueOwned) -> Result<Vec<u8>, StreamError> {
        if self.is_bytes {
            // sent as is, see TailBytes
            let ValueOwned::Vec(items) = value else {
                return Err(StreamError::Other(Error::Other(format!(
                    "expected bytes, got {value:?}"
                ))));
            };
            if let Ok(Some(max_len)) = self.ty.max_len(&self.bundle)
                && items.len() > max_len as usize
            {
                return Err(StreamError::Other(Error::Other(format!(
                    "{} bytes, at most {max_len} allowed",
                    items.len()
                ))));
            }
            return items
                .iter()
                .map(|item| match item {
                    ValueOwned::Numeric(NumericValue::U8(b)) => Ok(*b),
                    other => Err(StreamError::Other(Error::Other(format!(
                        "expected u8, got {other:?}"
                    )))),
                })
                .collect();
        }
        value
            .ser_shrink_wrap_dyn(&self.ty, &self.bundle)
            .map_err(|e| StreamError::Other(Error::Other(format!("{e:#}"))))
    }
}

fn other(e: anyhow::Error) -> Error {
    Error::Other(format!("{e:#}"))
}
