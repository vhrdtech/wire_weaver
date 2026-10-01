//! Blink over WebSocket, against `ww_ws_ncm` firmware from `examples_mcu/rp2` (`just run rp2 ww_ws_ncm`): the board is
//! a USB network adapter and gives the host an address over DHCP, the API is at `ws://192.168.7.1:8080/ww`.
//!
//! `cargo run -p blinky --features ws --example blinky_ws`

use anyhow::Result;
use blinky::Blinky;
use clap::Parser;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "ws://192.168.7.1:8080/ww")]
    url: String,
    #[arg(long, default_value_t = 3)]
    blinks: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // the default config also selects USB devices with the WireWeaver VID:PID, which the NCM firmware has too
    let device = Blinky::config(|c| c.websocket_url(args.url.clone()).no_ww_usb())
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
