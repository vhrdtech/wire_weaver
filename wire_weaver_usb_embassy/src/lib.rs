#![no_std]

use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::types::InterfaceNumber;
use embassy_usb::{msos, Builder};
use wire_weaver_usb_link::{PacketSink, PacketSource};

pub const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xFF;
pub const USB_SUBCLASS_NONE: u8 = 0x00;
pub const USB_PROTOCOL_WIRE_WEAVER: u8 = 0x37;

/// WireWeaver USB class
pub struct WireWeaverClass<'d, D: Driver<'d>> {
    _data_if: InterfaceNumber,
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
}

impl<'d, D: Driver<'d>> WireWeaverClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, max_packet_size: u16) -> Self {
        assert!(builder.control_buf_len() >= 7);

        let mut func = builder.function(
            USB_CLASS_VENDOR_SPECIFIC,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_WIRE_WEAVER,
        );

        const USB_DEVICE_CLASS_GUID: &str = "{4987DAA6-F852-4B79-A4C8-8C0E0648C845}";
        const DEVICE_INTERFACE_GUIDS: &[&str] = &[USB_DEVICE_CLASS_GUID];
        func.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
        func.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
            "DeviceInterfaceGUIDs",
            msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
        ));

        // Data interface
        let mut iface = func.interface();
        let data_if = iface.interface_number();
        let mut alt = iface.alt_setting(
            USB_CLASS_VENDOR_SPECIFIC,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_WIRE_WEAVER,
            None,
        );
        // Should be 2^(interval_ms - 1) 125μs units for High-Speed devices, so 125μs in this case
        // TODO: verify that None as endpoint address here is correct, first available endpoint will be used internally
        let read_ep = alt.endpoint_interrupt_out(None, max_packet_size, 1);
        let write_ep = alt.endpoint_interrupt_in(None, max_packet_size, 1);

        drop(func);

        WireWeaverClass {
            _data_if: data_if,
            read_ep,
            write_ep,
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
            },
            Receiver {
                read_ep: self.read_ep,
            },
        )
    }
}

/// WireWeaver raw packet sender.
///
/// You can obtain a `Sender` with [`WireWeaverClass::split`]
pub struct Sender<'d, D: Driver<'d>> {
    write_ep: D::EndpointIn,
}

impl<'d, D: Driver<'d>> Sender<'d, D> {
    /// Gets the maximum packet size in bytes.
    pub fn max_packet_size(&self) -> u16 {
        // The size is the same for both endpoints.
        self.write_ep.info().max_packet_size
    }

    /// Writes a single packet into the IN endpoint.
    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        self.write_ep.write(data).await
    }

    /// Waits for the USB host to enable this interface
    pub async fn wait_connection(&mut self) {
        self.write_ep.wait_enabled().await;
    }
}

/// WireWeaver raw packet receiver.
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

impl<'d, D: Driver<'d>> PacketSink for Sender<'d, D> {
    type Error = EndpointError;

    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        defmt::trace!("usb sending packet {:02x}", data);
        self.write_ep.write(data).await
    }
}

impl<'d, D: Driver<'d>> PacketSource for Receiver<'d, D> {
    type Error = EndpointError;

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
        let len = self.read_ep.read(data).await?;
        defmt::trace!("usb received packet {:02x}", &data[..len]);
        Ok(len)
    }

    async fn wait_usb_connection(&mut self) {
        self.read_ep.wait_enabled().await;
    }
}
