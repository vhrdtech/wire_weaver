//! Drives the bench supply like a test script would: sets a channel's limits, turns its output on, reads what it
//! measures, then asks for more current than the limit allows and shows the refusal.
//!
//! `cargo run -p bench_supply --example bench_supply_bench` against `bench_supply_sim` on this PC, or
//! `-- --url ws://ADDRESS:8081/ww` against another.

use anyhow::Result;
use bench_supply::BenchSupply;
use bench_supply_api::{Current, Voltage};
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:8081/ww")]
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let device = BenchSupply::config(|c| c.websocket_url(args.url.clone()).no_ww_usb())
        .connect()
        .await?;

    // --8<-- [start:set_and_read]
    let channel = device.channel(0);
    channel
        .write_voltage_limit(Voltage::bounded(5_000))
        .write()
        .await?;
    channel
        .write_current_limit(Current::bounded(1_000))
        .write()
        .await?;
    channel.set_output(true).call().await?;
    let measured = channel.read_measured().read().await?;
    println!("on: {} {}", measured.voltage, measured.current);
    // --8<-- [end:set_and_read]

    // --8<-- [start:refused]
    channel.set_output(false).call().await?;
    channel
        .write_current_limit(Current::bounded(100))
        .write()
        .await?;
    match channel.set_output(true).call().await {
        Ok(()) => println!("on"),
        Err(e) => println!("refused: {e}"),
    }
    // --8<-- [end:refused]

    device.disconnect().asynch().await?;
    Ok(())
}
