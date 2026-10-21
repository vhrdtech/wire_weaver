use std::time::Duration;

// generated clients (the hub's, in `hub`) name this crate by its name
#[cfg(feature = "hub")]
extern crate self as wire_weaver_client;

mod client;
pub mod client_meta;
mod compat;
mod config;
mod connection_event;
mod device_info;
pub mod envelope;
mod error;
mod error_list;
pub(crate) mod event_loop;
pub mod evolution;
pub mod expr;
pub mod follow;
pub mod holders;
pub mod hub;
/// Observing properties (CLT-28), see [client::observe](crate::observe)
pub use client::observe;
mod layout;
mod local_registry;
pub mod quantity;
pub mod tap;
mod tracing;

pub use client::attachment::Attachment;
pub use client::batch::{Batch, BatchItem, Unpack, unpack_timed, unpack_untimed};
pub use client::dyn_client::DynClient;
pub use client::dyn_observe::{DynConfig, DynObserved, DynObserver, DynSamples};
pub use client::dyn_positioned::{DynPositionedStream, DynStreamOpen, seconds};
pub use client::dynamic::{
    DynMultiArgs, DynMultiValue, DynResource, DynResourceKind, DynSink, DynStream,
};
pub use client::introspect::{Introspect, decode_introspection};
pub use client::multi::{self, MultiCall, MultiRead, MultiWrite};
pub use client::positioned::{
    AckMode, Cause, DEFAULT_STREAM_QUEUE_BYTES, DEFAULT_WINDOW, PositionedStream, Run, Second,
    StreamEvent, StreamStats,
};
pub use client::prepared_call::{PendingCall, PreparedCall};
pub use client::prepared_connection::PreparedConnection;
pub use client::prepared_disconnect::PreparedDisconnect;
pub use client::prepared_read::PreparedRead;
pub use client::prepared_stream_open::{PreparedStreamOpen, StreamHandle, StreamOpenResult};
pub use client::prepared_write::PreparedWrite;
pub use client::promise::{Promise, PromiseState};
pub use client::sink::CreditConfig;
pub use client::sink::Sink;
pub use client::stream::{RawStreamEvent, Stream, StreamError, TypedStreamEvent};
pub use client_meta::{ClientKind, ClientMeta};
pub use config::{ClientConfig, DEFAULT_HUB_GIVE_UP};
pub use connection_event::{ConnectionEvent, GapReason};
pub use device_info::{ApiInfo, DeviceApiInfo, DeviceInfo, UsbLocation};
pub use event_loop::commander::Commander;
pub use ww_stream::StreamCursor;

#[cfg(any(feature = "usb", feature = "udp", feature = "in_process"))]
mod framing;
#[cfg(feature = "usb")]
mod usb;
#[cfg(feature = "usb")]
pub use usb::{list_all_devices as list_all_usb_devices, list_devices as list_usb_devices};

#[cfg(feature = "rtt")]
mod rtt;

#[cfg(feature = "in_process")]
pub mod in_process;
#[cfg(feature = "local_socket")]
pub mod local_socket;

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
#[cfg(feature = "stm32_dfu")]
pub use ww_stm32_dfu;
pub use ww_tap;
pub use ww_tape;
pub use ww_version;

pub mod internal {
    pub use crate::device_info::ConnectionInfo;
    pub use crate::event_loop::command::{
        Command, ForwardSink, Forwarded, LoopbackParams, TestProgress,
    };
    pub use crate::tracing::TraceEvent;
}

pub trait WwClient {
    fn default_config() -> ClientConfig;
    fn from_cmd(cmd: Commander) -> Self;
}
