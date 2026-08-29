pub use wire_weaver_client_common;

#[cfg(feature = "usb")]
pub use wire_weaver_usb_host;

#[cfg(feature = "net")]
pub use wire_weaver_net_host;

pub mod device_info;
pub mod options;
mod prepared_connection;

pub use prepared_connection::PreparedConnection;

#[cfg(feature = "usb")]
mod usb;

use wire_weaver_client_common::DeviceFilter;

pub fn start(filter: DeviceFilter) {}
