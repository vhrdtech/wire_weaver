use crate::{Receiver, Sender, UsbTimings, WireWeaverClass};
use defmt::{debug, info};
use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Config, UsbDevice};
use ww_device::{EmbassyClock, FramedRx, FramedTx, LinkConfig, RxBuffer};

/// WireWeaver server over USB, see [ww_device::Server] on how to use it.
pub type UsbServer<'d, D> =
    ww_device::Server<'d, FramedTx<'d, Sender<'d, D>>, FramedRx<'d, Receiver<'d, D>>, EmbassyClock>;

/// Buffers used by [UsbServer].
///
/// * `MAX_USB_PACKET_LEN` - endpoint max packet size: up to 64 for Full Speed, 512 (Bulk) or 1024
///   (Interrupt) for High Speed. Bulk endpoints are capped at 512 automatically.
/// * `MAX_MESSAGE_LEN` - longest message the device accepts and replies it can serialize, reported to
///   the host exactly as is.
///
/// Takes `2 * MAX_MESSAGE_LEN + 2 * MAX_USB_PACKET_LEN` bytes.
pub struct ServerBuffers<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> {
    /// Used to receive USB packets and re-assemble messages from them
    rx: RxBuffer<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
    /// Used to prepare USB packets for transmission
    tx: [u8; MAX_USB_PACKET_LEN],
    /// Used to serialize replies, events and link messages
    scratch: [u8; MAX_MESSAGE_LEN],
}

impl<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> Default
    for ServerBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>
{
    fn default() -> Self {
        ServerBuffers {
            rx: RxBuffer::new(),
            tx: [0u8; MAX_USB_PACKET_LEN],
            scratch: [0u8; MAX_MESSAGE_LEN],
        }
    }
}

/// All buffers used by [usb_init].
pub struct UsbBuffers<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> {
    // buffer_usage() can be used to tune these
    config_descriptor: [u8; 96],
    bos_descriptor: [u8; 40],
    msos_descriptor: [u8; 330],
    control: [u8; 64],
    server: ServerBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
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
            server: ServerBuffers::default(),
        }
    }
}

impl<'d, D: Driver<'d>> WireWeaverClass<'d, D> {
    /// Create a server using this class for IO. `link_config.accumulation_time` is overwritten from `timings`.
    pub fn into_server<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize>(
        self,
        mut link_config: LinkConfig<'d>,
        timings: &UsbTimings,
        buffers: &'d mut ServerBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
    ) -> UsbServer<'d, D> {
        link_config.accumulation_time = timings.accumulation_time();
        // actual endpoint size, can be smaller than requested (e.g., Bulk is capped at 512)
        let max_packet_size = self.max_packet_size() as usize;
        defmt::assert!(
            max_packet_size <= MAX_USB_PACKET_LEN,
            "endpoint max packet size {} is larger than buffers for {}",
            max_packet_size,
            MAX_USB_PACKET_LEN
        );
        let (tx, rx) = self.split();
        ww_device::Server::new(
            link_config,
            FramedTx::new(tx, &mut buffers.tx[..max_packet_size]),
            FramedRx::new(rx, buffers.rx.assembly_buf(max_packet_size)),
            EmbassyClock,
            &mut buffers.scratch,
        )
    }
}

/// Initializes USB stack with default configuration and a single interface with WireWeaver class.
/// Device should work without drivers in Linux, macOS and Windows.
///
/// Returns the USB device, which must be run concurrently (`UsbDevice::run()`) and the WireWeaver
/// server, to be used in the user event loop (see [ww_device::Server]):
/// ```ignore
/// let (mut usb, mut server) = usb_init(driver, buffers, UsbTimings::fs_higher_speed(), link_config, |_| {});
/// join(usb.run(), async {
///     loop {
///         match select(server.wait(), other_source).await {
///             Either::First(ready) => { server.handle(ready, &mut state).await; }
///             Either::Second(x) => { /* use server.sink() to send stream updates */ }
///         }
///     }
/// }).await;
/// ```
///
/// This functions is a convenient way to initialize a minimum working device. If you need more
/// advanced setup (e.g., other USB classes alongside), do the same steps directly: create a
/// [Builder], add [WireWeaverClass::new] and other classes, build and call [WireWeaverClass::into_server].
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
>(
    driver: D,
    buffers: &'d mut UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
    timings: UsbTimings,
    link_config: LinkConfig<'d>,
    config_mut: impl FnOnce(&mut Config),
) -> (UsbDevice<'d, D>, UsbServer<'d, D>) {
    const {
        assert!(
            MAX_USB_PACKET_LEN >= 8 && MAX_USB_PACKET_LEN <= 1024,
            "USB max packet size must be in 8..=1024"
        )
    };
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

    let ww = WireWeaverClass::new(
        &mut builder,
        MAX_USB_PACKET_LEN as u16,
        timings.use_bulk_endpoints,
        timings.packet_send_timeout,
        &link_config.user_api_version,
    );

    let usb = builder.build();
    info!("USB builder built");
    debug!("{}", usb.buffer_usage());

    let server = ww.into_server(link_config, &timings, &mut buffers.server);
    (usb, server)
}
