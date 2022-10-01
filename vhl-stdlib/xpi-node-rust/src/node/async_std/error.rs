use std::net::AddrParseError;
use futures::channel::mpsc::SendError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("Invalid node address")]
    InvalidNodeAddr,
    #[error("IP address parse error")]
    AddrParseError(#[from] AddrParseError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("Attempted to attach node with same id({}) twice", .0)]
    NodeAlreadyAttached(u32),
    // #[error("Attach failed: {}", .0)]
    // AttachFailed(String),
    #[error("futures::mpsc error")]
    MpscSendError(#[from] SendError),
    #[error("Filter one: waited for rx but got None")]
    FilterOneFail,
}