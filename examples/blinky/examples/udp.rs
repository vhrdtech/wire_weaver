//! Blink over UDP, against `ww_udp_ncm` firmware from `examples_mcu/rp2` (`just run rp2 ww_udp_ncm`): the board is
//! a USB network adapter and gives the host an address over DHCP, the API is at `192.168.7.1:9000`.
//!
//! `cargo run -p blinky --features udp --example blinky_udp`

use anyhow::Result;
use blinky::Blinky;
use clap::Parser;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "192.168.7.1:9000")]
    addr: String,
    #[arg(long, default_value_t = 3)]
    blinks: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // the default config also selects USB devices with the WireWeaver VID:PID, which the NCM firmware has too
    let device = Blinky::config(|c| c.udp_addr(args.addr.clone()).no_ww_usb())
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
