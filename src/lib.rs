mod connection;
mod event_loop;
mod ww;
mod ww_nusb;

pub use event_loop::usb_worker;

use nusb::{DeviceInfo, Error as NusbError};
use std::fmt::Debug;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::error;
use wire_weaver::shrink_wrap::nib16::Nib16;
use wire_weaver_usb_link::Error as LinkError;

const IRQ_MAX_PACKET_SIZE: usize = 1024;
const MAX_MESSAGE_SIZE: usize = 2048;
type SeqTy = u16;

pub enum Command {
    /// Try to open device with the specified filter.
    Connect {
        filter: UsbDeviceFilter,
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

    SendCall {
        // WireWeaver client_server serialized Request, this shifts serializing onto caller and allows to reuse Vec
        args_bytes: Vec<u8>,
        path: Vec<Nib16>,
        timeout: Option<Duration>,
        done_tx: Option<oneshot::Sender<Result<Vec<u8>, Error>>>,
    },
    // SendFrame {
    //     frame: (),
    //     done_tx: oneshot::Sender<()>,
    // },
    // ReceiveFrame {
    //     id: (),
    //     done_tx: oneshot::Sender<()>,
    // },
    Subscribe {
        path: Vec<u16>,
        stream_data_tx: mpsc::UnboundedSender<Result<Vec<u8>, Error>>,
        // stop_rx: oneshot::Receiver<()>,
    },
    // RecycleBuffer(Vec<u8>),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Nusb(NusbError),
    #[error("WireWeaverUsbLink error: {}", .0)]
    Link(String),
    #[error("Timeout")]
    Timeout,
    #[error("nusb::watch_devices() iterator returned None")]
    WatcherReturnedNone,
    #[error("ShrinkWrap error {:?}", .0)]
    ShrinkWrap(wire_weaver::shrink_wrap::Error),
    #[error("LinkSetup was not received from device after several retries")]
    LinkSetupTimeout,
    #[error("Tried connecting to a device with incompatible protocol")]
    IncompatibleDeviceProtocol,
    #[error("Submitted a command requiring active connection, when there was none")]
    Disconnected,
    #[error("Device returned WireWeaver client_server error: {:?}", .0)]
    RemoteError(ww::no_alloc_client::client_server_v0_1::Error),
    #[error("Failed to deserialize a bytes slice from device response")]
    ByteSliceReadFailed,
}

pub enum UsbDeviceFilter {
    VidPid { vid: u16, pid: u16 },
    VidPidAndSerial { vid: u16, pid: u16, serial: String },
    Serial { serial: String },
    AnyVhrdTechCanBus,
}

#[derive(Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected {
        device_info: DeviceInfo,
    },
    Error {
        error_string: String,
    },
}

/// Shared struct containing connection information along with statistics.
#[derive(Default)]
pub struct ConnectionInfo {
    pub state: ConnectionState, // outstanding streams, requests, etc
    pub worker_running: bool,
}

impl From<NusbError> for Error {
    fn from(value: NusbError) -> Self {
        Error::Nusb(value)
    }
}

impl<T: Debug, R: Debug> From<LinkError<T, R>> for Error {
    fn from(value: LinkError<T, R>) -> Self {
        Error::Link(format!("{:?}", value))
    }
}

impl From<wire_weaver::shrink_wrap::Error> for Error {
    fn from(value: wire_weaver::shrink_wrap::Error) -> Self {
        Error::ShrinkWrap(value)
    }
}

impl From<&DeviceInfo> for UsbDeviceFilter {
    fn from(info: &DeviceInfo) -> Self {
        if let Some(serial) = info.serial_number() {
            UsbDeviceFilter::VidPidAndSerial {
                vid: info.vendor_id(),
                pid: info.product_id(),
                serial: serial.to_string(),
            }
        } else {
            UsbDeviceFilter::VidPid {
                vid: info.vendor_id(),
                pid: info.product_id(),
            }
        }
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
