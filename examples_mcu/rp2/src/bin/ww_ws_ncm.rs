//! WireWeaver API over WebSocket, on a network over USB: the board shows up on the host as a CDC-NCM Ethernet
//! adapter, runs embassy-net with a static IP and a small DHCP server (so the host gets an address by itself),
//! and accepts one WebSocket client at a time on port 8080. The same blinky API as `usb_blinky`. Also answers ping
//! and serves a hello world page at http://192.168.7.1.
//!
//! Run with `just run rp2 ww_ws_ncm`, then connect from the host with the `ws` feature of `wire_weaver_client`:
//! `ws://192.168.7.1:8080/ww`.
#![no_std]
#![no_main]

use core::net::Ipv4Addr;

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use edge_dhcp::io::server::run as dhcp_run;
use edge_dhcp::server::{Server as DhcpServer, ServerOptions};
use edge_http::Method;
use edge_http::io::Error as HttpError;
use edge_http::io::server::{Connection, Handler, Server as HttpServer};
use edge_nal::{TcpBind, UdpBind, WithTimeout};
use edge_nal_embassy::{Tcp, TcpBuffers, Udp, UdpBuffers};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Ipv4Cidr, Stack, StackResources, StaticConfigV4};
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver as UsbDriver, InterruptHandler};
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::class::cdc_ncm::embassy_net::{
    Device as NcmDevice, Runner as NcmRunner, State as NcmNetState,
};
use embassy_usb::class::cdc_ncm::{CdcNcmClass, State as NcmState};
use embassy_usb::{Builder, UsbDevice};
use embedded_io_async::{Read, Write};
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::ws::{EmbassyNetSocket, WsBuffers, WsConnection, WsServer, ws_server};
use ww_device::{EmbassyClock, LinkConfig};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// Device address, the host gets one from `DHCP_RANGE`
const DEVICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 7, 1);
const WS_PORT: u16 = 8080;
const HTTP_PORT: u16 = 80;
/// Locally administered MAC addresses: the device's own one, and the one the host's adapter gets
const DEVICE_MAC: [u8; 6] = [0x02, 0x77, 0x77, 0x00, 0x00, 0x01];
const HOST_MAC: [u8; 6] = [0x02, 0x77, 0x77, 0x00, 0x00, 0x02];
const MTU: usize = 1514;

/// Maximum WireWeaver message length
const MAX_MESSAGE_LEN: usize = 1024;
static WS_BUFFERS: StaticCell<WsBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();
static WS_CONNECTION: StaticCell<WsConnection<EmbassyNetSocket<'static>>> = StaticCell::new();

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
    static UDP_BUFFERS: StaticCell<UdpBuffers<1, 1024, 1024>> = StaticCell::new();
    let udp_buffers = UDP_BUFFERS.init(UdpBuffers::new());
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

const HTTP_PAGE: &[u8] = b"<!doctype html><title>WireWeaver</title><h1>Hello from WireWeaver!</h1>\
<p>API is at ws://192.168.7.1:8080/ww</p>\n";

/// Hello world page at `/`, 404 for anything else
struct HttpHandler;

impl Handler for HttpHandler {
    type Error<E>
        = HttpError<E>
    where
        E: core::fmt::Debug;

    async fn handle<T, const N: usize>(
        &self,
        _task_id: impl core::fmt::Display + Copy,
        conn: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write,
    {
        let headers = conn.headers()?;
        if headers.method != Method::Get {
            conn.initiate_response(405, Some("Method Not Allowed"), &[])
                .await
        } else if headers.path != "/" {
            conn.initiate_response(404, Some("Not Found"), &[]).await
        } else {
            conn.initiate_response(
                200,
                Some("OK"),
                &[("Content-Type", "text/html; charset=utf-8")],
            )
            .await?;
            conn.write_all(HTTP_PAGE).await
        }
    }
}

/// Plain HTTP on port 80, one connection at a time.
///
/// edge-http costs about 40 KB of flash and 3 KB of RAM (default release profile). For a single fixed page, a
/// hand-written task on a bare `TcpSocket` (read until `\r\n\r\n`, write the response, close) is enough and
/// saves most of that, see this file's history.
#[embassy_executor::task]
async fn http_task(stack: Stack<'static>) {
    static TCP_BUFFERS: StaticCell<TcpBuffers<1, 1024, 1024>> = StaticCell::new();
    static SERVER: StaticCell<HttpServer<1, 1024, 16>> = StaticCell::new();
    let tcp = Tcp::new(stack, TCP_BUFFERS.init(TcpBuffers::new()));
    let acceptor = unwrap!(
        tcp.bind(core::net::SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            HTTP_PORT
        ))
        .await
    );
    let server = SERVER.init(HttpServer::new());
    // a host that is gone without closing the connection: give up on it after 5 s
    let acceptor = WithTimeout::new(5_000, acceptor);
    if let Err(e) = server.run(Some(5_000), acceptor, HttpHandler).await {
        error!("http server: {:?}", Debug2Format(&e));
    }
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: WsServer<'static, EmbassyNetSocket<'static>, EmbassyClock>,
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
    info!("WireWeaver over WebSocket over USB CDC-NCM on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    let led = Output::new(p.PIN_25, Level::Low);

    // USB device with a single CDC-NCM function
    let driver = UsbDriver::new(p.USB, Irqs);
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("vhrd.tech");
    config.product = Some("WireWeaver WebSocket over NCM");
    config.serial_number = Some("rp2-ww-ws");
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

    // Network stack: static address, sockets for DHCP and the WebSocket server
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
    spawner.spawn(unwrap!(http_task(stack)));

    // WebSocket server on a single TCP socket
    static TCP_RX: StaticCell<[u8; 2048]> = StaticCell::new();
    static TCP_TX: StaticCell<[u8; 2048]> = StaticCell::new();
    let mut socket = TcpSocket::new(stack, TCP_RX.init([0; 2048]), TCP_TX.init([0; 2048]));
    // a host that is gone without closing the connection: writes fail after this, the link goes down
    socket.set_timeout(Some(Duration::from_secs(5)));
    socket.set_keep_alive(Some(Duration::from_secs(2)));
    let conn = WS_CONNECTION.init(WsConnection::new(EmbassyNetSocket::new(socket, WS_PORT)));
    let link_config = LinkConfig::new(
        blinky_api::BLINKY_API_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let server = ws_server(
        link_config,
        conn,
        EmbassyClock,
        WS_BUFFERS.init(WsBuffers::new()),
    );
    let state = ServerState { led };
    spawner.spawn(unwrap!(ww_server_task(server, state)));

    info!(
        "init done, ws://{}:{}/ww",
        Debug2Format(&DEVICE_IP),
        WS_PORT
    );
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
