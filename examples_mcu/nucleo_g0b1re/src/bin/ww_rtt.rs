//! WireWeaver API over RTT, next to defmt logging on another RTT channel: no USB needed, the
//! debug probe is the transport. The same blinky API as `usb_blinky`, only the medium differs.
//!
//! Run with `just run nucleo ww_rtt`, then stop `probe-rs run` (Ctrl+C, the firmware keeps running) and connect
//! from the host: `cargo run -p blinky --features rtt --example blinky_rtt`, or with the `rtt` feature of `wire_weaver_client`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use blinky_device::{Blinky, Led};
use cortex_m_rt::exception;
use defmt::{Debug2Format, error, info};
use embassy_stm32::{
    Config,
    gpio::{Level, Output, Speed},
};
use panic_probe as _;
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};
use static_cell::StaticCell;
use ww_device::EmbassyClock;
use ww_device::rtt::{RttBuffers, RttConfig, rtt_server};

/// Maximum WireWeaver message length, RTT channels below must be able to hold a whole message
/// plus its framing (see `ww_device::rtt`), and should be at least that size for `NoBlockSkip`.
const MAX_MESSAGE_LEN: usize = 1024;
static RTT_BUFFERS: StaticCell<RttBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();

static LED: StaticCell<BoardLed> = StaticCell::new();

/// The board's user LED as the blinky device sees it; each change is logged.
struct BoardLed(Output<'static>);

impl Led for BoardLed {
    fn set(&mut self, on: bool) {
        info!("led {}", if on { "on" } else { "off" });
        if on {
            self.0.set_high();
        } else {
            self.0.set_low();
        }
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
    let mut blinky = Blinky::new(LED.init(BoardLed(led)));

    let mut server = rtt_server(
        Blinky::link_config(),
        channels.up.1,
        channels.down.0,
        EmbassyClock,
        RttConfig::default(),
        RTT_BUFFERS.init(RttBuffers::new()),
    );
    info!("init done");
    blinky_device::serve(&mut server, &mut blinky, core::future::pending()).await
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
