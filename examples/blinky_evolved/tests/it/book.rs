//! What the book's chapter "Changing your API" shows, checked against `docs/generated/blinky_evolved/`: old and new
//! hosts against old and new firmware, served in process, and the checker's verdict on the change.

use std::future::Future;
use std::path::Path;

use tokio::sync::oneshot;
use wire_weaver_cli::cmd::api::check::{self, CheckOptions};
use wire_weaver_client::in_process;

/// Which firmware a test serves.
#[derive(Clone, Copy)]
enum Firmware {
    W1_0,
    W1_1,
}

impl Firmware {
    fn label(self) -> &'static str {
        match self {
            Firmware::W1_0 => "firmware 0.1.0 (w1.0)",
            Firmware::W1_1 => "firmware 0.1.1 (w1.1)",
        }
    }
}

/// Runs `host` against `firmware` served at the in-process `path`; returns what `host` reported, and whether the LED
/// was on at the end.
async fn with_firmware<F: Future<Output = String>>(
    firmware: Firmware,
    path: &str,
    host: impl FnOnce(String) -> F,
) -> (String, bool) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let stop = async {
        _ = stop_rx.await;
    };
    let port = in_process::ws_device(path);
    let device = local.spawn_local(async move {
        match firmware {
            Firmware::W1_0 => blinky_evolved::device::serve_w1_0(port, stop).await,
            Firmware::W1_1 => blinky_evolved::device::serve_w1_1(port, stop).await,
        }
    });
    local
        .run_until(async {
            let report = host(path.to_owned()).await;
            _ = stop_tx.send(());
            let led = device.await.expect("the firmware runs to its stop");
            (report, led)
        })
        .await
}

/// The host built from `blinky_api` 0.1.0: it knows everything but `led_toggle`.
async fn old_host(path: String) -> String {
    let device = blinky::Blinky::config(|c| c.in_process_path(path.clone()).no_ww_usb())
        .connect()
        .await
        .expect("connects");
    device.led_on().call().await.expect("led_on");
    device.disconnect().asynch().await.expect("disconnects");
    "  led_on: done\n".to_owned()
}

/// The host built from `blinky_api` 0.1.1: it knows `led_toggle` too.
async fn new_host(path: String) -> String {
    let device = blinky_evolved::Blinky::config(|c| c.in_process_path(path.clone()).no_ww_usb())
        .connect()
        .await
        .expect("connects");
    device.led_on().call().await.expect("led_on");
    // --8<-- [start:toggle]
    let toggle = match device.led_toggle().call().await {
        Ok(()) => "done".to_owned(),
        Err(e) => format!("refused: {e}"),
    };
    // --8<-- [end:toggle]
    device.disconnect().asynch().await.expect("disconnects");
    format!("  led_on: done\n  led_toggle: {toggle}\n")
}

#[tokio::test(start_paused = true)]
async fn book_output_versions() {
    let mut out = String::new();
    for firmware in [Firmware::W1_0, Firmware::W1_1] {
        let (report, led) = with_firmware(firmware, "blinky/old_host", old_host).await;
        out.push_str(&format!(
            "host 0.1.0 and {}\n{report}  the LED is {}\n",
            firmware.label(),
            on_off(led)
        ));
    }
    for firmware in [Firmware::W1_0, Firmware::W1_1] {
        let (report, led) = with_firmware(firmware, "blinky/new_host", new_host).await;
        out.push_str(&format!(
            "host 0.1.1 and {}\n{report}  the LED is {}\n",
            firmware.label(),
            on_off(led)
        ));
    }
    book_outputs::check("blinky_evolved/versions.txt", &out);
}

fn on_off(led: bool) -> &'static str {
    if led { "on" } else { "off" }
}

/// `ww api check` on the changed API against the snapshot of the version in the field.
#[test]
fn book_output_check() {
    console::set_colors_enabled(false);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::set_current_dir(&root).expect("the repository root");
    let crate_path = "examples/blinky_api_evolved";
    let against = "examples/blinky_api/api_snapshots/blinky_api_0_1_0.ron";
    let checked = check::render(
        crate_path.into(),
        CheckOptions {
            against: Some(against.into()),
            ..CheckOptions::default()
        },
    )
    .expect("checks");
    assert_eq!(checked.verdict, Ok(()));
    book_outputs::check(
        "blinky_evolved/check.txt",
        &format!(
            "$ ww api check {crate_path} --against {against}\n{}",
            checked.text
        ),
    );
}

/// The changed API's own snapshot is current: what its `tests/evolution.rs` would check, if the crate were a
/// member of the workspace.
#[test]
fn the_changed_api_matches_its_snapshot() {
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../blinky_api_evolved");
    wire_weaver_evolution::Check::new(crate_path)
        .require_current(true)
        .assert();
}
