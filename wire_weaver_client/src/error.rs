use std::fmt::{Debug, Display, Formatter};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::ErrorKindOwned;
use ww_version::FullVersionOwned;

use crate::DeviceInfo;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Called a method that required event loop to be running")]
    EventLoopNotRunning,
    #[error("RX dispatcher exited due to previous error, cannot operate without it")]
    RxDispatcherNotRunning,
    #[error("No devices found to connect to{}", describe_not_found(.filters, .unmatched, *.over_rtt))]
    DeviceNotFound {
        /// Human-readable device filters from the config, empty if none
        filters: Vec<String>,
        /// Connected WireWeaver devices that did not pass the filters (debug probes over RTT)
        unmatched: Vec<DeviceInfo>,
        /// The config connects over RTT: no matching debug probe was found
        over_rtt: bool,
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
    #[error("The device speaks link {device}, this host link {host}: update the older side")]
    IncompatibleLinkVersion { device: String, host: String },
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
    #[error("Remote device returned {}", .0)]
    RemoteError(ww_client_server::ErrorOwned),
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
    #[error(
        "Connected with `traits_only()`, only global trait calls are possible, not absolute path '{}'", .0
    )]
    AbsolutePathInTraitsOnly(String),
    /// The ww hub (or another process serving the local socket) answered the preamble with a status but `Ok`.
    #[error("{target}: {why} (ww hub {hub_version})")]
    HubRefused {
        /// The socket and device asked for
        target: String,
        /// The preamble answer's status (`ww_local_socket::Status`): 1 unknown device, 2 held elsewhere, 3 preamble
        /// version, 4 versions not served, 5 lent
        status: u8,
        /// What it means, with the fix where there is one
        why: String,
        hub_version: String,
    },
    #[error("{what} is not supported by the connected device: {reason}")]
    Unsupported {
        /// What was tried, e.g. `observing properties`
        what: &'static str,
        reason: String,
    },
}

impl Error {
    /// A ww hub that lent the device to another program (SRV-32): it comes back, wait and try again.
    pub fn is_lent(&self) -> bool {
        matches!(self, Error::HubRefused { status, .. } if *status == crate::connection_event::STATUS_LENT)
    }
}

fn describe_not_found(filters: &[String], unmatched: &[DeviceInfo], over_rtt: bool) -> String {
    let mut s = String::new();
    if !filters.is_empty() {
        s += &format!(" matching: {}", filters.join(", "));
    }
    if over_rtt {
        if unmatched.is_empty() {
            s += "\nNo debug probe found";
        } else {
            s += "\nDebug probes that did not match:";
            for d in unmatched {
                s += &format!("\n  {d}");
            }
        }
        return s;
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

impl Error {
    /// The device has no implementation of the called item: its handler returned `Unimplemented`
    /// (`RpcResult`, `GetResult`, `SetResult`), or the device's API doesn't have it ([Error::NotImplementedByDevice],
    /// known locally from introspection). For items without an error type, items with one return
    /// [CallError::Unimplemented] instead.
    pub fn is_unimplemented(&self) -> bool {
        match self {
            Error::NotImplementedByDevice { .. } => true,
            Error::RemoteError(e) => matches!(e.kind, ErrorKindOwned::Unimplemented),
            _ => false,
        }
    }
}

/// Error of a call to a method returning `Result<T, E>`, or of a write to a property with an error type.
///
/// `call()`, `blocking_call()`, `write()` and the promises of such items return `Result<T, CallError<E>>`,
/// so one `?` is enough, and a match tells the three outcomes apart.
///
/// `Display` and [source](std::error::Error::source) are transparent: the inner error's text, once.
#[derive(Debug)]
pub enum CallError<E> {
    /// The device answered with its error: the `Err(e)` of the method (or of the property write).
    Device(E),
    /// The device has no implementation: its handler returned `Unimplemented` (`RpcResult`, `GetResult`,
    /// `SetResult`), or the device's API doesn't have it (known locally from introspection, no round trip).
    Unimplemented {
        /// Resource name, e.g. `Gpio::set_mode` or `gpio[].set_mode`
        resource: String,
    },
    /// The call didn't go through: link, timeout, disconnect, another framework error.
    Client(Error),
}

impl<E> CallError<E> {
    /// Returns the device's error, if the device answered with one.
    pub fn device(&self) -> Option<&E> {
        match self {
            CallError::Device(e) => Some(e),
            _ => None,
        }
    }

    /// See [CallError::Unimplemented].
    pub fn is_unimplemented(&self) -> bool {
        matches!(self, CallError::Unimplemented { .. })
    }
}

impl<E: Display> Display for CallError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Device(e) => Display::fmt(e, f),
            CallError::Unimplemented { resource } => {
                write!(f, "'{resource}' is not implemented by the device")
            }
            CallError::Client(e) => Display::fmt(e, f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CallError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CallError::Device(e) => e.source(),
            CallError::Unimplemented { .. } => None,
            CallError::Client(e) => e.source(),
        }
    }
}

/// Error type of a call, read or write outcome: [Error] for items without an error type, [CallError] for items with one.
/// Implemented by generated client code's error types only, not meant to be implemented by users.
pub trait OutcomeError: Debug + Sized {
    /// Wraps an error of the call itself; `resource` names the called item for [CallError::Unimplemented].
    fn from_client(e: Error, resource: &'static str) -> Self;
}

/// Decodes a reply to `T` or the error it carries, see [OutcomeError].
pub trait DecodeReply<T>: OutcomeError {
    fn decode_reply(bytes: &[u8]) -> Result<T, Self>;
}

impl OutcomeError for Error {
    fn from_client(e: Error, _resource: &'static str) -> Self {
        e
    }
}

impl<T: DeserializeShrinkWrapOwned> DecodeReply<T> for Error {
    fn decode_reply(bytes: &[u8]) -> Result<T, Self> {
        Ok(T::from_ww_bytes_owned(bytes)?)
    }
}

impl<E: DeserializeShrinkWrapOwned + Debug> OutcomeError for CallError<E> {
    fn from_client(e: Error, resource: &'static str) -> Self {
        match e {
            Error::NotImplementedByDevice { resource, .. } => CallError::Unimplemented { resource },
            Error::RemoteError(remote) => match &remote.kind {
                ErrorKindOwned::Unimplemented => CallError::Unimplemented {
                    resource: resource.into(),
                },
                // a property write's user error
                ErrorKindOwned::UserBytes(bytes) => match E::from_ww_bytes_owned(bytes) {
                    Ok(e) => CallError::Device(e),
                    Err(e) => CallError::Client(e.into()),
                },
                _ => CallError::Client(Error::RemoteError(remote)),
            },
            e => CallError::Client(e),
        }
    }
}

impl<T, E> DecodeReply<T> for CallError<E>
where
    T: DeserializeShrinkWrapOwned,
    E: DeserializeShrinkWrapOwned + Debug,
{
    fn decode_reply(bytes: &[u8]) -> Result<T, Self> {
        match Result::<T, E>::from_ww_bytes_owned(bytes) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(CallError::Device(e)),
            Err(e) => Err(CallError::Client(e.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::prelude::SerializeShrinkWrap;

    fn remote(kind: ww_client_server::ErrorKind<'_>) -> Error {
        Error::RemoteError(ww_client_server::Error::new(1, kind).make_owned())
    }

    #[test]
    fn unimplemented_from_device_and_compat() {
        type E = CallError<u8>;
        let e = E::from_client(remote(ww_client_server::ErrorKind::Unimplemented), "T::m");
        assert!(matches!(&e, CallError::Unimplemented { resource } if resource == "T::m"));
        assert!(remote(ww_client_server::ErrorKind::Unimplemented).is_unimplemented());

        let not_on_device = || Error::NotImplementedByDevice {
            resource: "t.m".into(),
            device: Box::new(FullVersionOwned::new(
                "d".into(),
                ww_version::VersionOwned::new(0, 1, 0),
            )),
        };
        assert!(not_on_device().is_unimplemented());
        let e = E::from_client(not_on_device(), "T::m");
        assert!(matches!(&e, CallError::Unimplemented { resource } if resource == "t.m"));

        let e = E::from_client(Error::Timeout, "T::m");
        assert!(matches!(e, CallError::Client(Error::Timeout)));
        assert!(!Error::Timeout.is_unimplemented());
    }

    #[test]
    fn user_bytes_of_a_write_are_the_device_error() {
        let mut buf = [0u8; 8];
        let bytes = 7u8.to_ww_bytes(&mut buf).unwrap();
        let kind = ww_client_server::ErrorKind::UserBytes(
            wire_weaver::shrink_wrap::RefVec::new_bytes(bytes),
        );
        let e = CallError::<u8>::from_client(remote(kind), "T::p");
        assert!(matches!(e, CallError::Device(7)), "{e:?}");
    }

    #[test]
    fn decode_reply_splits_the_result() {
        let mut buf = [0u8; 8];
        let ok = Result::<u8, u8>::Ok(3)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        assert!(matches!(CallError::<u8>::decode_reply(&ok), Ok(3u8)));
        let err = Result::<u8, u8>::Err(4)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        assert!(matches!(
            <CallError<u8> as DecodeReply<u8>>::decode_reply(&err),
            Err(CallError::Device(4))
        ));
    }

    #[test]
    fn display_is_transparent() {
        let e = CallError::<std::fmt::Error>::Client(Error::Timeout);
        assert_eq!(e.to_string(), "Timeout");
        let e = CallError::<u8>::Unimplemented {
            resource: "T::m".into(),
        };
        assert_eq!(e.to_string(), "'T::m' is not implemented by the device");
    }
}
