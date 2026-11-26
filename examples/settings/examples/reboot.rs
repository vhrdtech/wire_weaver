//! Two settings across a reboot, on a simulated flash: the device boots on its defaults, a host changes both
//! settings, the device writes them to flash once they settle, and after a reboot from the same flash it serves the
//! new values.
//!
//! `cargo run -p settings --example settings_reboot`

use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use settings::{Client, Debounce, Device, Flash, PAGES};
use settings_api::MeterWindow;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Where the device serves, in this process.
const PATH: &str = "settings/reboot";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(run(PATH, &mut io::stdout()))
        .await
}

/// Boots the device twice from one flash, one line per step.
pub async fn run(path: &str, out: &mut impl Write) -> Result<()> {
    // --8<-- [start:first_boot]
    let flash = Flash::new(0, PAGES);
    let (device, stop) = boot(path, flash).await;
    let client = connect(path).await?;
    print_settings(&client, "first boot", out).await?;
    client
        .write_meter_window(MeterWindow::bounded(250))
        .write()
        .await?;
    client.write_led_on_at_boot(true).write().await?;
    writeln!(out, "host writes meter_window 250 ms, led_on_at_boot true")?;
    // --8<-- [end:first_boot]

    // --8<-- [start:reboot]
    // the debounce: the device writes once the values were left alone for a second
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    client.disconnect().asynch().await?;
    _ = stop.send(());
    let flash = device.await?.into_flash();
    writeln!(out, "power off, power on")?;

    let (device, stop) = boot(path, flash).await;
    let client = connect(path).await?;
    print_settings(&client, "second boot", out).await?;
    // --8<-- [end:reboot]

    client.disconnect().asynch().await?;
    _ = stop.send(());
    device.await?;
    Ok(())
}

/// Starts a device on `flash`, loading its settings from it; the device runs until the sender fires.
pub async fn boot(path: &str, flash: Flash) -> (JoinHandle<Device>, oneshot::Sender<()>) {
    let device = Device::new(flash, Debounce::default()).await;
    let (tx, rx) = wire_weaver_client::in_process::device(path, 256);
    let (stop_tx, stop_rx) = oneshot::channel();
    let handle = tokio::task::spawn_local(settings::serve(device, tx, rx, async {
        _ = stop_rx.await;
    }));
    (handle, stop_tx)
}

pub async fn connect(path: &str) -> Result<Client> {
    let client = Client::config(|c| c.in_process_path(path.to_owned()).no_ww_usb())
        .connect()
        .await?;
    Ok(client)
}

async fn print_settings(client: &Client, when: &str, out: &mut impl Write) -> Result<()> {
    let window = client.read_meter_window().read().await?;
    let led = client.read_led_on_at_boot().read().await?;
    writeln!(out, "{when}: meter_window {window}, led_on_at_boot {led}")?;
    Ok(())
}
