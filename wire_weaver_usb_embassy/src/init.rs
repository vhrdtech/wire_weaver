use crate::{UsbTimings, WireWeaverClass};
use defmt::{info, trace};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Config, UsbDevice};
use wire_weaver::{WireWeaverAsyncApiBackend, ww_version::FullVersion};
use wire_weaver_usb_link::WireWeaverUsbLink;

pub struct UsbServer<'d, D: Driver<'d>, B> {
    pub(crate) usb: UsbDevice<'d, D>,
    pub(crate) link: WireWeaverUsbLink<'d, super::Sender<'d, D>, super::Receiver<'d, D>>,
    pub(crate) call_publish_rx: Receiver<'d, CriticalSectionRawMutex, (), 1>,
    pub(crate) state: B,
    pub(crate) timings: UsbTimings,
    pub(crate) rx_message: &'d mut [u8],
    pub(crate) scratch_args: &'d mut [u8],
    pub(crate) scratch_event: &'d mut [u8],
}

pub struct UsbBuffers<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> {
    // buffer_usage() can be used to tune these
    config_descriptor: [u8; 96],
    bos_descriptor: [u8; 40],
    msos_descriptor: [u8; 330],
    control: [u8; 64],
    /// Used to receive USB packets
    rx: [u8; MAX_USB_PACKET_LEN],
    /// Used to assemble frames from multiple USB packets
    rx_message: [u8; MAX_MESSAGE_LEN],
    /// Used to prepare USB packets for transmission
    tx: [u8; MAX_USB_PACKET_LEN],
    /// Used to serialize arguments of methods
    scratch_args: [u8; MAX_MESSAGE_LEN],
    /// Used to serialize final event out of arguments and other pieces
    scratch_event: [u8; MAX_MESSAGE_LEN],
    call_publish: Channel<CriticalSectionRawMutex, (), 1>,
}

impl<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> Default
    for UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>
{
    fn default() -> Self {
        UsbBuffers {
            config_descriptor: [0u8; 96],
            bos_descriptor: [0u8; 40],
            msos_descriptor: [0u8; 330],
            control: [0u8; 64],
            rx: [0u8; MAX_USB_PACKET_LEN],
            rx_message: [0u8; MAX_MESSAGE_LEN],
            tx: [0u8; MAX_USB_PACKET_LEN],
            scratch_args: [0u8; MAX_MESSAGE_LEN],
            scratch_event: [0u8; MAX_MESSAGE_LEN],
            call_publish: Channel::new(),
        }
    }
}

/// Initializes USB stack with default configuration and a single interface with WireWeaver class.
/// Device should work without drivers in Linux, macOS and Windows.
///
/// This functions is a convenient way to initialize a minimum working device, if you need more advanced setup,
/// you can do the same steps directly and extend accordingly.
///
/// It is recommended to adjust USB config in the config_mut closure, in particular:
/// * Set vid, pid (default is 0xc0de:0xcafe)
/// * Set manufacturer and product (default is "Vhrd.Tech" "WireWeaver Generic")
/// * Set serial_number (default is None, use e.g., embassy_stm32::uid::uid_hex())
/// * max_power (default is 100mA)
/// * self_powered (default is false)
pub fn usb_init<
    'd,
    const MAX_USB_PACKET_LEN: usize,
    const MAX_MESSAGE_LEN: usize,
    D: Driver<'d>,
    C: FnOnce(&mut Config),
    B: WireWeaverAsyncApiBackend,
>(
    driver: D,
    buffers: &'d mut UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
    state: B,
    timings: UsbTimings,
    user_protocol: FullVersion<'static>,
    config_mut: C,
) -> (
    UsbServer<'d, D, B>,
    Sender<'d, CriticalSectionRawMutex, (), 1>,
) {
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Vhrd.Tech");
    config.product = Some("WireWeaver Generic");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config.max_power = 100;
    config.self_powered = false;
    config_mut(&mut config);

    let mut builder = Builder::new(
        driver,
        config,
        &mut buffers.config_descriptor,
        &mut buffers.bos_descriptor,
        &mut buffers.msos_descriptor,
        &mut buffers.control,
    );

    // Create class on the builder.
    let ww = WireWeaverClass::new(
        &mut builder,
        MAX_USB_PACKET_LEN as u16,
        timings.packet_send_timeout,
        user_protocol.clone(),
    );

    // Build the builder.
    let usb = builder.build();
    info!("USB builder built");
    trace!("{}", usb.buffer_usage());

    let (tx, rx) = ww.split(); // TODO: do not split?
    let link = WireWeaverUsbLink::new(user_protocol, tx, &mut buffers.tx, rx, &mut buffers.rx);

    (
        UsbServer {
            usb,
            link,
            state,
            timings,
            rx_message: &mut buffers.rx_message,
            scratch_args: &mut buffers.scratch_args,
            scratch_event: &mut buffers.scratch_event,
            call_publish_rx: buffers.call_publish.receiver(),
        },
        buffers.call_publish.sender(),
    )
}
