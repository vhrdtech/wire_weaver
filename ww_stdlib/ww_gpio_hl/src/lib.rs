pub mod asynchronous;
pub mod blocking;
pub mod promise;
mod ww;
pub use ww_gpio;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("client error: '{:?}'", .0)]
    Client(#[from] wire_weaver_client::Error),
    #[error("ww_gpio: {0}")]
    Gpio(ww_gpio::GpioErrorOwned),
    #[error("'{resource}' is not implemented by the device")]
    Unimplemented { resource: String },
    #[error(transparent)]
    Stream(#[from] wire_weaver_client::StreamError),
    #[error("expected ww_gpio::Gpio trait, got: '{}'", .0)]
    IncompatibleTrait(String),
    #[error("usage: '{}'", .0)]
    Usage(String),
    #[error("internal: '{}'", .0)]
    Internal(String),
}

impl From<ww_gpio::GpioErrorOwned> for Error {
    fn from(e: ww_gpio::GpioErrorOwned) -> Self {
        Error::Gpio(e)
    }
}

impl From<wire_weaver_client::CallError<ww_gpio::GpioErrorOwned>> for Error {
    fn from(e: wire_weaver_client::CallError<ww_gpio::GpioErrorOwned>) -> Self {
        use wire_weaver_client::CallError;
        match e {
            CallError::Device(e) => Error::Gpio(e),
            CallError::Unimplemented { resource } => Error::Unimplemented { resource },
            CallError::Client(e) => Error::Client(e),
        }
    }
}

impl embedded_hal::digital::Error for Error {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}
