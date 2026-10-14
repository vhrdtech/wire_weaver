//! The USB operations PICOBOOT needs, implemented on nusb ([crate::usb]) and by the simulated device ([crate::sim]).

use std::future::Future;

/// A failed USB transfer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    /// The device stalled the endpoint: it refused the command, [PicobootIo::cmd_status] says why.
    #[error("endpoint stalled")]
    Stall,
    #[error("timed out")]
    Timeout,
    #[error("device disconnected")]
    Disconnected,
    #[error("{0}")]
    Other(String),
}

/// Bulk IN / OUT endpoints and the two interface control requests of a PICOBOOT interface.
pub trait PicobootIo {
    /// Send `data` on the bulk OUT endpoint, an empty slice is a zero-length packet.
    fn bulk_out(&mut self, data: &[u8]) -> impl Future<Output = Result<(), IoError>> + Send;

    /// Receive exactly `len` bytes from the bulk IN endpoint, `0` waits for a zero-length packet.
    fn bulk_in(&mut self, len: usize) -> impl Future<Output = Result<Vec<u8>, IoError>> + Send;

    /// `PICOBOOT_IF_RESET`: abort the command in progress, clear stalls on both endpoints.
    fn reset_interface(&mut self) -> impl Future<Output = Result<(), IoError>> + Send;

    /// `PICOBOOT_IF_CMD_STATUS`: the 16-byte status of the last command.
    fn cmd_status(&mut self) -> impl Future<Output = Result<[u8; 16], IoError>> + Send;
}
