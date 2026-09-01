use ww_version::FullVersionOwned;

use crate::DeviceInfo;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Called a method that required event loop to be running")]
    EventLoopNotRunning,
    #[error("RX dispatcher exited due to previous error, cannot operate without it")]
    RxDispatcherNotRunning,
    #[error("No devices found to connect to")]
    DeviceNotFound,
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
    #[error("More than one devices matched the provided filter: {:#?}", .0)]
    AmbiguousDeviceChoice(Vec<DeviceInfo>),
    #[error("Multi request: '{}'", .0)]
    MultiReq(String),
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
