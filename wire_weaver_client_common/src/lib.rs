pub mod attachment;
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
pub mod ww;

// TODO: remove
pub use attachment::Attachment;
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
use ww_version::FullVersionOwned;

use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use ww_client_server::StreamSidebandEvent;

/// Command for the transport event loop host (USB host, WebSocket client, UDP client).
/// Generated client code uses [CommandSender](CommandSender), which sends out Command's.
pub enum Command {
    /// Try to connect to / open a device with the specified filter.
    Connect {
        filter: DeviceFilter,
        user_protocol_version: FullVersionOwned,
        // TODO: supported_use_protocols: Vec<FullVersion<'static>> and keep only the one in common
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
    },

    /// Complete outstanding requests (but ignore new ones)? Then close device connection, but keep worker task running.
    /// This allows all the outstanding streams to still be valid and continue upon reconnection.
    /// Alternatively it's also possible to connect to a different device, without other parts noticing.
    DisconnectKeepStreams {
        disconnected_tx: Option<oneshot::Sender<()>>,
    },
    /// Close device connection and stop worker task. All outstanding requests will return with Error,
    /// and streams will stop. Use when shutting down whole app.
    DisconnectAndExit {
        disconnected_tx: Option<oneshot::Sender<()>>,
    },

    SendMessage {
        bytes: Vec<u8>,
    },
    // RecycleBuffer(Vec<u8>),
    // GetStats,
    LoopbackTest {
        test_duration: Duration,
        packet_size: Option<usize>,
        progress_tx: mpsc::UnboundedSender<TestProgress>,
    },
}

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

impl Command {
    pub fn disconnect_and_exit() -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::DisconnectAndExit {
            disconnected_tx: Some(tx),
        };
        (cmd, rx)
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

#[derive(Debug)]
pub enum TestProgress {
    TestStarted(&'static str),
    Completion(&'static str, f32),
    TestCompleted(&'static str),
    FatalError(String),
    LoopbackReport {
        tx_count: u64,
        per_s: f32,
        lost_count: u64,
        data_corrupted_count: u64,
    },
    SpeedReport {
        name: &'static str,
        count: u64,
        per_s: f32,
        bytes_per_s: f32,
    },
}
