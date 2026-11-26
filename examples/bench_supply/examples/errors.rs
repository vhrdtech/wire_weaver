//! The errors of the bench supply as a host program sees them: the API's own `OutputError`, a common error the
//! device answers a property write with, and one `?` that takes both.
//!
//! `cargo run -p bench_supply --example bench_supply_errors` against `bench_supply_sim` on this PC, or
//! `-- ws://ADDRESS:8081/ww` against another.

use std::io::{self, Write};

use anyhow::Result;
use bench_supply::BenchSupply;
use bench_supply_api::{Current, OutputError, Voltage};
use wire_weaver_client::CallError;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:8081/ww".to_owned());
    let device = BenchSupply::config(|c| c.websocket_url(url.clone()).no_ww_usb())
        .connect()
        .await?;
    run(&device, &mut io::stdout()).await?;
    device.disconnect().asynch().await?;
    Ok(())
}

/// Makes channel 0 refuse in each way the chapter shows, one line per refusal.
pub async fn run(device: &BenchSupply, out: &mut impl Write) -> Result<()> {
    let channel = device.channel(0);

    // --8<-- [start:match]
    match channel.set_output(true).call().await {
        Ok(()) => writeln!(out, "output on")?,
        Err(CallError::Device(OutputError::NoVoltageSet)) => {
            writeln!(out, "set a voltage first")?;
        }
        Err(e) => return Err(e.into()),
    }
    // --8<-- [end:match]

    // --8<-- [start:would_draw]
    channel
        .write_voltage_limit(Voltage::bounded(30_000))
        .write()
        .await?;
    channel
        .write_current_limit(Current::bounded(1_000))
        .write()
        .await?;
    if let Err(CallError::Device(e)) = channel.set_output(true).call().await {
        writeln!(out, "refused: {e}")?;
        if let OutputError::OverCurrent { would_draw } = e {
            writeln!(out, "would draw: {would_draw}")?;
        }
    }
    // --8<-- [end:would_draw]

    // --8<-- [start:common]
    channel
        .write_voltage_limit(Voltage::bounded(5_000))
        .write()
        .await?;
    channel.set_output(true).call().await?;
    let refused = channel
        .write_voltage_limit(Voltage::bounded(6_000))
        .write()
        .await;
    if let Err(e) = refused {
        match e.common() {
            Some(common) => writeln!(out, "{}: {common}", common.name())?,
            None => return Err(e.into()),
        }
    }
    // --8<-- [end:common]

    // --8<-- [start:question_mark]
    if let Err(e) = switch_on(device, 1).await {
        writeln!(out, "error: {e}")?;
    }
    // --8<-- [end:question_mark]

    channel.set_output(false).call().await?;
    Ok(())
}

// --8<-- [start:switch_on]
/// Sets `channel` to 3.3 V and 0.5 A and turns its output on.
async fn switch_on(device: &BenchSupply, channel: u32) -> Result<()> {
    let channel = device.channel(channel);
    channel
        .write_voltage_limit(Voltage::bounded(3_300))
        .write()
        .await?;
    channel
        .write_current_limit(Current::bounded(500))
        .write()
        .await?;
    channel.set_output(true).call().await?;
    Ok(())
}
// --8<-- [end:switch_on]
