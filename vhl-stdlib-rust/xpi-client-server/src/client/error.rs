use std::net::AddrParseError;

use thiserror::Error;
use xpi::client_server_owned::Error as XpiError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("")]
    SplitFailed,
    #[error("IP address parse error")]
    AddrParseError(#[from] AddrParseError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("xPI error")]
    XpiError(XpiError),
    #[error("")]
    QueueError,
}
