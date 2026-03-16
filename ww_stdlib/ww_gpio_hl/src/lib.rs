pub mod asynchronous;
pub mod blocking;
pub mod promise;
mod ww;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("client error: '{:?}'", .0)]
    Client(#[from] wire_weaver_client_common::Error),
    #[error("ww_gpio error: '{:?}'", .0)]
    Gpio(ww_gpio::Error),
    #[error(transparent)]
    Stream(#[from] wire_weaver_client_common::StreamError),
    #[error("expected ww_gpio::Gpio trait, got: '{}'", .0)]
    IncompatibleTrait(String),
    #[error("usage: '{}'", .0)]
    Usage(String),
    #[error("internal: '{}'", .0)]
    Internal(String),
}

impl From<ww_gpio::Error> for Error {
    fn from(e: ww_gpio::Error) -> Self {
        Error::Gpio(e)
    }
}
