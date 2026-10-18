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

use crate::client::dyn_observe::{DynConfig, DynObserver, DynUpdate, LevelSchema};
use crate::client::dyn_positioned::{DynPositionedStream, DynStreamOpen};
use crate::client::prepared_stream_open::RawOpenResult;
use crate::error_list;
use crate::observe::ObserveMode;
use crate::{
    AckMode, BatchItem, Commander, CreditConfig, Error, PositionedStream, PreparedRead,
    PreparedStreamOpen, RawStreamEvent, Sink, Stream, StreamCursor, StreamError, TypedStreamEvent,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use wire_weaver::ValidIndicesOwned;
use wire_weaver::prelude::{DeserializeShrinkWrap, UNib32};
use wire_weaver::shrink_wrap::BufReader;
use wire_weaver::shrink_wrap::either_any_vec::EitherAnyVec;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{ErrorKindOwned, ErrorOwned, MultiIndexOwned, PathKind, RequestKindOwned};
use ww_numeric::NumericValue;
use ww_self::errors::{error_list_contains, error_value_name, fmt_error_list};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, ArgumentOwned, Delivery,
    ErrorVariantOwned, FlowControl, Multiplicity, PropertyAccess, TypeOwned, ValueOwned,
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
    /// An `#[optional]` item the device leaves out (API-16), requests to it fail locally.
    omitted: bool,
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
        /// `#[errors(..)]` (API-32): the variants of the `Err` type this method returns, `None` for any.
        errors: Option<&'a [ErrorVariantOwned]>,
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
    /// Root of the API. Values are encoded and decoded with concrete types: a bundle with generic types in it
    /// has to be [prepared](ww_self::generics::prepare) first, as introspection data from a device
    /// ([Commander::device_introspect](crate::Commander::device_introspect)) is.
    pub fn root(bundle: Arc<ApiBundleOwned>) -> Self {
        DynResource {
            bundle,
            index_chain: vec![],
            path: String::new(),
            item: None,
            node: Node::Level { trait_idx: None },
            omitted: false,
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
                ApiItemKindOwned::Method {
                    args,
                    return_ty,
                    errors,
                } => DynResourceKind::Method {
                    args,
                    return_ty: return_ty.as_ref(),
                    errors: errors.as_deref(),
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

    /// Whether this is an `#[optional]` resource the device leaves out (API-16): requests to it fail with
    /// [Error::NotImplementedByDevice] without being sent.
    pub fn is_omitted(&self) -> bool {
        self.omitted
    }

    /// Ids of the optional items of this level (the API root or a trait) the device leaves out.
    fn omitted_ids(&self) -> &[UNib32] {
        let omitted = match (&self.node, &self.item) {
            (Node::Level { trait_idx: None }, _) => &self.bundle.root_omitted,
            (
                Node::Level { .. },
                Some(ApiItemOwned {
                    kind: ApiItemKindOwned::Trait { omitted, .. },
                    ..
                }),
            ) => omitted,
            _ => return &[],
        };
        omitted.as_deref().unwrap_or_default()
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
            omitted: self.omitted || self.omitted_ids().contains(&item.id),
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
            omitted: self.omitted,
        })
    }

    fn resolve(&self, item: &ApiItemOwned) -> Result<Node, Error> {
        Ok(match &item.kind {
            ApiItemKindOwned::Trait { trait_idx, .. } => {
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

    /// Path to send a request to, an error for an omitted optional resource.
    fn path_kind(&self) -> Result<PathKind<'_>, Error> {
        if self.omitted {
            let device = self
                .bundle
                .crate_version(self.bundle.root.crate_idx.0)
                .map_err(other)?;
            return Err(Error::NotImplementedByDevice {
                resource: self.path.clone(),
                device: Box::new(device.clone()),
            });
        }
        Ok(PathKind::absolute(&self.index_chain))
    }

    fn method(&self) -> Result<(&[ArgumentOwned], Option<&TypeOwned>), Error> {
        match self.kind()? {
            DynResourceKind::Method {
                args, return_ty, ..
            } => Ok((args, return_ty)),
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
        let value = ValueOwned::des_shrink_wrap_dyn(bytes, return_ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' return value: {e:#}", self.path)))?;
        self.check_error_list(&value);
        Ok(Some(value))
    }

    /// Warns when a method returned an error outside its `#[errors(..)]` (API-32).
    fn check_error_list(&self, value: &ValueOwned) {
        if let ValueOwned::Result(Err(err)) = value {
            self.check_listed(err);
        }
    }

    /// Warns when `err` (of a method, stream function or property write) is outside the item's `#[errors(..)]`.
    fn check_listed(&self, err: &ValueOwned) {
        let Some(list) = self.item.as_ref().and_then(|i| i.kind.error_list()) else {
            return;
        };
        if !error_list_contains(list, err) {
            error_list::unlisted(&self.path, &error_value_name(err), &fmt_error_list(list));
        }
    }

    /// Checks a property read's or write's device error (a `UserBytes` remote error) against its `#[errors(..)]`.
    fn check_property_error<T>(&self, result: Result<T, Error>) -> Result<T, Error> {
        if let Err(Error::RemoteError(remote)) = &result
            && let ErrorKindOwned::UserBytes(bytes) = &remote.kind
            && let Some(ApiItemKindOwned::Property {
                write_err_ty: Some(err_ty),
                errors: Some(_),
                ..
            }) = self.item.as_ref().map(|i| &i.kind)
            && let Ok(err) = ValueOwned::des_shrink_wrap_dyn(bytes, err_ty, &self.bundle)
        {
            self.check_listed(&err);
        }
        result
    }

    /// Call a method with arguments in the order they are defined in, returns None if it returns nothing.
    pub async fn call(
        &self,
        cmd: &Commander,
        args: &[ValueOwned],
    ) -> Result<Option<ValueOwned>, Error> {
        let bytes = self.call_args(args)?;
        let reply = cmd
            .prepare_call::<TailBytesOwned, Error>(self.path_kind()?, Ok(bytes), "")
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
        let mut call = cmd.prepare_call::<TailBytesOwned, Error>(self.path_kind()?, Ok(bytes), "");
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

    fn prepare_read(&self, cmd: &Commander) -> Result<PreparedRead<TailBytesOwned>, Error> {
        let (_, access) = self.property()?;
        Ok(if matches!(access, PropertyAccess::Const) {
            cmd.prepare_const_read(self.path_kind()?)
        } else {
            cmd.prepare_read(self.path_kind()?)
        })
    }

    fn encode_property(&self, value: &ValueOwned) -> Result<Vec<u8>, Error> {
        let (ty, _) = self.property()?;
        value
            .ser_shrink_wrap_dyn(ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' value: {e:#}", self.path)))
    }

    /// Read a property. A `const` is read from the device once per connection, later reads return that value.
    pub async fn read(&self, cmd: &Commander) -> Result<ValueOwned, Error> {
        let bytes = self.check_property_error(self.prepare_read(cmd)?.read().await)?;
        self.decode_property(&bytes)
    }

    /// Read a property. A `const` is read from the device once per connection, later reads return that value.
    pub fn read_blocking(
        &self,
        cmd: &Commander,
        timeout: Option<Duration>,
    ) -> Result<ValueOwned, Error> {
        let mut read = self.prepare_read(cmd)?;
        if let Some(timeout) = timeout {
            read = read.with_timeout(timeout);
        }
        let bytes = self.check_property_error(read.blocking_read())?;
        self.decode_property(&bytes)
    }

    /// Write a property.
    pub async fn write(&self, cmd: &Commander, value: &ValueOwned) -> Result<(), Error> {
        let bytes = self.encode_property(value)?;
        let result = cmd
            .prepare_write::<Error>(self.path_kind()?, Ok(bytes), "")
            .write()
            .await;
        self.check_property_error(result)
    }

    /// Write a property.
    pub fn write_blocking(
        &self,
        cmd: &Commander,
        value: &ValueOwned,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let bytes = self.encode_property(value)?;
        let mut write = cmd.prepare_write::<Error>(self.path_kind()?, Ok(bytes), "");
        if let Some(timeout) = timeout {
            write = write.with_timeout(timeout);
        }
        self.check_property_error(write.blocking_write())
    }

    /// Read which indices are valid for an array of resources.
    pub async fn valid_indices(&self, cmd: &Commander) -> Result<ValidIndicesOwned, Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        cmd.prepare_read::<ValidIndicesOwned>(self.path_kind()?)
            .read()
            .await
    }

    /// Read which indices are valid for an array of resources.
    pub fn valid_indices_blocking(&self, cmd: &Commander) -> Result<ValidIndicesOwned, Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        cmd.prepare_read::<ValidIndicesOwned>(self.path_kind()?)
            .blocking_read()
    }

    /// This level's `+observe` properties, by resource index (TOOL-11): what [Self::observe]'s `entries` addresses
    /// and what a [DynObserver] decodes a `PropertyUpdate` against.
    pub fn observable_properties(&self) -> Result<Vec<(u32, String, TypeOwned)>, Error> {
        Ok(self.observe_schema()?.props().to_vec())
    }

    fn observe_schema(&self) -> Result<LevelSchema, Error> {
        let level = self.level()?;
        let props = level
            .items
            .iter()
            .filter(|item| matches!(item.multiplicity, Multiplicity::Flat))
            .filter_map(|item| match &item.kind {
                ApiItemKindOwned::Property {
                    ty,
                    access:
                        PropertyAccess::ReadOnly { observe: true }
                        | PropertyAccess::ReadWrite { observe: true },
                    ..
                } => Some((item.id.0, item.ident.clone(), ty.clone())),
                _ => None,
            })
            .collect();
        Ok(LevelSchema::new(self.bundle.clone(), props))
    }

    /// Observe this trait level's `+observe` properties without generated code (TOOL-11): one device subscription
    /// for the whole level, decoded from introspection alone. `entries` is the mode to request per resource index
    /// (see [Self::observable_properties]); an index that isn't `+observe` is refused by the device.
    pub async fn observe(
        &self,
        cmd: &Commander,
        entries: BTreeMap<u32, ObserveMode>,
    ) -> Result<DynObserver, Error> {
        let schema = self.observe_schema()?;
        let prepared = cmd.prepare_observe::<DynUpdate, DynUpdate>(
            self.path_kind()?,
            DynConfig(entries),
            Some,
        );
        Ok(DynObserver::new(prepared.subscribe().await?, schema))
    }

    /// Blocking variant of [Self::observe].
    pub fn observe_blocking(
        &self,
        cmd: &Commander,
        entries: BTreeMap<u32, ObserveMode>,
    ) -> Result<DynObserver, Error> {
        let schema = self.observe_schema()?;
        let prepared = cmd.prepare_observe::<DynUpdate, DynUpdate>(
            self.path_kind()?,
            DynConfig(entries),
            Some,
        );
        Ok(DynObserver::new(prepared.subscribe_blocking()?, schema))
    }

    /// Open a plain stream from a device (a `stream!` without arguments): the device starts it.
    // TODO(CLT-23): stream functions with arguments, counted and reliable streams
    pub async fn stream(&self, cmd: &Commander) -> Result<DynStream, Error> {
        let ty = self.plain_stream_ty()?;
        let stream = cmd
            .prepare_stream_open::<(), Stream<TailBytesOwned>>(self.path_kind()?, None, Ok(vec![]))
            .call()
            .await?;
        DynStream::new(self, stream, ty)
    }

    /// Blocking variant of [Self::stream].
    pub fn stream_blocking(&self, cmd: &Commander) -> Result<DynStream, Error> {
        let ty = self.plain_stream_ty()?;
        let stream = cmd
            .prepare_stream_open::<(), Stream<TailBytesOwned>>(self.path_kind()?, None, Ok(vec![]))
            .blocking_call()?;
        DynStream::new(self, stream, ty)
    }

    /// Item type of a plain stream without arguments and result.
    fn plain_stream_ty(&self) -> Result<TypeOwned, Error> {
        let ty = self.stream_ty(true)?.0.clone();
        match self.stream_fn_ref() {
            // a bare `stream!(name: T)` with `#[batched]` (API-41): delivery is Plain, but it's modeled as a
            // StreamFn now that it carries `batched`, same as a plain stream otherwise
            Some(s) if s.delivery == Delivery::Plain && s.args.is_empty() && s.return_ty.is_none() => {
                Ok(ty)
            }
            Some(_) => Err(Error::Other(format!(
                "'{}' is a stream function or a counted or reliable stream, not supported by the dynamic \
                 client yet",
                self.display_path()
            ))),
            None => Ok(ty),
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

    fn dyn_positioned(
        &self,
        stream: PositionedStream<TailBytesOwned>,
    ) -> Result<DynPositionedStream, Error> {
        let ty = self
            .stream_ty(true)
            .map(|(ty, _)| ty.clone())
            .expect("checked when opening");
        let stream_fn = self.stream_fn_ref();
        let batched = match stream_fn {
            Some(stream_fn) => {
                let wire_item_ty = stream_fn
                    .wire_item_ty()
                    .map_err(|e| Error::Other(format!("'{}': {e:#}", self.path)))?;
                crate::client::dyn_positioned::DynBatching::new(
                    stream_fn.batched,
                    wire_item_ty,
                    &self.bundle,
                )
                .map_err(|e| Error::Other(format!("'{}': {e}", self.path)))?
            }
            None => None,
        };
        Ok(DynPositionedStream {
            stream,
            is_bytes: ty.is_byte_slice(&self.bundle).unwrap_or(false),
            ty,
            bundle: self.bundle.clone(),
            batched,
            pending: Default::default(),
        })
    }

    /// This item as a stream function, see [ApiItemKindOwned::as_stream_fn].
    fn stream_fn_ref(&self) -> Option<ww_self::StreamFnRef<'_>> {
        self.item.as_ref().and_then(|i| i.kind.as_stream_fn())
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
            Some(ValueOwned::Result(Err(e))) => {
                self.check_listed(&e);
                DynStreamOpen::Rejected(*e)
            }
            Some(ValueOwned::Result(Ok(v))) => DynStreamOpen::Open {
                result: Some(*v),
                stream: Box::new(self.dyn_positioned(stream)?),
            },
            result => DynStreamOpen::Open {
                result,
                stream: Box::new(self.dyn_positioned(stream)?),
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
                    cmd.prepare_stream_open(self.path_kind()?, Some(delivery), Ok(vec![]));
                open.resume = Some(cursor);
                open
            }
            None => cmd.prepare_stream_open(
                self.path_kind()?,
                Some(delivery),
                self.stream_fn_args(args),
            ),
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
            None => cmd.prepare_sink(self.path_kind()?).await?,
            Some(FlowControl::Credit) => cmd.prepare_credit_sink(self.path_kind()?).await?,
        };
        Ok(DynSink::new(self, sink, ty))
    }

    /// Sink on a device.
    pub fn sink_blocking(&self, cmd: &Commander) -> Result<DynSink, Error> {
        let (ty, flow) = self.stream_ty(false)?;
        let ty = ty.clone();
        let sink = match flow {
            None => cmd.prepare_sink_blocking(self.path_kind()?)?,
            Some(FlowControl::Credit) => cmd.prepare_credit_sink_blocking(self.path_kind()?)?,
        };
        Ok(DynSink::new(self, sink, ty))
    }

    /// This array's own absolute path (an `#[optional]` one checked first), the request path of every multi
    /// request on it, see [Self::multi_read]/[Self::multi_write]/[Self::multi_call].
    fn array_path(&self) -> Result<Vec<UNib32>, Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        // reuses path_kind()'s `#[optional]` check (API-16); an array is always addressed by Absolute path
        match self.path_kind()? {
            PathKind::Absolute { path } => path
                .iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| Error::Other(format!("{e:#}"))),
            _ => unreachable!("DynResource paths are always Absolute"),
        }
    }

    /// Looks up a resource by name inside this array's trait level (array-of-trait, e.g. `pin[].mode`), or this
    /// array's own kind when `child` is `None` (array of a flat property/method/stream, e.g.
    /// `property!(led[]: bool)`). Returns the resource's id for `in_each_array_id` (`None` for the flat case)
    /// together with its kind. Bypasses [Self::kind], which always reports [DynResourceKind::Array] for an array
    /// node regardless of what its element actually is.
    fn array_child(
        &self,
        child: Option<&str>,
    ) -> Result<(Option<UNib32>, &ApiItemKindOwned), Error> {
        if !matches!(self.node, Node::Array) {
            return Err(self.not_a("array"));
        }
        let item = self.item.as_ref().expect("array node always has an item");
        match child {
            None => Ok((None, &item.kind)),
            Some(name) => {
                let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind else {
                    return Err(Error::Other(format!(
                        "'{}' is not an array of a trait, pass child: None to address it directly",
                        self.display_path()
                    )));
                };
                let level = self.bundle.get_trait(trait_idx.0).map_err(other)?;
                let found = level
                    .items
                    .iter()
                    .find(|i| i.ident == name)
                    .ok_or_else(|| {
                        let names: Vec<_> = level.items.iter().map(|i| i.ident.as_str()).collect();
                        Error::Other(format!(
                            "'{}' has no resource '{name}', available: {}",
                            self.display_path(),
                            names.join(", ")
                        ))
                    })?;
                Ok((Some(found.id), &found.kind))
            }
        }
    }

    fn array_property(&self, child: Option<&str>) -> Result<(Option<UNib32>, &TypeOwned), Error> {
        let (id, kind) = self.array_child(child)?;
        match kind {
            ApiItemKindOwned::Property { ty, .. } => Ok((id, ty)),
            _ => Err(self.not_a("property")),
        }
    }

    #[allow(clippy::type_complexity)]
    fn array_method(
        &self,
        child: Option<&str>,
    ) -> Result<(Option<UNib32>, &[ArgumentOwned], Option<&TypeOwned>), Error> {
        let (id, kind) = self.array_child(child)?;
        match kind {
            ApiItemKindOwned::Method {
                args, return_ty, ..
            } => Ok((id, args, return_ty.as_ref())),
            _ => Err(self.not_a("method")),
        }
    }

    /// Read the same property (or flat array element, `child: None`) across an array (CLT-15): `multi_idx`
    /// selects which elements (`Range`/`List`/`Mask32`, at least 2), `child` names one of the array's trait's
    /// resources (`None` for an array of a flat property, e.g. `property!(led[]: bool)`).
    pub async fn multi_read(
        &self,
        cmd: &Commander,
        multi_idx: MultiIndexOwned,
        child: Option<&str>,
    ) -> Result<Vec<Result<ValueOwned, ErrorOwned>>, Error> {
        let path = self.array_path()?;
        let (in_each_array_id, ty) = self.array_property(child)?;
        let ty = ty.clone();
        let count = multi_idx.len();
        let bytes = crate::client::multi::send_multi(
            cmd.transport(),
            path,
            RequestKindOwned::MultiRead {
                multi_idx,
                in_each_array_id,
            },
            None,
        )
        .await?;
        self.decode_dyn_items(&bytes, count, &ty)
    }

    /// Write the same property (or flat array element) across an array (CLT-15): see [Self::multi_read].
    /// `values` gives one value for every element (`Same`) or one per element in `multi_idx` order (`Different`).
    pub async fn multi_write(
        &self,
        cmd: &Commander,
        multi_idx: MultiIndexOwned,
        child: Option<&str>,
        values: DynMultiValue<'_>,
    ) -> Result<Vec<Result<(), ErrorOwned>>, Error> {
        let path = self.array_path()?;
        let (in_each_array_id, ty) = self.array_property(child)?;
        let count = multi_idx.len();
        let multi_data =
            crate::client::multi::merge_args(values.encode(ty, &self.bundle, count)?)?;
        let bytes = crate::client::multi::send_multi(
            cmd.transport(),
            path,
            RequestKindOwned::MultiWrite {
                multi_idx,
                in_each_array_id,
                multi_data,
            },
            None,
        )
        .await?;
        crate::client::multi::decode_items(&bytes, count)
    }

    /// Call the same method across an array (CLT-15): see [Self::multi_read]. `args` gives one argument list for
    /// every element (`Same`) or one per element in `multi_idx` order (`Different`); `Ok(None)` per index for a
    /// method without a return value.
    pub async fn multi_call(
        &self,
        cmd: &Commander,
        multi_idx: MultiIndexOwned,
        child: Option<&str>,
        args: DynMultiArgs<'_>,
    ) -> Result<Vec<Result<Option<ValueOwned>, ErrorOwned>>, Error> {
        let path = self.array_path()?;
        let (in_each_array_id, defs, return_ty) = self.array_method(child)?;
        let defs = defs.to_vec();
        let return_ty = return_ty.cloned();
        let count = multi_idx.len();
        let multi_args =
            crate::client::multi::merge_args(args.encode(&defs, &self.bundle, count)?)?;
        let bytes = crate::client::multi::send_multi(
            cmd.transport(),
            path,
            RequestKindOwned::MultiCall {
                multi_idx,
                in_each_array_id,
                multi_args,
            },
            None,
        )
        .await?;
        match &return_ty {
            Some(ty) => {
                let values = self.decode_dyn_items(&bytes, count, ty)?;
                Ok(values.into_iter().map(|r| r.map(Some)).collect())
            }
            // no return type: every item is just an error flag with nothing on the Ok side
            None => crate::client::multi::decode_items::<()>(&bytes, count)
                .map(|v| v.into_iter().map(|r| r.map(|()| None)).collect()),
        }
    }

    /// Decodes `count` items of an `EitherAnyVec` response as `ty` (CLT-15): unlike
    /// [ValueOwned::des_shrink_wrap_dyn] (a whole top-level response, one value), an item here is one of several
    /// packed together and must be read through [EitherAnyVecIter::next_with], which bounds a dynamically unsized
    /// `ty` to its own size first (from the trailer) so it doesn't overrun into the next item.
    fn decode_dyn_items(
        &self,
        bytes: &[u8],
        count: usize,
        ty: &TypeOwned,
    ) -> Result<Vec<Result<ValueOwned, ErrorOwned>>, Error> {
        let mut rd = BufReader::new(bytes);
        let vec = EitherAnyVec::des_shrink_wrap(&mut rd)?;
        let mut iter = vec.iter();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut dyn_err = None;
            let item = iter.next_with(
                |rd| rd.read_owned::<ErrorOwned>().map(either::Either::Left),
                |rd| match ValueOwned::des_shrink_wrap_field_dyn(rd, ty, &self.bundle) {
                    Ok(v) => Ok(either::Either::Right(v)),
                    Err(e) => {
                        dyn_err = Some(e);
                        Err(wire_weaver::shrink_wrap::Error::OutOfBoundsReadRawSlice)
                    }
                },
            );
            if let Some(e) = dyn_err {
                return Err(Error::Other(format!("'{}' value: {e:#}", self.path)));
            }
            out.push(item?.into());
        }
        Ok(out)
    }
}

/// Arguments for [DynResource::multi_call]: one argument list used for every index (`Same`), or one per index in
/// `multi_idx` order (`Different`).
pub enum DynMultiArgs<'a> {
    Same(&'a [ValueOwned]),
    Different(&'a [Vec<ValueOwned>]),
}

impl DynMultiArgs<'_> {
    fn encode(
        &self,
        defs: &[ArgumentOwned],
        bundle: &ApiBundleOwned,
        count: usize,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let ser_one = |args: &[ValueOwned]| -> Result<Vec<u8>, Error> {
            if args.len() != defs.len() {
                return Err(Error::Other(format!(
                    "takes {} arguments, got {}",
                    defs.len(),
                    args.len()
                )));
            }
            ValueOwned::ser_shrink_wrap_vec_dyn(args, defs.iter().map(|a| &a.ty), bundle)
                .map_err(|e| Error::Other(format!("arguments: {e:#}")))
        };
        match self {
            DynMultiArgs::Same(args) => Ok(vec![ser_one(args)?; count]),
            DynMultiArgs::Different(list) => {
                if list.len() != count {
                    return Err(Error::MultiReq(format!(
                        "multi_idx covers {count} indices, got {} argument lists",
                        list.len()
                    )));
                }
                list.iter().map(|args| ser_one(args)).collect()
            }
        }
    }
}

/// Values for [DynResource::multi_write]: one value used for every index (`Same`), or one per index in
/// `multi_idx` order (`Different`).
pub enum DynMultiValue<'a> {
    Same(&'a ValueOwned),
    Different(&'a [ValueOwned]),
}

impl DynMultiValue<'_> {
    fn encode(
        &self,
        ty: &TypeOwned,
        bundle: &ApiBundleOwned,
        count: usize,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let ser_one = |value: &ValueOwned| -> Result<Vec<u8>, Error> {
            value
                .ser_shrink_wrap_dyn(ty, bundle)
                .map_err(|e| Error::Other(format!("value: {e:#}")))
        };
        match self {
            DynMultiValue::Same(value) => Ok(vec![ser_one(value)?; count]),
            DynMultiValue::Different(list) => {
                if list.len() != count {
                    return Err(Error::MultiReq(format!(
                        "multi_idx covers {count} indices, got {} values",
                        list.len()
                    )));
                }
                list.iter().map(ser_one).collect()
            }
        }
    }
}

/// Stream from a device with items as [ValueOwned]. Batched (API-41): [recv](Self::recv) and the others still give
/// one item at a time, [recv_batch](Self::recv_batch) a whole batch.
pub struct DynStream {
    stream: Stream<TailBytesOwned>,
    ty: TypeOwned,
    is_bytes: bool,
    bundle: Arc<ApiBundleOwned>,
    batched: Option<crate::client::dyn_positioned::DynBatching>,
    pending: std::collections::VecDeque<ValueOwned>,
}

impl DynStream {
    fn new(
        resource: &DynResource,
        stream: Stream<TailBytesOwned>,
        ty: TypeOwned,
    ) -> Result<Self, Error> {
        let is_bytes = ty.is_byte_slice(&resource.bundle).unwrap_or(false);
        let batched = match resource.stream_fn_ref() {
            Some(s) => {
                let wire_item_ty = s
                    .wire_item_ty()
                    .map_err(|e| Error::Other(format!("'{}': {e:#}", resource.path)))?;
                crate::client::dyn_positioned::DynBatching::new(
                    s.batched,
                    wire_item_ty,
                    &resource.bundle,
                )
                .map_err(|e| Error::Other(format!("'{}': {e}", resource.path)))?
            }
            None => None,
        };
        Ok(DynStream {
            stream,
            ty,
            is_bytes,
            bundle: resource.bundle.clone(),
            batched,
            pending: Default::default(),
        })
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
        loop {
            if let Some(v) = self.pending.pop_front() {
                return Ok(v);
            }
            let ev = self.stream.recv_any().await?;
            self.handle(ev)?;
        }
    }

    /// Receive one item, see [Stream::recv_blocking].
    pub fn recv_blocking(&mut self) -> Result<ValueOwned, StreamError> {
        loop {
            if let Some(v) = self.pending.pop_front() {
                return Ok(v);
            }
            let ev = self.stream.recv_any_blocking()?;
            self.handle(ev)?;
        }
    }

    /// Receive one item if available, see [Stream::try_recv].
    pub fn try_recv(&mut self) -> Result<Option<ValueOwned>, StreamError> {
        loop {
            if let Some(v) = self.pending.pop_front() {
                return Ok(Some(v));
            }
            match self.stream.try_recv_any()? {
                Some(ev) => self.handle(ev)?,
                None => return Ok(None),
            }
        }
    }

    /// The rest of the next batch, or the next one whole (API-41): its items, each with its device time
    /// (unwrapped). An item of a stream that isn't batched is a batch of one, without a time.
    pub async fn recv_batch(&mut self) -> Result<Vec<BatchItem<ValueOwned>>, StreamError> {
        loop {
            if !self.pending.is_empty() {
                return Ok(self.pending.drain(..).map(|value| BatchItem { t: None, value }).collect());
            }
            let ev = self.stream.recv_any().await?;
            self.handle_batch(ev)?;
        }
    }

    /// One raw event: data decoded (into `pending`, items for a batched stream, one value otherwise), a
    /// reconnection restarts the device time, anything else (sideband, a disconnect) is an error, as for
    /// [Stream::recv].
    fn handle(&mut self, ev: TypedStreamEvent<TailBytesOwned>) -> Result<(), StreamError> {
        match ev {
            TypedStreamEvent::Data(bytes) => self.decode(&bytes)?,
            TypedStreamEvent::Connected => {
                if let Some(batched) = &mut self.batched {
                    batched.restart();
                }
            }
            TypedStreamEvent::Dropped { .. } => {}
            TypedStreamEvent::Sideband(s) => {
                return Err(StreamError::UnexpectedEvent(RawStreamEvent::Sideband(s)));
            }
            TypedStreamEvent::Disconnected => {
                return Err(StreamError::UnexpectedEvent(RawStreamEvent::Disconnected));
            }
        }
        Ok(())
    }

    /// Like [Self::handle], but every item of a batched stream's data event goes into `pending` with its own time
    /// (for [Self::recv_batch]: nothing is ever decoded down to just the first item).
    fn handle_batch(&mut self, ev: TypedStreamEvent<TailBytesOwned>) -> Result<(), StreamError> {
        match ev {
            TypedStreamEvent::Data(bytes) => match &mut self.batched {
                None => self.pending.push_back(decode_item(
                    self.is_bytes,
                    &self.ty,
                    &self.bundle,
                    bytes,
                )?),
                Some(batched) => {
                    let items = batched.decode(&bytes, &self.bundle)?;
                    self.pending.extend(items.into_iter().map(|i| i.value));
                }
            },
            other => self.handle(other)?,
        }
        Ok(())
    }

    /// One item's bytes into `pending`: a batched stream's data event is every one of its items, a plain one is
    /// the value itself.
    fn decode(&mut self, bytes: &TailBytesOwned) -> Result<(), StreamError> {
        match &mut self.batched {
            None => self.pending.push_back(decode_item(
                self.is_bytes,
                &self.ty,
                &self.bundle,
                bytes.clone(),
            )?),
            Some(batched) => {
                let items = batched.decode(bytes, &self.bundle)?;
                self.pending.extend(items.into_iter().map(|i| i.value));
            }
        }
        Ok(())
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
    pub fn credit_progress(&self) -> Option<(u64, u64)> {
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
