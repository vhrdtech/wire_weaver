//! Watches the thermometer's gain: the host observes it once, then the device sends each change, the host's own
//! write and the device lowering the gain after its amplifier clipped.
//!
//! `cargo run -p thermometer --example thermometer_observe` against `thermometer_sim` on this PC, or
//! `-- ws://ADDRESS:8080/ww` against another.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use thermometer::Thermometer;
use thermometer_api::Gain;
use tokio::time::Instant;
use wire_weaver_client::observe::on_change;

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

/// Observes the gain, writes it, then makes the device lower it; one line per step, with the seconds since the start.
pub async fn run(device: &Thermometer, out: &mut impl Write) -> Result<()> {
    let start = Instant::now();
    let at = || start.elapsed().as_secs_f32();

    // --8<-- [start:subscribe]
    // every change, as soon as it happens; the first update is the value now
    writeln!(out, "{:.1} s  host observes gain", at())?;
    let mut gain = device
        .observe_gain(on_change(Duration::ZERO))
        .subscribe()
        .await?;
    let update = gain.recv().await?;
    writeln!(out, "{:.1} s  update: gain {}", at(), update.value.get())?;
    // --8<-- [end:subscribe]

    // --8<-- [start:changes]
    tokio::time::sleep(Duration::from_secs(1)).await;
    device.write_gain(Gain::bounded(16)).write().await?;
    writeln!(out, "{:.1} s  host writes gain 16", at())?;
    let update = gain.recv().await?;
    writeln!(out, "{:.1} s  update: gain {}", at(), update.value.get())?;

    // above gain 8 the simulated amplifier clips, and the device halves the gain at its next reading
    tokio::time::sleep(Duration::from_secs(1)).await;
    device.read_once().call().await?;
    writeln!(out, "{:.1} s  host reads once: the amplifier clips", at())?;
    let update = gain.recv().await?;
    writeln!(out, "{:.1} s  update: gain {}", at(), update.value.get())?;
    // --8<-- [end:changes]
    Ok(())
}
