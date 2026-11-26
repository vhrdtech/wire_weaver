//! The thermometer's inputs as an array of channels: which exist, each one's last sample, turning one off, and a
//! channel the board doesn't have; then the status LED, a single sub-trait.
//!
//! `cargo run -p thermometer --example thermometer_channels` against `thermometer_sim` on this PC, or
//! `-- ws://ADDRESS:8080/ww` against another.

use std::io::{self, Write};

use anyhow::Result;
use thermometer::Thermometer;
use thermometer_api::IndicationMode;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:8080/ww".to_owned());
    let device = Thermometer::config(|c| c.websocket_url(url.clone()).no_ww_usb())
        .connect()
        .await?;
    run(&device, &mut io::stdout()).await?;
    device.disconnect().asynch().await?;
    Ok(())
}

/// Lists the channels, turns channel 1 off, asks a channel past the board, and dims the status LED.
pub async fn run(device: &Thermometer, out: &mut impl Write) -> Result<()> {
    // --8<-- [start:list]
    // the device says which indices exist
    let valid = device.channel_valid_indices().read().await?;
    for index in valid.iter() {
        let channel = device.channel(index);
        let enabled = channel.read_enabled().read().await?;
        let last = channel.last().call().await?;
        writeln!(
            out,
            "channel[{index}]: enabled {enabled}, input {}, {}",
            last.input.get(),
            last.temp
        )?;
    }
    // --8<-- [end:list]

    // --8<-- [start:one]
    device.channel(1).write_enabled(false).write().await?;
    let enabled = device.channel(1).read_enabled().read().await?;
    writeln!(out, "channel[1]: enabled {enabled}")?;
    // an index the device doesn't have: the request fails
    if let Err(e) = device.channel(4).last().call().await {
        writeln!(out, "channel[4]: {e}")?;
    }
    // --8<-- [end:one]

    // --8<-- [start:indication]
    let indication = device.indication();
    indication
        .write_mode(IndicationMode::OnlyEssential)
        .write()
        .await?;
    writeln!(
        out,
        "indication: {:?}",
        indication.read_mode().read().await?
    )?;
    // --8<-- [end:indication]
    Ok(())
}
