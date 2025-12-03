mod connection;
mod event_loop;
// pub mod util;
mod loopback;
mod ww_nusb;

pub use event_loop::usb_worker;
pub use nusb::DeviceInfo;
pub use wire_weaver_client_common;

use nusb::transfer::TransferError;
use std::fmt::Debug;
use tracing::error;

const MAX_MESSAGE_SIZE: usize = 2048;

#[derive(thiserror::Error, Debug, Clone)]
pub enum UsbError {
    #[error("nusb error: {}", .0)]
    Nusb(String),
    #[error("WireWeaverUsbLink error: {:?}", .0)]
    Link(wire_weaver_usb_link::Error<TransferError, TransferError>),
    #[error("nusb::watch_devices() iterator returned None")]
    WatcherReturnedNone,
}

impl From<nusb::Error> for UsbError {
    fn from(value: nusb::Error) -> Self {
        UsbError::Nusb(format!("{:?}", value))
    }
}

impl From<wire_weaver_usb_link::Error<TransferError, TransferError>> for UsbError {
    fn from(value: wire_weaver_usb_link::Error<TransferError, TransferError>) -> Self {
        UsbError::Link(value)
    }
}

impl Into<String> for UsbError {
    fn into(self) -> String {
        format!("{:?}", self)
    }
}
