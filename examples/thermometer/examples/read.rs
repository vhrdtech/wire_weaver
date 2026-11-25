//! Reads the thermometer: one sample, its mode and gain, then ten samples at 5 Hz from its stream, then the gain as
//! the device lowers it.
//!
//! `cargo run -p thermometer --example thermometer_read` against `thermometer_sim` on this PC, or
//! `-- --url ws://ADDRESS:8080/ww` against another.

use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use thermometer::Thermometer;
use thermometer_api::{Gain, Mode, Rate};
use wire_weaver_client::observe::on_change;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:8080/ww")]
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let device = Thermometer::config(|c| c.websocket_url(args.url.clone()).no_ww_usb())
        .connect()
        .await?;

    // --8<-- [start:read]
    let sample = device.read_once().call().await?;
    println!("input {}: {}", sample.input.get(), sample.temp);
    // --8<-- [end:read]

    // --8<-- [start:properties]
    let mode = device.read_mode().read().await?;
    println!("mode: {mode:?}");
    device.write_gain(Gain::bounded(8)).write().await?;
    println!("gain: {}", device.read_gain().read().await?.get());
    // 20 is no Gain: the type refuses it, so a bad value never reaches the wire
    if let Err(e) = Gain::try_new(20) {
        println!("refused: {e}");
    }
    // --8<-- [end:properties]

    // --8<-- [start:stream]
    let mut samples = device.samples().call().await?;
    device.channel(1).write_enabled(false).write().await?;
    device.set_rate(Rate::bounded(5)).call().await?;
    device.write_mode(Mode::Continuous).write().await?;
    for _ in 0..10 {
        let s = samples.recv().await?;
        println!("#{} input {}: {}", s.seq.get(), s.input.get(), s.temp);
    }
    device.write_mode(Mode::Idle).write().await?;
    // --8<-- [end:stream]

    // --8<-- [start:observe]
    let mut gain = device
        .observe_gain(on_change(Duration::ZERO))
        .subscribe()
        .await?;
    // above gain 8 the simulated amplifier clips, and the device halves the gain at its next reading:
    // the observer sees 8, 16, then 8
    device.write_gain(Gain::bounded(16)).write().await?;
    device.read_once().call().await?;
    for _ in 0..3 {
        println!("gain {}", gain.recv().await?.value.get());
    }
    // --8<-- [end:observe]

    device.disconnect().asynch().await?;
    Ok(())
}
