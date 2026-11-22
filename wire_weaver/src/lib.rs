//! The runtime crate of generated servers and clients: re-exports [`ww_api`]'s prelude (the derive macros, the
//! `shrink_wrap` prelude, [`CommonError`], `FullVersion`), the sans-IO handler [`Context`] (sequence number,
//! medium, [`EventOut`] / [`BlockingEventOut`]), and the per-call results [`RpcResult`] / [`GetResult`] /
//! [`SetResult`]. Generated server code is IO-free; transport lives in `wire_weaver_client`, `ww_device` and the
//! medium crates.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod api_id;
#[cfg(feature = "std")]
pub mod authorize;
#[cfg(feature = "std")]
pub mod call;
mod call_context;
mod context;
mod disconnect_reason;
#[cfg(feature = "std")]
pub mod forward;
#[doc(hidden)]
pub mod handler;
mod priority;
mod rpc;
mod test;
mod valid_indices;

pub use call_context::{
    CallContext, ConnectionId, Gid, Peer, PeerKey, Pid, SpanId, TraceContext, TraceFlags, TraceId,
    Uid,
};
pub use context::{
    BlockingEventOut, BlockingMessageSink, Context, EventOut, EventQueue, EventWriter, MessageSink,
    ReplyTo, SendError,
};
pub use disconnect_reason::DisconnectReason;
pub use priority::Priority;
pub use rpc::{Failure, GetResult, IntoOutput, RpcResult, SetResult, Unimplemented};
pub use shrink_wrap;
pub use shrink_wrap::derive_shrink_wrap;
use shrink_wrap::prelude::ShrinkWrapError;
pub use valid_indices::ValidIndices;
#[cfg(feature = "std")]
pub use valid_indices::ValidIndicesOwned;
pub use ww_api;
pub use ww_api::ww_error::CommonError;
pub use ww_api::{Error, api_error, compact_version, full_version, ww_codegen, ww_trait};
/// The API model (`ww_self`), for generated code of the authorization wrapper.
#[cfg(feature = "std")]
#[doc(hidden)]
pub use ww_self;
pub use ww_version;
use ww_version::FullVersion;

/// Everything a server or client crate names: the runtime, and the API prelude of [ww_api] (the macros, the
/// `shrink_wrap` prelude, `CommonError`, `FullVersion`), since handlers speak the API's vocabulary.
pub mod prelude {
    pub use crate::rpc::GetResult::{GetError, Value};
    pub use crate::rpc::RpcResult::{Deferred, Ready};
    pub use crate::rpc::SetResult::{Set, SetError};
    pub use crate::rpc::{Failure, GetResult, IntoOutput, RpcResult, SetResult, Unimplemented};
    pub use crate::valid_indices::ValidIndices;
    #[cfg(feature = "std")]
    pub use crate::valid_indices::ValidIndicesOwned;
    pub use crate::ww_unimplemented;
    pub use crate::{
        BlockingEventOut, BlockingMessageSink, CallContext, Context, EventOut, EventQueue,
        EventWriter, MessageSink, Priority, ReplyTo, SendError, WireWeaverApiBackend,
        WireWeaverAsyncApiBackend, WireWeaverSharedApiBackend,
    };
    pub use ww_api::prelude::*;
}

/// Async server backend, glue between a device event loop (e.g., `ww_device::Server`) and a generated server.
///
/// Usually forwards to the generated `process_request_bytes`, directly if it was generated with
/// `use_async = true`, or through [EventWriter::queued] otherwise.
pub trait WireWeaverAsyncApiBackend {
    /// Medium type passed to handlers in [Context], must match `ww_codegen!(.., medium = "..")`, `()` by default.
    type Medium: Copy;

    /// Deserialize request and process it. Events sent by handlers go to `out`, the reply is serialized into
    /// `scratch` and returned (empty if there is nothing to send back).
    fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: Self::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;

    /// Implemented version of an API. Return `<your_ww_api_crate>::DEVICE_API_ROOT_FULL_GID` from this method.
    fn version(&self) -> FullVersion<'_>;
}

/// An async server as a host runtime (SRV-12) serves it: every request with the connection's origin, and the shared
/// items (SRV-65, `ww_codegen!(.., receiver_model = "..")`) side by side. The runtime asks
/// [is_shared](Self::is_shared) for each request and serves a shared one through
/// [process_shared_bytes](Self::process_shared_bytes) with `&self`, while other shared ones are in flight; everything
/// else goes through [process_exclusive_bytes](Self::process_exclusive_bytes) and runs alone.
///
/// Usually forwards to the generated `is_shared`, `process_shared_from` and `process_request_from`. A server without a
/// `receiver_model` implements it the same way: its `is_shared` says no to every request, so every request runs
/// alone.
pub trait WireWeaverSharedApiBackend: WireWeaverAsyncApiBackend {
    /// Deserialize a request and process it through the `&mut self` handlers, alone. `origin` is who asked, as the
    /// connection knows it; the request's `seq` and `priority` replace its own. Events go to `out`, the reply is
    /// serialized into `scratch` and returned (empty if there is nothing to send back).
    fn process_exclusive_bytes<'a>(
        &mut self,
        origin: &CallContext,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: Self::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;

    /// Whether the request in `data` goes to [process_shared_bytes](Self::process_shared_bytes). Takes no `self`, so
    /// the runtime can ask while an exclusive request holds the server.
    fn is_shared(data: &[u8]) -> bool;

    /// Deserialize a shared request and process it through `&self` handlers. `origin` is who asked, as the
    /// connection knows it; the request's `seq` and `priority` replace its own. Events go to `out`, the reply is
    /// serialized into `scratch` and returned (empty if there is nothing to send back).
    fn process_shared_bytes<'a>(
        &self,
        origin: &CallContext,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: Self::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;
}

/// Same as [WireWeaverAsyncApiBackend], for blocking event loops (e.g., `ww_device::blocking::Server`) and servers
/// generated with `use_async = false`.
pub trait WireWeaverApiBackend {
    /// Medium type passed to handlers in [Context], must match `ww_codegen!(.., medium = "..")`, `()` by default.
    type Medium: Copy;

    /// Deserialize request and process it. Events sent by handlers go to `out`, the reply is serialized into
    /// `scratch` and returned (empty if there is nothing to send back).
    fn process_bytes<'a>(
        &mut self,
        out: &mut impl BlockingEventOut,
        medium: Self::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError>;

    /// Implemented version of an API. Return `<your_ww_api_crate>::DEVICE_API_ROOT_FULL_GID` from this method.
    fn version(&self) -> FullVersion<'_>;
}
