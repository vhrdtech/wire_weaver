//! Blink over RTT through a debug probe, against `ww_rtt` firmware from `examples_mcu` (e.g., `just run nucleo ww_rtt`
//! in `examples_mcu/nucleo_g0b1re`). Stop `probe-rs run` first, a probe can only be opened once.
//!
//! `cargo run -p blinky --features rtt --example blinky_rtt -- --chip STM32G0B1RETx`

use anyhow::Result;
use blinky::Blinky;
use clap::Parser;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    /// probe-rs chip name
    #[arg(long, default_value = "STM32G0B1RETx")]
    chip: String,
    /// Select a probe by serial number (substring), if several are connected
    #[arg(long)]
    probe_serial: Option<String>,
    /// SWD / JTAG clock
    #[arg(long)]
    speed_hz: Option<u32>,
    #[arg(long, default_value_t = 3)]
    blinks: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // default config (device's USB VID:PID) is kept, only filters after rtt(..) select the probe
    let device = Blinky::config(|c| {
        let c = c.rtt(args.chip.clone(), args.speed_hz);
        match &args.probe_serial {
            Some(serial) => c.serial_contains(serial.clone()),
            None => c,
        }
    })
    .connect()
    .await?;
    println!("Connected: {:?}", device.info());

    for _ in 0..args.blinks {
        println!("Turning LED on");
        device.led_on().call().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("Turning LED off");
        device.led_off().call().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    device.disconnect().asynch().await?;
    Ok(())
}
