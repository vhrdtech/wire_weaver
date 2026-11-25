//! WireWeaver API over RTT, next to defmt logging on another RTT channel: no USB needed, the
//! debug probe is the transport. The same blinky device (`examples/blinky_device`) as `usb_blinky`, only the medium
//! differs.
//!
//! Run with `just run rp2 ww_rtt`, then connect from the host with the `rtt` feature of
//! `wire_weaver_client`, or `ww` CLI.
#![no_std]
#![no_main]

use blinky_device::{Blinky, Led};
use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
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

/// The board's LED as the blinky device sees it.
struct BoardLed(Output<'static>);

impl Led for BoardLed {
    fn set(&mut self, on: bool) {
        if on {
            self.0.set_high();
        } else {
            self.0.set_low();
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
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
    info!("WireWeaver over RTT on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    let led = Output::new(p.PIN_25, Level::Low);
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
