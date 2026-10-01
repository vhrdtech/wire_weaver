#![cfg_attr(not(feature = "std"), no_std)]

pub mod api_id;
mod context;
mod disconnect_reason;
mod rpc;
mod test;
mod valid_indices;

pub use context::{
    BlockingEventOut, BlockingMessageSink, Context, EventOut, EventQueue, EventWriter, MessageSink,
    ReplyTo, SendError,
};
pub use disconnect_reason::DisconnectReason;
pub use rpc::{GetResult, RpcResult, SetResult, Unimplemented};
pub use shrink_wrap;
pub use shrink_wrap::derive_shrink_wrap;
use shrink_wrap::prelude::ShrinkWrapError;
pub use valid_indices::ValidIndices;
#[cfg(feature = "std")]
pub use valid_indices::ValidIndicesOwned;
pub use wire_weaver_derive::{compact_version, full_version, ww_api, ww_codegen, ww_trait};
pub use ww_version;
use ww_version::FullVersion;

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
        BlockingEventOut, BlockingMessageSink, Context, EventOut, EventQueue, EventWriter,
        MessageSink, ReplyTo, SendError, WireWeaverApiBackend, WireWeaverAsyncApiBackend,
    };
    pub use shrink_wrap;
    pub use shrink_wrap::prelude::*;
    pub use wire_weaver_derive::{
        compact_version, full_version, ww_api, ww_api_root, ww_codegen, ww_impl, ww_trait,
    };
    pub use ww_version;
    pub use ww_version::FullVersion;
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
