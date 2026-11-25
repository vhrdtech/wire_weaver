//! Finds blinky devices announced on the LAN over DNS-SD (`_wireweaver._tcp`), lists them, then blinks the first one.
//! The device side is `blinky_ws_device` on this PC or on another one.
//!
//! `cargo run -p blinky --features dns_sd --example blinky_discover`

use std::time::Duration;

use anyhow::{Context, Result, bail};
use blinky::Blinky;
use wire_weaver_client::discovery::{self, Discovery, Select};
use wire_weaver_client::{ConnectionState, KeepTrying};

#[tokio::main]
async fn main() -> Result<()> {
    // --8<-- [start:browse]
    let discovery = Discovery::start()?;
    // browsing is not instant: a record shows up when its announcement or an answer arrives
    tokio::time::sleep(Duration::from_secs(2)).await;
    for server in discovery.servers().iter() {
        println!(
            "{}: {:?} at {:?}, port {}",
            server.instance(),
            server.api(),
            server.addresses(),
            server.port()
        );
    }
    // --8<-- [end:browse]
    let Some(first) = discovery.servers().first().cloned() else {
        bail!("no blinky announced on this network");
    };

    // --8<-- [start:follow]
    // the address is looked up again at every attempt, so the device is followed through a new DHCP lease
    let device = Blinky::config(|c| {
        c.websocket_urls_from(discovery::urls(
            &discovery,
            Select::Name(first.instance().to_owned()),
            Vec::new(), // no saved address to fall back on
        ))
        .no_ww_usb()
        .keep_trying(KeepTrying::default())
    })
    .connect_keep_trying()?;
    // the handle exists before the link: wait for it, as a call before that fails at once
    let mut state = device
        .cmd()
        .connection_state()
        .context("a keep-trying connection has a state")?;
    state
        .wait_for(|s| matches!(s, ConnectionState::Connected { .. }))
        .await?;
    // --8<-- [end:follow]
    device.led_on().call().await?;
    println!("LED on at {}", first.instance());
    device.led_off().call().await?;
    device.disconnect().asynch().await?;
    Ok(())
}
