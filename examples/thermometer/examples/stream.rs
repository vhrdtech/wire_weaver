//! Streams the thermometer's samples and sends it commands: continuous samples at 5 Hz, then one triggered sample,
//! then a triggered sample after the numbering was restarted.
//!
//! `cargo run -p thermometer --example thermometer_stream` against `thermometer_sim` on this PC, or
//! `-- ws://ADDRESS:8080/ww` against another.

use std::io::{self, Write};

use anyhow::Result;
use thermometer::Thermometer;
use thermometer_api::{Command, Mode, Rate, Sample};
use tokio::time::Instant;
use ww_si::Seq;

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

/// Receives two rounds of continuous samples, then two triggered ones; one line per sample, with the seconds since
/// the start.
pub async fn run(device: &Thermometer, out: &mut impl Write) -> Result<()> {
    let start = Instant::now();
    let mut print = |s: &Sample| {
        writeln!(
            out,
            "{:.1} s  #{} input {}: {}",
            start.elapsed().as_secs_f32(),
            s.seq.get(),
            s.input.get(),
            s.temp
        )
    };

    // --8<-- [start:stream]
    // open the stream first: only samples sent after the open reach the host
    let mut samples = device.samples().call().await?;
    device.set_rate(Rate::bounded(5)).call().await?;
    device.write_mode(Mode::Continuous).write().await?;
    // each round reads the four inputs: two rounds are eight samples
    for _ in 0..8 {
        print(&samples.recv().await?)?;
    }
    device.write_mode(Mode::Idle).write().await?;
    // --8<-- [end:stream]

    // --8<-- [start:sink]
    let mut commands = device.commands().await?;
    device.write_mode(Mode::Triggered).write().await?;
    commands.send(Command::Trigger).await?;
    print(&samples.recv().await?)?;
    // restart the numbering at 1000, then trigger once more
    commands
        .send(Command::Mark {
            seq: Seq::new(1000),
        })
        .await?;
    commands.send(Command::Trigger).await?;
    print(&samples.recv().await?)?;
    // --8<-- [end:sink]
    device.write_mode(Mode::Idle).write().await?;
    Ok(())
}
