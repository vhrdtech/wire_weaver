#![no_std]

//! WireWeaver USB class for embassy-usb: descriptors, packet IO and setup helpers.
//!
//! The event loop is in user code, built from [ww_device::Server] (see its docs), this crate only
//! provides USB specifics. The easiest way to start is [usb_init].

mod config;
mod init;

pub use config::UsbTimings;
use defmt::warn;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
pub use embassy_usb::UsbDevice;
pub use init::{ServerBuffers, UsbBuffers, UsbServer, usb_init};
pub use ww_device::{self, LinkConfig};

use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::msos::windows_version;
use embassy_usb::types::{InterfaceNumber, StringIndex};
use embassy_usb::{Builder, Handler, msos};
use wire_weaver::full_version;
use wire_weaver::prelude::FullVersion;

pub const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xFF;
pub const USB_SUBCLASS_NONE: u8 = 0x00;
pub const USB_PROTOCOL_WIRE_WEAVER: u8 = 0x37;

/// Number of endpoints this crate allocates, can be used to calculate required buffer lengths
pub const ENDPOINTS_USED: usize = 2;

const CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_SELF_ID_VERSION: u8 = 0x40 + 0;
const CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_USER_CRATE: u8 = 0x40 + 1;
const CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_USER_VERSION: u8 = 0x40 + 2;

use wire_weaver::ww_version;
const SELF_VERSION: FullVersion = full_version!();
const USB_DEVICE_CLASS_GUID: &str = "{4987DAA6-F852-4B79-A4C8-8C0E0648C845}";
const DEVICE_INTERFACE_GUIDS: &[&str] = &[USB_DEVICE_CLASS_GUID];

/// Serves the API id interface string, must outlive [WireWeaverClass].
pub struct State {
    api_id_index: Option<StringIndex>,
    api_id: &'static str,
}

impl State {
    pub const fn new() -> Self {
        State {
            api_id_index: None,
            api_id: "",
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for State {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        (Some(index) == self.api_id_index).then_some(self.api_id)
    }
}

/// WireWeaver USB class
pub struct WireWeaverClass<'d, D: Driver<'d>> {
    _data_if: InterfaceNumber,
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
    write_timeout: Duration,
}

impl<'d, D: Driver<'d>> WireWeaverClass<'d, D> {
    /// Adds WireWeaver function and interface to the builder.
    ///
    /// `api_id` is used as the interface string, so that hosts can see which API the device implements
    /// (and optionally its user label) without opening it. Use generated `API_ID` constant from the server module,
    /// or [wire_weaver::api_id::with_label] to add a label to it. See [wire_weaver::api_id] for more details.
    ///
    /// Control buffer passed to the [Builder] must be able to hold the longest string descriptor: `2 + 2 * chars`
    /// bytes plus one, 256 bytes is enough for any string.
    pub fn new(
        builder: &mut Builder<'d, D>,
        state: &'d mut State,
        max_packet_size: u16,
        use_bulk: bool,
        write_timeout: Duration,
        user_protocol: &FullVersion<'_>,
        api_id: &'static str,
    ) -> Self {
        defmt::debug_assert!(builder.control_buf_len() >= 7);
        defmt::assert!(
            builder.control_buf_len() > 2 + 2 * api_id.encode_utf16().count(),
            "control buffer is too small for API id string"
        );
        #[cfg(debug_assertions)]
        check_api_id(api_id, user_protocol);

        // Add MSOS headers so that the device automatically gets assigned the WinUSB driver on Windows.
        //
        // It seems these always need to be at added at the device level for this to work and for
        // composite devices they also need to be added on the function level (as shown later).
        builder.msos_descriptor(windows_version::WIN8_1, 0);
        builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
        builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
            "DeviceInterfaceGUIDs",
            msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
        ));

        let mut func = builder.function(
            USB_CLASS_VENDOR_SPECIFIC,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_WIRE_WEAVER,
        );

        func.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
        func.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
            "DeviceInterfaceGUIDs",
            msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
        ));

        // Data interface
        let mut iface = func.interface();
        let data_if = iface.interface_number();
        let api_id_index = iface.string();
        state.api_id_index = Some(api_id_index);
        state.api_id = api_id;
        let mut alt = iface.alt_setting(
            USB_CLASS_VENDOR_SPECIFIC,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_WIRE_WEAVER,
            Some(api_id_index),
        );
        let (read_ep, write_ep) = if use_bulk {
            let max_packet_size = if max_packet_size > 512 {
                warn!("Bulk max packet size is 512, correcting");
                512
            } else {
                max_packet_size
            };
            (
                alt.endpoint_bulk_out(None, max_packet_size),
                alt.endpoint_bulk_in(None, max_packet_size),
            )
        } else {
            // Should be 2^(interval_ms - 1) 125μs units for High-Speed devices, so 125μs in this case
            // TODO: verify that None as endpoint address here is correct, first available endpoint will be used internally
            (
                alt.endpoint_interrupt_out(None, max_packet_size, 1),
                alt.endpoint_interrupt_in(None, max_packet_size, 1),
            )
        };

        let self_version = [
            b'w',
            b'w',
            b'u',
            b'e',
            u8::try_from(SELF_VERSION.version.major.0).unwrap_or(255),
            u8::try_from(SELF_VERSION.version.minor.0).unwrap_or(255),
            u8::try_from(SELF_VERSION.version.patch.0).unwrap_or(255),
        ];
        alt.descriptor(
            CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_SELF_ID_VERSION,
            &self_version,
        );
        alt.descriptor(
            CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_USER_CRATE,
            user_protocol.crate_id.as_bytes(),
        );
        let user_version = [
            u8::try_from(user_protocol.version.major.0).unwrap_or(255),
            u8::try_from(user_protocol.version.minor.0).unwrap_or(255),
            u8::try_from(user_protocol.version.patch.0).unwrap_or(255),
        ];
        alt.descriptor(
            CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_USER_VERSION,
            &user_version,
        );

        drop(func);
        builder.handler(state);

        WireWeaverClass {
            _data_if: data_if,
            read_ep,
            write_ep,
            write_timeout,
        }
    }

    /// Gets the maximum packet size in bytes.
    pub fn max_packet_size(&self) -> u16 {
        // The size is the same for both endpoints.
        self.read_ep.info().max_packet_size
    }

    /// Writes a single packet into the IN endpoint.
    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        self.write_ep.write(data).await
    }

    /// Reads a single packet from the OUT endpoint.
    pub async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        self.read_ep.read(data).await
    }

    /// Waits for the USB host to enable this interface
    pub async fn wait_connection(&mut self) {
        self.read_ep.wait_enabled().await;
    }

    /// Split the class into a sender and receiver.
    ///
    /// This allows concurrently sending and receiving packets from separate tasks.
    pub fn split(self) -> (Sender<'d, D>, Receiver<'d, D>) {
        (
            Sender {
                write_ep: self.write_ep,
                write_timeout: self.write_timeout,
            },
            Receiver {
                read_ep: self.read_ep,
            },
        )
    }
}

#[cfg(debug_assertions)]
fn check_api_id(api_id: &str, user_protocol: &FullVersion<'_>) {
    match wire_weaver::api_id::parse(api_id) {
        Some(id) => {
            if id.crate_id != user_protocol.crate_id || id.version != user_protocol.version {
                warn!(
                    "API id {} does not match user API {}",
                    api_id, user_protocol.crate_id
                );
            }
        }
        None => warn!("API id {} is malformed", api_id),
    }
}

/// USB raw packet sender.
///
/// You can obtain a `Sender` with [`WireWeaverClass::split`]
pub struct Sender<'d, D: Driver<'d>> {
    write_ep: D::EndpointIn,
    write_timeout: Duration,
}

impl<'d, D: Driver<'d>> Sender<'d, D> {
    /// Gets the maximum packet size in bytes.
    pub fn max_packet_size(&self) -> u16 {
        // The size is the same for both endpoints.
        self.write_ep.info().max_packet_size
    }

    /// Writes a single packet into the IN endpoint.
    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        let write = self.write_ep.write(data);
        let timeout = Timer::after(self.write_timeout);
        match select(write, timeout).await {
            Either::First(r) => r,
            Either::Second(_t) => {
                warn!("USB write timed out, host must have closed the device");
                Err(EndpointError::Disabled)
            }
        }
    }

    /// Waits for the USB host to enable this interface
    pub async fn wait_connection(&mut self) {
        self.write_ep.wait_enabled().await;
    }
}

/// USB raw packet receiver.
///
/// You can obtain a `Receiver` with [`WireWeaverClass::split`]
pub struct Receiver<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
}

impl<'d, D: Driver<'d>> Receiver<'d, D> {
    /// Gets the maximum packet size in bytes.
    pub fn max_packet_size(&self) -> u16 {
        // The size is the same for both endpoints.
        self.read_ep.info().max_packet_size
    }

    /// Reads a single packet from the OUT endpoint.
    /// Must be called with a buffer large enough to hold max_packet_size bytes.
    pub async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        self.read_ep.read(data).await
    }

    /// Waits for the USB host to enable this interface
    pub async fn wait_connection(&mut self) {
        self.read_ep.wait_enabled().await;
    }
}

impl<'d, D: Driver<'d>> ww_device::PacketSink for Sender<'d, D> {
    type Error = EndpointError;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Self::Error> {
        defmt::trace!("usb sending packet {}: {:02x}", packet.len(), packet);
        Sender::write_packet(self, packet).await
    }
}

impl<'d, D: Driver<'d>> ww_device::PacketSource for Receiver<'d, D> {
    type Error = EndpointError;

    fn max_packet_len(&self) -> usize {
        self.max_packet_size() as usize
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let len = self.read_ep.read(buf).await?;
        defmt::trace!("usb received packet {}: {:02x}", len, &buf[..len]);
        Ok(len)
    }

    async fn wait_connected(&mut self) {
        self.read_ep.wait_enabled().await;
    }
}
