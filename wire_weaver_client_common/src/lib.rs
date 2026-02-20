pub mod attachment;
mod command;
pub mod command_sender;
pub mod device_filter;
pub mod event_loop_state;
mod introspect;
mod prepared_call;
mod prepared_read;
mod prepared_write;
pub mod promise;
pub mod rx_dispatcher;
mod sink;
pub mod stream;
mod tracing;
pub mod ww;

// TODO: remove
pub use attachment::Attachment;
pub use command::{Command, DeviceInfoBundle, TestProgress};
pub use command_sender::CommandSender;
pub use device_filter::DeviceFilter;
pub use prepared_call::PreparedCall;
pub use prepared_read::PreparedRead;
pub use prepared_write::PreparedWrite;
pub use sink::Sink;
pub use stream::{Stream, StreamError};
pub use ww_client_server;
pub use ww_self;
pub use ww_version;

use std::time::Duration;
use ww_client_server::StreamSidebandEvent;

pub type SeqTy = u16;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Called a method that required event loop to be running")]
    EventLoopNotRunning,
    #[error("RX dispatcher exited due to previous error, cannot operate without it")]
    RxDispatcherNotRunning,
    #[error("No devices found to connect to")]
    DeviceNotFound,
    #[error("Timeout")]
    Timeout,
    #[error("LinkSetup was not received from device after several retries")]
    LinkSetupTimeout,
    #[error("shrink_wrap::Error {:?}", .0)]
    ShrinkWrap(wire_weaver::shrink_wrap::Error),
    #[error("Tried connecting to a device with incompatible protocol")]
    IncompatibleDeviceProtocol,
    #[error("Submitted a command requiring active connection, when there was none")]
    Disconnected,
    #[error("Remote device returned ww_client_server::{:?}", .0)]
    RemoteError(ww_client_server::ErrorOwned),
    #[error("Remote device returned {}", .0)]
    RemoteErrorDes(String),
    #[error("All command senders were dropped")]
    CmdTxDropped,
    #[error("Exit command received")]
    ExitRequested,
    #[error("Transport specific error: {}", .0)]
    Transport(String),
    #[error("User error: '{}'", .0)]
    User(String),
    #[error("Other error: '{}'", .0)]
    Other(String),
}

impl From<wire_weaver::shrink_wrap::Error> for Error {
    fn from(e: wire_weaver::shrink_wrap::Error) -> Self {
        Error::ShrinkWrap(e)
    }
}

/// Configures how to handle connection errors
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum OnError {
    /// Exit immediately with an error if no devices found, or an error occurs.
    ExitImmediately,
    /// Keep waiting for a device to appear for timeout.
    ///
    /// Might be useful in CLI or automated testing applications, giving user some time to connect a device.
    RetryFor {
        timeout: Duration,
        // subsequent_errors: bool,
    },
    /// Keep retrying forever for device to appear and later even if device is disconnected,
    /// all outstanding streams and requests will be held until reconnection.
    ///
    /// Might be useful in dashboard-like applications, that must gracefully handle intermittent loss
    /// of connection.
    KeepRetrying,
}

impl OnError {
    pub fn retry_for_secs(secs: u64) -> Self {
        Self::RetryFor {
            timeout: Duration::from_secs(secs),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum StreamEvent {
    /// Data channel from remote device
    Data(Vec<u8>),
    /// Sideband channel from remote device
    Sideband(StreamSidebandEvent),
    /// Locally generated event, sent when connection to remote device is up
    Connected,
    /// Locally generated event, sent when connection to remote device is down
    Disconnected,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TypedStreamEvent<T> {
    /// Data channel from remote device
    Data(T),
    /// Sideband channel from remote device
    Sideband(StreamSidebandEvent),
    /// Locally generated event, sent when connection to remote device is up
    Connected,
    /// Locally generated event, sent when connection to remote device is down
    Disconnected,
}
