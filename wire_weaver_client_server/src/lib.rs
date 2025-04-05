pub mod event_loop_state;
pub mod util;
pub mod ww;

use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::nib16::Nib16;

pub enum Command<F, E> {
    /// Try to connect to / open a device with the specified filter.
    Connect {
        filter: F,
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<(), Error<E>>>>,
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

    SendCall {
        // WireWeaver client_server serialized Request, this shifts serializing onto caller and allows to reuse Vec
        args_bytes: Vec<u8>,
        path: Vec<Nib16>,
        timeout: Option<Duration>,
        done_tx: Option<oneshot::Sender<Result<Vec<u8>, Error<E>>>>,
    },
    SendWrite {
        value_bytes: Vec<u8>,
        path: Vec<Nib16>,
        timeout: Option<Duration>,
        // Vec is always empty here, but allows for common code
        done_tx: Option<oneshot::Sender<Result<Vec<u8>, Error<E>>>>,
    },
    SendRead {
        path: Vec<Nib16>,
        timeout: Option<Duration>,
        done_tx: Option<oneshot::Sender<Result<Vec<u8>, Error<E>>>>,
    },
    Subscribe {
        path: Vec<u16>,
        stream_data_tx: mpsc::UnboundedSender<Result<Vec<u8>, Error<E>>>,
        // stop_rx: oneshot::Receiver<()>,
    },
    // RecycleBuffer(Vec<u8>),
}

pub type SeqTy = u16;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(thiserror::Error, Debug)]
pub enum Error<E> {
    #[error("Called a method that required event loop to be running")]
    EventLoopNotRunning,
    #[error("Timeout")]
    Timeout,
    #[error("LinkSetup was not received from device after several retries")]
    LinkSetupTimeout,
    #[error("ShrinkWrap error {:?}", .0)]
    ShrinkWrap(wire_weaver::shrink_wrap::Error),
    #[error("Tried connecting to a device with incompatible protocol")]
    IncompatibleDeviceProtocol,
    #[error("Submitted a command requiring active connection, when there was none")]
    Disconnected,
    #[error("Device returned WireWeaver client_server error: {:?}", .0)]
    RemoteError(ww::no_alloc_client::client_server_v0_1::Error),
    #[error("Failed to deserialize a bytes slice from device response")]
    ByteSliceReadFailed,

    #[error("Transport specific error")]
    Transport(E),
}

impl<E> From<wire_weaver::shrink_wrap::Error> for Error<E> {
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
