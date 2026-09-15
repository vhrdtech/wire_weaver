use std::time::Duration;

mod client;
mod config;
mod device_info;
mod error;
pub(crate) mod event_loop;
mod tracing;

pub use client::attachment::Attachment;
pub use client::dyn_client::DynClient;
pub use client::introspect::Introspect;
pub use client::multi_read::MultiRead;
pub use client::prepared_call::PreparedCall;
pub use client::prepared_connection::PreparedConnection;
pub use client::prepared_disconnect::PreparedDisconnect;
pub use client::prepared_read::PreparedRead;
pub use client::prepared_write::PreparedWrite;
pub use client::promise::{Promise, PromiseState};
pub use client::sink::Sink;
pub use client::stream::{Stream, StreamError, StreamEvent, TypedStreamEvent};
pub use config::ClientConfig;
pub use device_info::{ApiInfo, DeviceApiInfo, DeviceInfo};
pub use event_loop::commander::Commander;

#[cfg(feature = "usb")]
mod usb;

#[cfg(feature = "rtt")]
mod rtt;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_CMD_QUEUE_SIZE: usize = 8_192;
const DEFAULT_MAX_MESSAGE_SIZE: usize = 16_384;
pub type SeqTy = u16;
pub use error::Error;

pub use ww_client_server;
pub use ww_self;
pub use ww_version;

pub mod internal {
    pub use crate::device_info::ConnectionInfo;
    pub use crate::event_loop::command::{Command, TestProgress};
}

pub trait WwClient {
    fn default_config() -> ClientConfig;
    fn from_cmd(cmd: Commander) -> Self;
}
