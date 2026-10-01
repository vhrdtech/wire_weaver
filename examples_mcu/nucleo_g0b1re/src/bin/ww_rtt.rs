//! WireWeaver API over RTT, next to defmt logging on another RTT channel: no USB needed, the
//! debug probe is the transport. The same blinky API as `usb_blinky`, only the medium differs.
//!
//! Run with `just run nucleo ww_rtt`, then stop `probe-rs run` (Ctrl+C, the firmware keeps running) and connect
//! from the host: `cargo run -p blinky --features rtt --example blinky_rtt`, or with the `rtt` feature of `wire_weaver_client`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::{Debug2Format, error, info};
use embassy_stm32::{
    Config,
    gpio::{Level, Output, Speed},
};
use panic_probe as _;
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::rtt::{RttBuffers, RttConfig, rtt_server};
use ww_device::{EmbassyClock, LinkConfig};

/// Maximum WireWeaver message length, RTT channels below must be able to hold a whole message
/// plus its framing (see `ww_device::rtt`), and should be at least that size for `NoBlockSkip`.
const MAX_MESSAGE_LEN: usize = 1024;
static RTT_BUFFERS: StaticCell<RttBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();

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
        introspect = "no_docs",
    );
}

impl ServerState {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        info!("led on");
        self.led.set_high();
        Ready(())
    }

    async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        info!("led off");
        self.led.set_low();
        Ready(())
    }
}

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let channels = rtt_init! {
        up: {
            0: {
                size: 1024,
                mode: NoBlockSkip,
                name: "defmt"
            }
            1: {
                size: 2048,
                mode: NoBlockSkip,
                name: "ww_up"
            }
        }
        down: {
            0: {
                size: 2048,
                mode: NoBlockSkip,
                name: "ww_down"
            }
        }
    };
    rtt_target::set_defmt_channel(channels.up.0);
    info!("WireWeaver over RTT on STM32G0B1 starting...");

    let p = embassy_stm32::init(Config::default());

    #[cfg(feature = "b129a_cannify")]
    let led = Output::new(p.PB14, Level::Low, Speed::Low);
    #[cfg(feature = "nucleo_g0b1re")]
    let led = Output::new(p.PA5, Level::Low, Speed::Low);
    let mut state = ServerState { led };

    let link_config = LinkConfig::new(
        blinky_api::BLINKY_API_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let mut server = rtt_server(
        link_config,
        channels.up.1,
        channels.down.0,
        EmbassyClock,
        RttConfig::default(),
        RTT_BUFFERS.init(RttBuffers::new()),
    );
    info!("init done");
    // Nothing else to wait for: use the prepared loop. See ww_device::Server for a custom one.
    server.run(&mut state).await
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
