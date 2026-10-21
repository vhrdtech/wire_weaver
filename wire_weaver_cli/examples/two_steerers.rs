//! Two steerers on one device (SRV-31), a bench check of a board's `Clock::steer`: steerer A is followed, B only
//! noted while A's points keep coming, and B takes over once A stopped for the device's holdover time.
//!
//! ```sh
//! cargo run -p wire_weaver_cli --example two_steerers -- <serial or label part>
//! ```
//!
//! Both steer over one direct connection (not through a ww hub, which would steer itself), told apart by their
//! steerer ids, as two programs would be; the connection's own steering (CLT-46) is off.

use anyhow::{Context, Result, bail};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use wire_weaver_client::clock_sync::{ClockSync, Options, Status};
use wire_weaver_client::hub::HubMode;
use wire_weaver_client::{ClientConfig, DynClient};
use ww_timebase::SteererOwned;

#[tokio::main]
async fn main() -> Result<()> {
    let wanted = std::env::args()
        .nth(1)
        .context("usage: two_steerers <serial or label part>")?
        .to_lowercase();
    let listed = wire_weaver_client::list_usb_devices().await?;
    let matching: Vec<_> = listed
        .iter()
        .filter(|d| {
            d.user_label.to_lowercase().contains(&wanted)
                || d.serials.iter().any(|s| s.to_lowercase().contains(&wanted))
        })
        .collect();
    let [device] = matching[..] else {
        bail!("{} devices match '{wanted}', want one", matching.len());
    };
    let usb = device.usb.as_ref().context("not a USB device")?;
    let config = ClientConfig::new()
        .traits_only()
        .hub(HubMode::Direct)
        .clock_sync(false)
        .usb_port_chain(usb.bus_id.clone(), usb.port_chain.clone());
    let client = DynClient::from_config(config).connect().await?;
    println!("{} {:?}", device.product, device.serials);

    let named = |id: u64, program: &str| Options {
        steerer: Some(SteererOwned {
            id,
            host: "bench".into(),
            program: program.into(),
            pid: std::process::id(),
        }),
        ..Options::default()
    };
    let started = Instant::now();
    let a = ClockSync::start(client.cmd(), named(0xA, "steerer A")).await?;
    let mut watch_a = a.watch();
    let s = wait(&mut watch_a, "A followed", |s| s.followed == Some(true)).await?;
    println!(
        "A followed after {:.1} s: {}",
        started.elapsed().as_secs_f32(),
        device_line(&s)
    );
    wait(&mut watch_a, "A locked", |s| {
        s.device
            .as_ref()
            .is_some_and(|d| d.state == ww_timebase::SyncState::Locked)
    })
    .await?;

    let b = ClockSync::start(client.cmd(), named(0xB, "steerer B")).await?;
    let mut watch_b = b.watch();
    let s = wait(&mut watch_b, "B noted", |s| {
        s.followed == Some(false) && s.device.as_ref().is_some_and(|d| d.noted.is_some())
    })
    .await?;
    println!("B only noted: {}", device_line(&s));
    // A keeps the device while it steers: B stays noted for a while
    tokio::time::sleep(Duration::from_secs(10)).await;
    let s = watch_b.borrow().clone();
    println!("10 s later: {}", device_line(&s));
    if s.followed != Some(false) {
        bail!("B took over while A was still steering");
    }

    a.stop();
    let stopped = Instant::now();
    let s = wait(&mut watch_b, "B followed", |s| s.followed == Some(true)).await?;
    println!(
        "A stopped, B followed after {:.1} s: {}",
        stopped.elapsed().as_secs_f32(),
        device_line(&s)
    );
    let s = wait(&mut watch_b, "B locked", |s| {
        s.device
            .as_ref()
            .is_some_and(|d| d.state == ww_timebase::SyncState::Locked)
    })
    .await?;
    println!(
        "locked on B {:.1} s after A stopped: {}",
        stopped.elapsed().as_secs_f32(),
        device_line(&s)
    );
    b.stop();
    _ = client.disconnect().asynch().await;
    Ok(())
}

async fn wait(
    watch: &mut watch::Receiver<Status>,
    what: &str,
    f: impl FnMut(&Status) -> bool,
) -> Result<Status> {
    let s = tokio::time::timeout(Duration::from_secs(30), watch.wait_for(f))
        .await
        .with_context(|| format!("{what}: not within 30 s"))??;
    Ok(s.clone())
}

/// The device's own view: state, source, whom it follows and whom it noted.
fn device_line(s: &Status) -> String {
    let Some(d) = &s.device else {
        return "no status yet".into();
    };
    let who = |st: &Option<SteererOwned>| match st {
        Some(st) => format!("{} ({:#x}, {}, pid {})", st.program, st.id, st.host, st.pid),
        None => "-".into(),
    };
    format!(
        "{:?} source {:?}, steerer {}, noted {}",
        d.state,
        d.source,
        who(&d.steerer),
        who(&d.noted)
    )
}
