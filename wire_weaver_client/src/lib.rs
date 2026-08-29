use std::time::Duration;

mod client;
mod commander;
mod config;
mod device_info;
mod error;
pub(crate) mod event_loop;
mod tracing;

pub use client::attachment::Attachment;
pub use client::introspect::Introspect;
pub use client::prepared_call::PreparedCall;
pub use client::prepared_connection::PreparedConnection;
pub use client::prepared_read::PreparedRead;
pub use client::prepared_write::PreparedWrite;
pub use client::promise::Promise;
pub use client::sink::Sink;
pub use client::stream::{Stream, StreamError, StreamEvent, TypedStreamEvent};
pub use commander::Commander;
pub use config::ClientConfig;
pub use device_info::{ApiInfo, DeviceInfo};

#[cfg(feature = "usb")]
mod usb;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_CMD_QUEUE_SIZE: usize = 8_192;
pub type SeqTy = u16;
pub use error::Error;

// pub fn start(filter: DeviceFilter) {}
