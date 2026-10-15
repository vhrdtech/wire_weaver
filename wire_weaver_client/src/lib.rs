use std::time::Duration;

mod client;
mod compat;
mod config;
mod device_info;
mod error;
mod error_list;
pub(crate) mod event_loop;
pub mod evolution;
pub mod follow;
mod layout;
mod local_registry;
mod tracing;

pub use client::attachment::Attachment;
pub use client::credit::CreditConfig;
pub use client::dyn_client::DynClient;
pub use client::dyn_positioned::{DynPositionedStream, DynStreamOpen, seconds};
pub use client::dynamic::{DynResource, DynResourceKind, DynSink, DynStream};
pub use client::introspect::Introspect;
pub use client::multi::{self, MultiCall, MultiRead, MultiWrite};
pub use client::positioned::{
    AckMode, Cause, DEFAULT_STREAM_QUEUE_BYTES, DEFAULT_WINDOW, PositionedStream, Run, Second,
    StreamEvent, StreamStats,
};
pub use client::prepared_call::PreparedCall;
pub use client::prepared_connection::PreparedConnection;
pub use client::prepared_disconnect::PreparedDisconnect;
pub use client::prepared_read::PreparedRead;
pub use client::prepared_stream_open::{PreparedStreamOpen, StreamHandle, StreamOpenResult};
pub use client::prepared_write::PreparedWrite;
pub use client::promise::{Promise, PromiseState};
pub use client::sink::Sink;
pub use client::stream::{RawStreamEvent, Stream, StreamError, TypedStreamEvent};
pub use config::ClientConfig;
pub use device_info::{ApiInfo, DeviceApiInfo, DeviceInfo, UsbLocation};
pub use event_loop::commander::Commander;
pub use ww_stream::StreamCursor;

#[cfg(feature = "usb")]
mod usb;
#[cfg(feature = "usb")]
pub use usb::{list_all_devices as list_all_usb_devices, list_devices as list_usb_devices};

#[cfg(feature = "rtt")]
mod rtt;

#[cfg(feature = "in_process")]
pub mod in_process;

#[cfg(feature = "ws")]
mod ws;

#[cfg(feature = "udp")]
mod udp;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_CMD_QUEUE_SIZE: usize = 8_192;
/// Maximum ww_client_server message this host can receive; advertised to a device during link setup
/// and used to size rx assembly buffers.
pub(crate) const DEFAULT_MAX_MESSAGE_SIZE: usize = 16_384;
pub type SeqTy = u32;
/// Default for [ClientConfig::max_seq], the largest request seq number that is serialized into 3 bytes.
pub const DEFAULT_MAX_SEQ: SeqTy = (1 << 21) - 1;
pub use error::{CallError, DecodeReply, Error, OutcomeError};
pub use error_list::{ErrorList, set_strict_error_lists};

pub use wire_weaver_snapshots as snapshots;
pub use ww_client_server;
pub use ww_numeric;
#[cfg(feature = "picoboot")]
pub use ww_picoboot;
pub use ww_self;
pub use ww_version;

pub mod internal {
    pub use crate::device_info::ConnectionInfo;
    pub use crate::event_loop::command::{Command, LoopbackParams, TestProgress};
    pub use crate::tracing::TraceEvent;
}

pub trait WwClient {
    fn default_config() -> ClientConfig;
    fn from_cmd(cmd: Commander) -> Self;
}
