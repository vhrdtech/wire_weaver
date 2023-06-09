use std::net::AddrParseError;

use thiserror::Error;
use xpi::error::XpiError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("")]
    SplitFailed,
    #[error("IP address parse error")]
    AddrParseError(#[from] AddrParseError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("xPI error")]
    XpiError(#[from] XpiError),
}
