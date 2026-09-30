use ww_version::FullVersionOwned;

use crate::DeviceInfo;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Called a method that required event loop to be running")]
    EventLoopNotRunning,
    #[error("RX dispatcher exited due to previous error, cannot operate without it")]
    RxDispatcherNotRunning,
    #[error("No devices found to connect to{}", describe_not_found(.filters, .unmatched))]
    DeviceNotFound {
        /// Human-readable device filters from the config, empty if none
        filters: Vec<String>,
        /// Connected WireWeaver devices that did not pass the filters
        unmatched: Vec<DeviceInfo>,
    },
    #[error("No transport selected in the client config, use .usb() or another one to select it")]
    NoTransportSelected,
    #[error("Failed to connect to {device}: {reason}")]
    ConnectFailed {
        device: Box<DeviceInfo>,
        reason: String,
    },
    #[error("Timeout")]
    // TODO: add timeout name and duration used?
    Timeout,
    #[error("LinkSetup was not received from device after several retries")]
    LinkSetupTimeout,
    #[error("shrink_wrap::Error {:?}", .0)]
    ShrinkWrap(wire_weaver::shrink_wrap::Error),
    #[error("Tried connecting to a device with incompatible protocol")]
    IncompatibleDeviceProtocol,
    #[error("Connected device has an older protocol version: {:?}, required for the operation: {:?}", .0, .1)]
    OlderProtocol(Box<FullVersionOwned>, Box<FullVersionOwned>),
    #[error("'{resource}' is not implemented by the connected device, its API is {device:?}")]
    NotImplementedByDevice {
        /// Resource path, e.g. `gpio[].set_high`
        resource: String,
        device: Box<FullVersionOwned>,
    },
    #[error(
        "'{resource}' is incompatible with the connected device, its API is {device:?}: {reason}"
    )]
    IncompatibleResource {
        /// Resource path, e.g. `gpio[].set_high`
        resource: String,
        device: Box<FullVersionOwned>,
        reason: String,
    },
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
    #[error("No ping from device")]
    NoPingFromDevice,
    #[error("Transport specific error: {}", .0)]
    Transport(String),
    #[error("User error: '{}'", .0)]
    User(String),
    #[error("Other error: '{}'", .0)]
    Other(String),
    #[error("More than one device matched the provided filter:{}", .0.iter().map(|d| format!("\n  {d}")).collect::<String>())]
    AmbiguousDeviceChoice(Vec<DeviceInfo>),
    #[error("Multi request: '{}'", .0)]
    MultiReq(String),
}

fn describe_not_found(filters: &[String], unmatched: &[DeviceInfo]) -> String {
    let mut s = String::new();
    if !filters.is_empty() {
        s += &format!(" matching: {}", filters.join(", "));
    }
    if unmatched.is_empty() {
        if filters.is_empty() {
            s += "\nNo connected device reports a WireWeaver API";
        } else {
            s += "\nNo other WireWeaver devices are connected";
        }
    } else {
        s += "\nConnected WireWeaver devices that did not match:";
        for d in unmatched {
            s += &format!("\n  {d}");
        }
    }
    s
}

impl From<wire_weaver::shrink_wrap::Error> for Error {
    fn from(e: wire_weaver::shrink_wrap::Error) -> Self {
        Error::ShrinkWrap(e)
    }
}

// Configures how to handle connection errors
// #[derive(Copy, Clone, PartialEq, Eq)]
// pub enum OnError {
//     /// Exit immediately with an error if no devices found, or an error occurs.
//     ExitImmediately,
//     /// Keep waiting for a device to appear for timeout.
//     ///
//     /// Might be useful in CLI or automated testing applications, giving user some time to connect a device.
//     RetryFor {
//         timeout: Duration,
//         // subsequent_errors: bool,
//     },
//     /// Keep retrying forever for device to appear and later even if device is disconnected,
//     /// all outstanding streams and requests will be held until reconnection.
//     ///
//     /// Might be useful in dashboard-like applications, that must gracefully handle intermittent loss
//     /// of connection.
//     KeepRetrying,
// }
