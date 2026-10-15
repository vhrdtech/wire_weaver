//! defmt logs over USB and RTT at once: `ww_defmt_logger` is the global logger, every frame goes to an RTT channel
//! named `defmt` (`cnt run` / `probe-rs` show it as usual) and into the `ww_log::LogDefmt` stream, whose store keeps
//! the logs while no host is connected.
//!
//! Run with `just run rp2 usb_log`, then on the host:
//! `cargo run -p defmt_log --example defmt_log -- --elf examples_mcu/rp2/target/thumbv8m.main-none-eabihf/debug/rp2_usb_log`
//! (`--flood 500` overloads the logger: the host shows exactly how many messages were dropped).
#![no_std]
#![no_main]

use cortex_m_rt::exception;
use defmt::{Debug2Format, error, info, unwrap, warn};
use embassy_futures::select::{Either, select};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver as UsbDriver, InterruptHandler};
use embassy_time::{Instant, Timer};
use panic_probe as _;
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_usb_embassy::{LinkConfig, UsbBuffers, UsbDevice, UsbTimings, usb_init};
use ww_device::LinkEvent;
use ww_firmware_info::build_id;
use ww_stream::store::{FullPolicy, RamRing};
use ww_stream::tx::Reliable;

// the defmt global logger: frames between two pumps of the event loop fit into 4 KiB
ww_defmt_logger::global_logger!(LOG, 4096);

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64;
const MAX_MESSAGE_LEN: usize = 1024;
/// Logs kept until a host acknowledges them, the oldest go first when full
const LOG_STORE_LEN: usize = 16384;
type LogStream = Reliable<RamRing<LOG_STORE_LEN>>;

static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
static STATE: StaticCell<ServerState> = StaticCell::new();

struct ServerState {
    log_frames_stream: LogStream,
    stream_sessions: u32,
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
        defmt_log_api::DEFMT_LOG_DEMO_FULL_GID
    }
}

mod server_impl {
    wire_weaver::ww_codegen!(
        defmt_log_api :: DefmtLogDemo for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
    );
}

impl ServerState {
    async fn flood(&mut self, _cx: &mut Context<'_, impl EventOut>, count: u32) -> RpcResult<()> {
        for i in 0..count {
            info!("flood {=u32} of {=u32}", i + 1, count);
        }
        Ready(())
    }

    // TODO(API-36): consts served from a value, without a handler named after the const
    #[allow(non_snake_case)]
    async fn get_log_DEFMT(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<ww_log::DefmtInfo<'_>, ()> {
        GetResult::Value(ww_log::DefmtInfo {
            encoding: ww_defmt_logger::encoding(),
            build_id: MaxLen::new_unchecked(RefVec::new_bytes(build_id())),
        })
    }

    async fn log_frames(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), ww_log::LogError<'_>>> {
        Ready(Ok(()))
    }

    async fn close_log_frames(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

    async fn stream_log_frames(&mut self) -> &mut LogStream {
        &mut self.log_frames_stream
    }

    fn new_stream_session(&mut self) -> u32 {
        // TODO: put a boot counter into the high bits, so that a cursor from before a reboot doesn't match
        self.stream_sessions = self.stream_sessions.wrapping_add(1);
        self.stream_sessions
    }
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn heartbeat() {
    let mut n = 0u32;
    loop {
        info!("heartbeat {=u32}", n);
        n += 1;
        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let channels = rtt_init! {
        up: {
            0: {
                size: 1024,
                mode: NoBlockSkip,
                name: "defmt"
            }
        }
    };
    LOG.set_rtt_channel(channels.up.0);
    info!(
        "defmt over USB and RTT on RP235x starting, build id {=[u8]:02x}",
        build_id()
    );

    let p = embassy_rp::init(Default::default());

    let state = STATE.init(ServerState {
        log_frames_stream: Reliable::new(RamRing::new().with_policy(FullPolicy::DropOldest)),
        stream_sessions: 0,
    });
    // a session from boot on: the store keeps the logs until a host resumes it
    let session = state.new_stream_session();
    if let Err(e) = state.log_frames_stream.start_session(session).await {
        error!("log store: {}", Debug2Format(&e));
    }

    let driver = UsbDriver::new(p.USB, Irqs);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let link_config = LinkConfig::new(
        defmt_log_api::DEFMT_LOG_DEMO_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let (usb, mut server) = usb_init(
        driver,
        buffers,
        UsbTimings::fs_higher_speed(),
        link_config,
        server_impl::API_ID,
        |_config| {},
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(heartbeat()));
    info!("init done");

    // the event loop: requests, then the logger's ring into the store, then the store out to the host
    let mut buf = [0u8; 600];
    loop {
        if let Either::First(ready) = select(server.wait(), Timer::after_millis(5)).await
            && let Some(event) = server.handle(ready, state).await
        {
            info!("link {}", event);
            if matches!(event, LinkEvent::Down(_)) {
                state.log_frames_stream.detach();
            }
        }
        if let Err(e) = LOG.pump(&mut state.log_frames_stream).await {
            warn!("log store: {}", Debug2Format(&e));
        }
        if server.is_up() {
            let now_ms = Instant::now().as_millis();
            _ = state
                .poll_streams(&mut server.sink(), now_ms, &mut buf, 2048)
                .await;
        }
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    cnt::cnt!(unhandled_exceptions: u32, warn);
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}
