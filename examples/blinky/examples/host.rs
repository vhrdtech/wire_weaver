//! A host program for the blinky device: turns the LED on and off, then calls the methods with an argument and a
//! return value. It connects to the simulated device by default, or to a board over USB with `--usb`.
//!
//! `cargo run -p blinky --example blinky_sim` in one terminal, `cargo run -p blinky --example blinky_host` in
//! another; with a board plugged in, `cargo run -p blinky --example blinky_host -- --usb`.

use anyhow::Result;
use blinky::Blinky;
use clap::Parser;

#[derive(Parser)]
struct Args {
    /// The simulated device's address
    #[arg(long, default_value = "ws://127.0.0.1:8080/ww")]
    url: String,
    /// Connect to a board over USB instead
    #[arg(long)]
    usb: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // --8<-- [start:connect]
    let device = if args.usb {
        Blinky::new().connect().await?
    } else {
        Blinky::config(|c| c.websocket_url(args.url.clone()).no_ww_usb())
            .connect()
            .await?
    };
    // --8<-- [end:connect]

    // --8<-- [start:calls]
    device.led_on().call().await?;
    device.led_off().call().await?;
    // --8<-- [end:calls]

    // --8<-- [start:methods]
    let was_on = device.set_led(true).call().await?;
    println!("set_led(true): the LED was on before: {was_on}");
    let blinks = device.blinks().call().await?;
    println!("blinks: {}", blinks.get());
    // --8<-- [end:methods]

    device.disconnect().asynch().await?;
    Ok(())
}
