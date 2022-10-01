use std::net::AddrParseError;
use futures::channel::mpsc::SendError;
use thiserror::Error;
use xpi::error::XpiError;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("Invalid node address")]
    InvalidNodeAddr,
    #[error("IP address parse error")]
    AddrParseError(#[from] AddrParseError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("xPI error")]
    XpiError(#[from] XpiError),
    #[error("Attempted to attach node with same id({}) twice", .0)]
    NodeAlreadyAttached(u32),
    // #[error("Attach failed: {}", .0)]
    // AttachFailed(String),
    #[error("futures::mpsc error")]
    MpscSendError(#[from] SendError),
    #[error("Filter one: waited for rx but got None")]
    FilterOneFail,
    #[error("Expected reply, got {}", .0)]
    ExpectedReply(String),
    #[error("Expected reply with kind: {}, got: {}", .0, .1)]
    ExpectedReplyKind(String, String),
    #[error("Expected different amount of: {}", .0)]
    ExpectedDifferentAmountOf(String),
}