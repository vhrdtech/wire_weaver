//! Reads the thermometer once a second and says what the connection does, through a device that goes away: stop
//! `thermometer_sim` with Ctrl-C and start it again.
//!
//! `cargo run -p thermometer --example thermometer_keep_trying`, with `thermometer_sim` running in another terminal.

use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use thermometer::Thermometer;
use wire_weaver_client::{ConnectionEvent, KeepTrying};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:8080/ww")]
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // --8<-- [start:connect]
    // returns at once: every attempt, the first one too, runs in the background
    let device = Thermometer::config(|c| {
        c.websocket_url(args.url.clone())
            .no_ww_usb()
            .keep_trying(KeepTrying::default())
    })
    .connect_keep_trying()?;
    // --8<-- [end:connect]

    // --8<-- [start:events]
    let mut events = device.cmd().connection_events();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event {
                ConnectionEvent::Connecting { attempt } => println!("trying, attempt {attempt}"),
                ConnectionEvent::AttemptFailed { retry_in, .. } => {
                    println!("failed, next attempt in {} ms", retry_in.as_millis())
                }
                ConnectionEvent::Connected { address, .. } => println!("connected to {address}"),
                ConnectionEvent::Disconnected { .. } => println!("the link is gone"),
                ConnectionEvent::Reconnected { attempts, .. } => {
                    println!("connected again, after {attempts} attempts")
                }
                ConnectionEvent::GaveUp { error } => println!("gave up: {error}"),
                _ => {}
            }
        }
    });
    // --8<-- [end:events]

    // --8<-- [start:calls]
    loop {
        match device.read_once().call().await {
            Ok(sample) => println!("input {}: {}", sample.input.get(), sample.temp),
            // while there is no link, a call fails at once instead of waiting
            Err(e) => println!("no reading: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // --8<-- [end:calls]
}
