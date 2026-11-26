//! The settings example's reboot on tokio's paused clock: `meter_window` / `led_on_at_boot` set over the real
//! client/server codepath, written to a simulated flash once their debounce is over, and read back unchanged after
//! the device is rebuilt from the same flash image. The book shows what `settings_reboot` prints.

#[path = "../examples/reboot.rs"]
#[expect(dead_code, reason = "main runs from cargo run, not here")]
mod reboot_example;

/// What `settings_reboot` prints, the values of the second boot included.
#[tokio::test(start_paused = true)]
async fn book_output_settings_reboot() {
    let mut out = Vec::new();
    tokio::task::LocalSet::new()
        .run_until(reboot_example::run("settings/book_reboot", &mut out))
        .await
        .expect("runs");
    let printed = String::from_utf8(out).expect("prints text");
    assert!(
        printed.contains("second boot: meter_window 250 ms, led_on_at_boot true"),
        "{printed}"
    );
    book_outputs::check(
        "settings/reboot.txt",
        &format!("$ cargo run -p settings --example settings_reboot\n{printed}"),
    );
}

/// A change the device had no time to write is lost: a reboot before the debounce is over serves the defaults.
#[tokio::test(start_paused = true)]
async fn a_reboot_before_the_debounce_loses_the_change() {
    use settings_api::MeterWindow;
    tokio::task::LocalSet::new()
        .run_until(async {
            let path = "settings/early_reboot";
            let (device, stop) =
                reboot_example::boot(path, settings::Flash::new(0, settings::PAGES)).await;
            let client = reboot_example::connect(path).await.expect("connects");
            client
                .write_meter_window(MeterWindow::bounded(250))
                .write()
                .await
                .expect("writes");
            client.disconnect().asynch().await.expect("disconnects");
            _ = stop.send(());
            let flash = device.await.expect("runs").into_flash();

            let (device, stop) = reboot_example::boot(path, flash).await;
            let client = reboot_example::connect(path).await.expect("connects");
            let window = client.read_meter_window().read().await.expect("reads");
            assert_eq!(
                window.get(),
                100,
                "the default: the change never reached the flash"
            );
            client.disconnect().asynch().await.expect("disconnects");
            _ = stop.send(());
            device.await.expect("runs");
        })
        .await;
}
