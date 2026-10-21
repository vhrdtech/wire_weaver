//! The USB operations DFU needs, implemented on nusb ([crate::usb]) and by the simulated device ([crate::sim]).

use std::future::Future;

/// A failed USB transfer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    /// The device stalled the control request.
    #[error("request stalled")]
    Stall,
    #[error("timed out")]
    Timeout,
    #[error("device disconnected")]
    Disconnected,
    #[error("{0}")]
    Other(String),
}

/// Class requests on the DFU interface, and what the descriptors say about the selected alternate setting.
pub trait DfuIo {
    /// Class request to the interface, host to device.
    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        data: &[u8],
    ) -> impl Future<Output = Result<(), IoError>> + Send;

    /// Class request to the interface, device to host, at most `len` bytes come back.
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        len: usize,
    ) -> impl Future<Output = Result<Vec<u8>, IoError>> + Send;

    /// The interface string of the selected alternate setting, the memory layout (`@Internal Flash /0x08000000/...`).
    fn layout_string(&self) -> &str;

    /// `wTransferSize` of the DFU functional descriptor: the largest data block.
    fn transfer_size(&self) -> usize;
}
