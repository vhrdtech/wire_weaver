//! WireWeaver API over UDP, on a network over USB: the board shows up on the host as a CDC-NCM Ethernet adapter, runs
//! embassy-net with a static IP and a small DHCP server (so the host gets an address by itself), and serves the same
//! blinky API as `usb_blinky` on UDP port 9000, to one host at a time. Also answers ping.
//!
//! Run with `just run rp2 ww_udp_ncm`, then connect from the host with the `udp` feature of `wire_weaver_client`:
//! `cargo run -p blinky --features udp --example blinky_udp` (`192.168.7.1:9000`).
#![no_std]
#![no_main]

use core::net::Ipv4Addr;

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use edge_dhcp::io::server::run as dhcp_run;
use edge_dhcp::server::{Server as DhcpServer, ServerOptions};
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers as DhcpUdpBuffers};
use embassy_executor::Spawner;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Ipv4Cidr, Stack, StackResources, StaticConfigV4};
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver as UsbDriver, InterruptHandler};
use embassy_time::{Instant, Timer};
use embassy_usb::class::cdc_ncm::embassy_net::{
    Device as NcmDevice, Runner as NcmRunner, State as NcmNetState,
};
use embassy_usb::class::cdc_ncm::{CdcNcmClass, State as NcmState};
use embassy_usb::{Builder, UsbDevice};
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::udp::{EmbassyNetUdpSocket, UdpBuffers, UdpConnection, UdpServer, udp_server};
use ww_device::{EmbassyClock, LinkConfig};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// Device address, the host gets one from `DHCP_RANGE`
const DEVICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 7, 1);
const UDP_PORT: u16 = 9000;
/// Locally administered MAC addresses: the device's own one, and the one the host's adapter gets
const DEVICE_MAC: [u8; 6] = [0x02, 0x77, 0x77, 0x00, 0x00, 0x01];
const HOST_MAC: [u8; 6] = [0x02, 0x77, 0x77, 0x00, 0x00, 0x02];
const MTU: usize = 1514;

/// Maximum WireWeaver message length
const MAX_MESSAGE_LEN: usize = 1024;
static UDP_BUFFERS: StaticCell<UdpBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();
static UDP_CONNECTION: StaticCell<UdpConnection<EmbassyNetUdpSocket<'static>>> = StaticCell::new();

type Usb = UsbDriver<'static, USB>;

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Usb>) -> ! {
    usb.run().await
}

#[embassy_executor::task]
async fn ncm_task(runner: NcmRunner<'static, Usb, MTU>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, NcmDevice<'static, MTU>>) -> ! {
    runner.run().await
}

/// Hands out addresses to the host (normally just one), so that no manual network configuration is needed there.
#[embassy_executor::task]
async fn dhcp_task(stack: Stack<'static>) {
    static DHCP_UDP_BUFFERS: StaticCell<DhcpUdpBuffers<1, 1024, 1024>> = StaticCell::new();
    let udp_buffers = DHCP_UDP_BUFFERS.init(DhcpUdpBuffers::new());
    let udp = Udp::new(stack, udp_buffers);
    let mut socket = unwrap!(
        udp.bind(core::net::SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 67))
            .await
    );
    let mut options = ServerOptions::new(DEVICE_IP, None);
    // no gateway and no DNS: the host keeps using its other connections for everything else
    options.lease_duration_secs = 3600;
    let mut server = DhcpServer::<_, 2>::new(|| Instant::now().as_secs(), DEVICE_IP);
    let mut buf = [0u8; 1024];
    loop {
        if let Err(e) = dhcp_run(&mut server, &options, &mut socket, &mut buf).await {
            warn!("dhcp server: {:?}", Debug2Format(&e));
            Timer::after_millis(500).await;
        }
    }
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UdpServer<'static, EmbassyNetUdpSocket<'static>, EmbassyClock>,
    mut state: ServerState,
) {
    // Nothing else to wait for: use the prepared loop. See ww_device::Server for a custom one.
    server.run(&mut state).await
}

struct ServerState {
    led: Output<'static>,
}

impl WireWeaverAsyncApiBackend for ServerState {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        blinky_api::BLINKY_API_FULL_GID
    }
}

mod server_impl {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
    );
}

impl ServerState {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.led.set_high();
        Ready(())
    }

    async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.led.set_low();
        Ready(())
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("WireWeaver over UDP over USB CDC-NCM on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    let led = Output::new(p.PIN_25, Level::Low);

    // USB device with a single CDC-NCM function
    let driver = UsbDriver::new(p.USB, Irqs);
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("vhrd.tech");
    config.product = Some("WireWeaver UDP over NCM");
    config.serial_number = Some("rp2-ww-udp");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [], // no msos descriptors
        CONTROL_BUF.init([0; 128]),
    );
    static NCM_STATE: StaticCell<NcmState> = StaticCell::new();
    let class = CdcNcmClass::new(&mut builder, NCM_STATE.init(NcmState::new()), HOST_MAC, 64);
    let usb = builder.build();
    spawner.spawn(unwrap!(usb_task(usb)));

    static NCM_NET_STATE: StaticCell<NcmNetState<MTU, 4, 4>> = StaticCell::new();
    let (ncm_runner, device) = class
        .into_embassy_net_device::<MTU, 4, 4>(NCM_NET_STATE.init(NcmNetState::new()), DEVICE_MAC);
    spawner.spawn(unwrap!(ncm_task(ncm_runner)));

    // Network stack: static address, sockets for DHCP and the WireWeaver server
    let net_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(DEVICE_IP, 24),
        gateway: None,
        dns_servers: Default::default(),
    });
    static NET_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        device,
        net_config,
        NET_RESOURCES.init(StackResources::new()),
        RoscRng.next_u64(),
    );
    spawner.spawn(unwrap!(net_task(net_runner)));
    spawner.spawn(unwrap!(dhcp_task(stack)));

    // WireWeaver server on a single UDP socket, its buffers must fit at least one maximum datagram each way
    static RX_META: StaticCell<[PacketMetadata; 4]> = StaticCell::new();
    static TX_META: StaticCell<[PacketMetadata; 4]> = StaticCell::new();
    static UDP_RX: StaticCell<[u8; 4096]> = StaticCell::new();
    static UDP_TX: StaticCell<[u8; 4096]> = StaticCell::new();
    let socket = UdpSocket::new(
        stack,
        RX_META.init([PacketMetadata::EMPTY; 4]),
        UDP_RX.init([0; 4096]),
        TX_META.init([PacketMetadata::EMPTY; 4]),
        UDP_TX.init([0; 4096]),
    );
    let socket = unwrap!(EmbassyNetUdpSocket::bind(socket, UDP_PORT));
    let conn = UDP_CONNECTION.init(UdpConnection::new(socket));
    let link_config = LinkConfig::new(
        blinky_api::BLINKY_API_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let server = udp_server(
        link_config,
        conn,
        EmbassyClock,
        UDP_BUFFERS.init(UdpBuffers::new()),
    );
    let state = ServerState { led };
    spawner.spawn(unwrap!(ww_server_task(server, state)));

    info!("init done, {}:{}", Debug2Format(&DEVICE_IP), UDP_PORT);
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}
