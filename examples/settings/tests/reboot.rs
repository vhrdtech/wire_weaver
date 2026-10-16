//! `meter_window_ms` / `led_on_at_boot` set over the real client/server codepath, written to a simulated flash
//! once their debounce is over, and read back unchanged after the device is rebuilt from the same flash image (a
//! reboot) - see `docs/api/settings.md` and `examples/settings/src/lib.rs`.

use std::time::Duration;

use settings::{Client, Debounce, Device, Flash, PAGES};

/// Starts a device thread under `path` serving `flash`, returns a handle that joins into the flash image once
/// `stop` fires (`ww_device` futures are not `Send`, so it needs its own current-thread runtime).
fn spawn_device(
    path: &str,
    flash: Flash,
    debounce: Debounce,
) -> (
    std::thread::JoinHandle<Flash>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (tx, rx) = wire_weaver_client::in_process::device(path, 256);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let handle = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let device = Device::new(flash, debounce).await;
                let device = settings::serve(device, tx, rx, async {
                    let _ = stop_rx.await;
                })
                .await;
                device.into_flash()
            })
    });
    (handle, stop_tx)
}

async fn connect(path: &str) -> Client {
    Client::config(|c| c.in_process_path(path.to_string()))
        .connect()
        .await
        .expect("connect")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_persist_across_reboot() {
    let path = "examples/settings/reboot";
    let debounce = Debounce {
        quiet_ms: 20,
        max_ms: 100,
    };

    let (handle, stop) = spawn_device(path, Flash::new(0, PAGES), debounce);
    let client = connect(path).await;
    assert_eq!(client.read_meter_window_ms().read().await.unwrap(), 100);
    assert!(!client.read_led_on_at_boot().read().await.unwrap());

    client
        .write_meter_window_ms(250)
        .write()
        .await
        .expect("write meter_window_ms");
    client
        .write_led_on_at_boot(true)
        .write()
        .await
        .expect("write led_on_at_boot");
    // past the debounce: the next tick of `serve` writes both to the fake flash
    tokio::time::sleep(Duration::from_millis(150)).await;

    client.disconnect().asynch().await.expect("disconnect");
    stop.send(()).expect("device still running");
    let flash = handle.join().expect("device thread");

    // a reboot: a new Device built from the same flash image
    let (handle, stop) = spawn_device(path, flash, Debounce::default());
    let client = connect(path).await;
    assert_eq!(
        client.read_meter_window_ms().read().await.unwrap(),
        250,
        "meter_window_ms survived the reboot"
    );
    assert!(
        client.read_led_on_at_boot().read().await.unwrap(),
        "led_on_at_boot survived the reboot"
    );

    client.disconnect().asynch().await.expect("disconnect");
    stop.send(()).expect("device still running");
    handle.join().expect("device thread");
}
