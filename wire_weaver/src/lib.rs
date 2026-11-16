#![cfg_attr(not(feature = "std"), no_std)]

pub mod api_doc;
pub mod api_id;
#[cfg(feature = "std")]
pub mod call;
mod call_context;
mod context;
mod disconnect_reason;
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
pub use rpc::{GetResult, RpcResult, SetResult, Unimplemented};
pub use shrink_wrap;
pub use shrink_wrap::derive_shrink_wrap;
use shrink_wrap::prelude::ShrinkWrapError;
pub use valid_indices::ValidIndices;
#[cfg(feature = "std")]
pub use valid_indices::ValidIndicesOwned;
pub use ww_api;
pub use ww_api::{compact_version, full_version, ww_codegen, ww_trait};
pub use ww_version;
use ww_version::FullVersion;

/// Everything a server or client crate names: the runtime, and the API prelude of [ww_api] (the macros, the
/// `shrink_wrap` prelude, `CommonError`, `FullVersion`), since handlers speak the API's vocabulary.
pub mod prelude {
    pub use crate::rpc::GetResult::{GetError, Value};
    pub use crate::rpc::RpcResult::{Deferred, Ready};
    pub use crate::rpc::SetResult::{Set, SetError};
    pub use crate::rpc::{GetResult, RpcResult, SetResult, Unimplemented};
    pub use crate::valid_indices::ValidIndices;
    #[cfg(feature = "std")]
    pub use crate::valid_indices::ValidIndicesOwned;
    pub use crate::ww_unimplemented;
    pub use crate::{
        BlockingEventOut, BlockingMessageSink, CallContext, Context, EventOut, EventQueue,
        EventWriter, MessageSink, Priority, ReplyTo, SendError, WireWeaverApiBackend,
        WireWeaverAsyncApiBackend,
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
