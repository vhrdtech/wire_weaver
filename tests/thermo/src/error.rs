use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::{CallError, StreamError};

/// Every error of the driver, whatever line the device is on: the per-line error enums fold into it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The call didn't go through: link, timeout, disconnect, another framework error.
    #[error(transparent)]
    Client(wire_weaver_client::Error),
    #[error(transparent)]
    Stream(#[from] StreamError),
    /// `RateError::Busy` of any line.
    #[error("the device is busy with a triggered sample")]
    Busy,
    #[error("self test failed: {0:?}")]
    SelfTest(crate::model::SelfTestError),
    /// The device's firmware doesn't have it: an older revision of its line (`needs` is a wire revision), or a line
    /// without it (`needs` names the lines).
    #[error("{what} is not on this device: it is at wire {device}, {what} needs {needs}")]
    Unsupported {
        what: &'static str,
        device: WireVersion,
        needs: String,
    },
    /// The value has no exact form on the device's line (a gain of 1.5 for a line-1 device that takes whole steps).
    /// Never rounded silently.
    #[error("{what} = {value} can't be sent exactly to a device at wire {device}")]
    NotRepresentable {
        what: &'static str,
        value: String,
        device: WireVersion,
    },
    /// The device sent a variant of an enum newer than this driver's client of its line (an evolvable enum reads it
    /// as `Unknown`).
    #[error("the device sent {what} variant {code}, newer than this driver")]
    UnknownVariant { what: &'static str, code: u32 },
    /// The device's handler for it returned `Unimplemented`.
    #[error("{0} is not implemented by the device")]
    Unimplemented(String),
}

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
    /// Names the item of an [Error::Unsupported] made from the client's `OlderProtocol`.
    pub(crate) fn of(self, item: &'static str) -> Self {
        match self {
            Error::Unsupported { device, needs, .. } => Error::Unsupported {
                what: item,
                device,
                needs,
            },
            e => e,
        }
    }
}

/// A call to an item newer than the device's revision is refused by the client before it is sent
/// (`OlderProtocol`): [Error::Unsupported], since the facade's caller asks for a feature, not for a wire revision.
impl From<wire_weaver_client::Error> for Error {
    fn from(e: wire_weaver_client::Error) -> Self {
        match e {
            wire_weaver_client::Error::OlderProtocol {
                device_wire,
                required,
                ..
            } => Error::Unsupported {
                what: "",
                device: device_wire,
                needs: required.to_string(),
            },
            e => Error::Client(e),
        }
    }
}

/// An operation a line doesn't write (`wire_weaver_client::facade!`'s default).
impl From<wire_weaver_client::facade::Unsupported> for Error {
    fn from(e: wire_weaver_client::facade::Unsupported) -> Self {
        Error::Unsupported {
            what: e.what,
            device: e.device,
            needs: e.needs.into(),
        }
    }
}

/// A device's error of one line, before it is folded into [Error]: the per-line modules implement `From` for their
/// error enums.
pub struct DeviceError<E>(pub E);

impl<E> From<CallError<E>> for Error
where
    Error: From<DeviceError<E>>,
{
    fn from(e: CallError<E>) -> Self {
        match e {
            CallError::Device(e) => DeviceError(e).into(),
            CallError::Unimplemented { resource } => Error::Unimplemented(resource),
            CallError::Client(e) => e.into(),
        }
    }
}
