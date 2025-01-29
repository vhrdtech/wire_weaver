#![no_std]

use core::cell::RefCell;
use core::future::poll_fn;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;
use embassy_sync::waitqueue::WakerRegistration;
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::types::InterfaceNumber;
use embassy_usb::{msos, Builder, Handler};
use wire_weaver_usb_common::{FrameSink, FrameSource};

pub const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xFF;
pub const USB_SUBCLASS_NONE: u8 = 0x00;
pub const USB_PROTOCOL_WIRE_WEAVER: u8 = 0x37;

/// Internal state for CDC-ACM
pub struct State<'a> {
    control: MaybeUninit<Control<'a>>,
    shared: ControlShared,
}

impl<'a> Default for State<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> State<'a> {
    /// Create a new `State`.
    pub fn new() -> Self {
        Self {
            control: MaybeUninit::uninit(),
            shared: ControlShared::default(),
        }
    }
}

/// Packet level implementation of a CDC-ACM serial port.
///
/// This class can be used directly, and it has the least overhead due to directly reading and
/// writing USB packets with no intermediate buffers, but it will not act like a stream-like serial
/// port. The following constraints must be followed if you use this class directly:
///
/// - `read_packet` must be called with a buffer large enough to hold `max_packet_size` bytes.
/// - `write_packet` must not be called with a buffer larger than `max_packet_size` bytes.
/// - If you write a packet that is exactly `max_packet_size` bytes long, it won't be processed by the
///   host operating system until a subsequent shorter packet is sent. A zero-length packet (ZLP)
///   can be sent if there is no other data to send. This is because USB bulk transactions must be
///   terminated with a short packet, even if the bulk endpoint is used for stream-like data.
pub struct WireWeaverClass<'d, D: Driver<'d>> {
    _comm_ep: D::EndpointIn,
    _data_if: InterfaceNumber,
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
    control: &'d ControlShared,
}

struct Control<'a> {
    comm_if: InterfaceNumber,
    shared: &'a ControlShared,
}

/// Shared data between Control and CdcAcmClass
struct ControlShared {
    // line_coding: CriticalSectionMutex<Cell<LineCoding>>,
    // dtr: AtomicBool,
    waker: RefCell<WakerRegistration>,
    changed: AtomicBool,
}

impl Default for ControlShared {
    fn default() -> Self {
        ControlShared {
            waker: RefCell::new(WakerRegistration::new()),
            changed: AtomicBool::new(false),
        }
    }
}

impl ControlShared {
    async fn changed(&self) {
        poll_fn(|cx| {
            if self.changed.load(Ordering::Relaxed) {
                self.changed.store(false, Ordering::Relaxed);
                Poll::Ready(())
            } else {
                self.waker.borrow_mut().register(cx.waker());
                Poll::Pending
            }
        })
        .await;
    }
}

impl<'a> Control<'a> {
    fn shared(&mut self) -> &'a ControlShared {
        self.shared
    }
}

impl<'d> Handler for Control<'d> {
    fn reset(&mut self) {
        let shared = self.shared();

        shared.changed.store(true, Ordering::Relaxed);
        shared.waker.borrow_mut().wake();
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if (req.request_type, req.recipient, req.index)
            != (
                RequestType::Class,
                Recipient::Interface,
                self.comm_if.0 as u16,
            )
        {
            return None;
        }

        match req.request {
            _ => Some(OutResponse::Rejected),
        }
    }

    fn control_in<'a>(&'a mut self, req: Request, _buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        if (req.request_type, req.recipient, req.index)
            != (
                RequestType::Class,
                Recipient::Interface,
                self.comm_if.0 as u16,
            )
        {
            return None;
        }

        match req.request {
            _ => Some(InResponse::Rejected),
        }
    }
}

impl<'d, D: Driver<'d>> WireWeaverClass<'d, D> {
    /// Creates a new CdcAcmClass with the provided UsbBus and `max_packet_size` in bytes. For
    /// full-speed devices, `max_packet_size` has to be one of 8, 16, 32 or 64.
    pub fn new(
        builder: &mut Builder<'d, D>,
        state: &'d mut State<'d>,
        max_packet_size: u16,
    ) -> Self {
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

        // Control interface: TODO: remove it
        let mut iface = func.interface();
        let comm_if = iface.interface_number();
        let _data_if = u8::from(comm_if) + 1;
        let mut alt = iface.alt_setting(
            USB_CLASS_VENDOR_SPECIFIC,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_WIRE_WEAVER,
            None,
        );

        alt.descriptor(
            // CS_INTERFACE,
            0x24,
            &[
                // CDC_TYPE_HEADER, // bDescriptorSubtype
                0x00, 0x10, 0x01, // bcdCDC (1.10)
            ],
        );

        let comm_ep = alt.endpoint_interrupt_in(8, 255);

        // Data interface
        let mut iface = func.interface();
        let data_if = iface.interface_number();
        let mut alt = iface.alt_setting(
            USB_CLASS_VENDOR_SPECIFIC,
            0x00,
            USB_PROTOCOL_WIRE_WEAVER,
            None,
        );
        // Should be 2^(interval_ms - 1) 125μs units for High-Speed devices, so 125μs in this case
        let read_ep = alt.endpoint_interrupt_out(max_packet_size, 1);
        let write_ep = alt.endpoint_interrupt_in(max_packet_size, 1);

        drop(func);

        let control = state.control.write(Control {
            shared: &state.shared,
            comm_if,
        });
        builder.handler(control);

        let control_shared = &state.shared;

        WireWeaverClass {
            _comm_ep: comm_ep,
            _data_if: data_if,
            read_ep,
            write_ep,
            control: control_shared,
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
    pub fn split_raw(self) -> (Sender<'d, D>, Receiver<'d, D>) {
        (
            Sender {
                write_ep: self.write_ep,
                _control: self.control,
            },
            Receiver {
                read_ep: self.read_ep,
                _control: self.control,
            },
        )
    }

    pub fn split(self) -> (WireWeaverUSBSink<'d, D>, WireWeaverUSBSource<'d, D>) {
        let (sender, receiver) = self.split_raw();
        (WireWeaverUSBSink { sender }, WireWeaverUSBSource { receiver } )
    }
}

/// CDC ACM Control status change monitor
///
/// You can obtain a `ControlChanged` with [`WireWeaverClass::split_with_control`]
pub struct ControlChanged<'d> {
    control: &'d ControlShared,
}

impl<'d> ControlChanged<'d> {
    /// Return a future for when the control settings change
    pub async fn control_changed(&self) {
        self.control.changed().await;
    }
}

/// CDC ACM class packet sender.
///
/// You can obtain a `Sender` with [`WireWeaverClass::split`]
pub struct Sender<'d, D: Driver<'d>> {
    write_ep: D::EndpointIn,
    _control: &'d ControlShared,
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

/// CDC ACM class packet receiver.
///
/// You can obtain a `Receiver` with [`WireWeaverClass::split`]
pub struct Receiver<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
    _control: &'d ControlShared,
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

pub struct WireWeaverUSBSink<'d, D: Driver<'d>> {
    sender: Sender<'d, D>
}

impl<'d, D: Driver<'d>> FrameSink for WireWeaverUSBSink<'d, D> {
    async fn write_frame(&mut self, data: &[u8]) {
        match self.sender.write_packet(data).await {
            Ok(_) => {},
            Err(e) => {
                defmt::error!("Write packet error: {}", e);
            }
        }
    }
}

pub struct WireWeaverUSBSource<'d, D: Driver<'d>> {
    receiver: Receiver<'d, D>,
}

impl<'d, D: Driver<'d>> FrameSource for WireWeaverUSBSource<'d, D> {
    async fn read_frame(&mut self, data: &mut [u8]) -> Option<usize> {
        match self.receiver.read_packet(data).await {
            Ok(len) => Some(len),
            Err(e) => {
                defmt::error!("Read packet error: {}", e);
                Some(0) // TODO: Is it correct to return Some(0)?
            }
        }
    }
}