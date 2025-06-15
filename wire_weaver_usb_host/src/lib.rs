mod connection;
mod event_loop;
// pub mod util;
mod ww_nusb;

pub use event_loop::usb_worker;
pub use nusb::DeviceInfo;
pub use wire_weaver_client_common;
pub use wire_weaver_usb_link::ProtocolInfo;

use nusb::Error as NusbError;
use nusb::transfer::TransferError;
use std::fmt::Debug;
use tracing::error;
use wire_weaver_usb_link::Error as LinkError;

const IRQ_MAX_PACKET_SIZE: usize = 1024;
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

#[derive(Clone, Debug)]
pub enum UsbDeviceFilter {
    VidPid { vid: u16, pid: u16 },
    VidPidAndSerial { vid: u16, pid: u16, serial: String },
    Serial { serial: String },
    AnyVhrdTechCanBus,
    AnyVhrdTechIo,
}

#[derive(Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connected {
        device_info: DeviceInfo,
    },
    Error {
        error_string: String,
    },
}

/// Shared struct containing connection information along with statistics.
#[derive(Default)]
pub struct ConnectionInfo {
    pub state: ConnectionState, // outstanding streams, requests, etc
    pub worker_running: bool,
}

impl From<NusbError> for UsbError {
    fn from(value: NusbError) -> Self {
        UsbError::Nusb(format!("{:?}", value))
    }
}

impl From<LinkError<TransferError, TransferError>> for UsbError {
    fn from(value: LinkError<TransferError, TransferError>) -> Self {
        UsbError::Link(value)
    }
}

// impl From<wire_weaver::shrink_wrap::Error> for UsbError {
//     fn from(value: wire_weaver::shrink_wrap::Error) -> Self {
//         UsbError::ShrinkWrap(value)
//     }
// }

impl From<&DeviceInfo> for UsbDeviceFilter {
    fn from(info: &DeviceInfo) -> Self {
        if let Some(serial) = info.serial_number() {
            UsbDeviceFilter::VidPidAndSerial {
                vid: info.vendor_id(),
                pid: info.product_id(),
                serial: serial.to_string(),
            }
        } else {
            UsbDeviceFilter::VidPid {
                vid: info.vendor_id(),
                pid: info.product_id(),
            }
        }
    }
}
