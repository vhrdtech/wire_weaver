use std::time::Duration;

pub use wire_weaver_client;

#[cfg(feature = "usb")]
pub use wire_weaver_usb_host;

#[cfg(feature = "net")]
pub use wire_weaver_net_host;

mod attachment;
mod commander;
mod config;
pub mod device_info;
mod introspect;
mod multi_read;
mod prepared_call;
mod prepared_connection;
mod prepared_read;
mod prepared_write;
mod promise;
mod sink;
mod stream;

pub use attachment::Attachment;
pub use commander::Commander;
pub use config::ClientConfig;
pub use prepared_call::PreparedCall;
pub use prepared_connection::PreparedConnection;
pub use prepared_read::PreparedRead;
pub use prepared_write::PreparedWrite;
pub use sink::Sink;
pub use stream::{Stream, StreamError};

#[cfg(feature = "usb")]
mod usb;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_CMD_QUEUE_SIZE: usize = 8_192;

// pub fn start(filter: DeviceFilter) {}

pub type Error = wire_weaver_client::Error;
