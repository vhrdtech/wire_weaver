//! Reads the thermometer: one sample, then its mode and gain. The stream, the observed gain and the channels have
//! programs of their own: `thermometer_stream`, `thermometer_observe` and `thermometer_channels`.
//!
//! `cargo run -p thermometer --example thermometer_read` against `thermometer_sim` on this PC, or
//! `-- --url ws://ADDRESS:8080/ww` against another.

use anyhow::Result;
use clap::Parser;
use thermometer::Thermometer;
use thermometer_api::Gain;

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

    device.disconnect().asynch().await?;
    Ok(())
}
