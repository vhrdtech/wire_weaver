// This example demonstrates how to use WireWeaver API over RTT simultaneously with defmt for logging
#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_time::Timer;
use gpio::{Level, Output};
use panic_probe as _;
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};

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
                size: 512,
                mode: NoBlockSkip,
                name: "ww_up"
            }
        }
        down: {
            0: {
                size: 512,
                mode: NoBlockSkip,
                name: "ww_down"
            }
        }
    };
    rtt_target::set_defmt_channel(channels.up.0);
    info!("WireWeaver over RTT on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    // let mut led = Output::new(p.PIN_25, Level::Low);

    let mut ww_up = channels.up.1;
    let mut ww_down = channels.down.0;
    let mut buf = [0u8; 512];
    loop {
        let count = ww_down.read(&mut buf[..]);
        if count > 0 {
            for c in buf.iter_mut() {
                c.make_ascii_uppercase();
            }

            let mut p = 0;
            while p < count {
                p += ww_up.write(&buf[p..count]);
            }
        }
    }
}
