//! A host program for the blinky device: turns the LED on and off, then calls the methods with an argument and a
//! return value. It connects to the simulated device by default, or to a board over USB with `--usb`.
//!
//! `cargo run -p blinky --example blinky_sim` in one terminal, `cargo run -p blinky --example blinky_host` in
//! another; with a board plugged in, `cargo run -p blinky --example blinky_host -- --usb`. The test
//! `book_output_host` runs [run] against the simulated device and checks what it prints.

use std::io::Write;

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

    run(&device, &mut std::io::stdout()).await?;
    device.disconnect().asynch().await?;
    Ok(())
}

/// The calls of the book's chapters, in order; what they print goes to `out`.
pub(crate) async fn run(device: &Blinky, out: &mut impl Write) -> Result<()> {
    // --8<-- [start:calls]
    device.led_on().call().await?;
    device.led_off().call().await?;
    // --8<-- [end:calls]

    // --8<-- [start:methods]
    let was_on = device.set_led(true).call().await?;
    writeln!(out, "set_led(true): the LED was on before: {was_on}")?;
    let blinks = device.blinks().call().await?;
    writeln!(out, "blinks: {}", blinks.get())?;
    // --8<-- [end:methods]
    Ok(())
}
