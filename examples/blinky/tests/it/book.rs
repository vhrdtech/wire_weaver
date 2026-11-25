//! What the book shows of the blinky device, checked against `docs/generated/blinky/`: the `ww` transcripts are the
//! text `ww` prints, from the same device the chapters run, served in process.

use std::cell::Cell;

use blinky::Led;
use tokio::sync::oneshot;
use wire_weaver_cli::cmd::call::call;
use wire_weaver_cli::cmd::introspect::{IntrospectView, render};
use wire_weaver_client::{DynClient, in_process};

/// The URL a reader's simulated device serves at: `blinky_sim`'s default.
pub(crate) const SIM_URL: &str = "ws://127.0.0.1:8080/ww";

/// An LED nobody looks at.
struct DarkLed(Cell<bool>);

impl Led for DarkLed {
    fn set(&mut self, on: bool) {
        self.0.set(on);
    }
}

/// Runs `test` against a simulated blinky device at `path`, through the dynamic client `ww` and Python use.
pub(crate) async fn with_dyn_device<F: Future<Output = ()>>(
    path: &str,
    test: impl FnOnce(DynClient) -> F,
) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let led: &'static mut DarkLed = Box::leak(Box::new(DarkLed(Cell::new(false))));
    let device = local.spawn_local(blinky::simulated(in_process::ws_device(path), led, async {
        _ = stop_rx.await;
    }));
    local
        .run_until(async {
            let config = wire_weaver_client::ClientConfig::new()
                .in_process_path(path.to_owned())
                .no_ww_usb();
            let host = DynClient::from_config(config)
                .connect()
                .await
                .expect("connects");
            test(host).await;
            _ = stop_tx.send(());
            device.await.expect("the device runs to its stop");
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn book_output_introspect() {
    console::set_colors_enabled(false);
    with_dyn_device("blinky/book_introspect", |dev| async move {
        let tree = render(&dev, IntrospectView::default())
            .await
            .expect("renders");
        book_outputs::check(
            "blinky/introspect.txt",
            &format!("$ ww --websocket {SIM_URL} introspect\n{tree}"),
        );
    })
    .await;
}

/// `ww call` on the simulated device: a method without arguments, one with an argument and a return value, and one
/// whose return value is a count.
#[tokio::test(start_paused = true)]
async fn book_output_call() {
    console::set_colors_enabled(false);
    with_dyn_device("blinky/book_call", |dev| async move {
        let mut transcript = String::new();
        for what in ["led_on", "'set_led(false)'", "'set_led(true)'", "blinks"] {
            let printed = call(&dev, what.trim_matches('\'')).await.expect("calls");
            transcript.push_str(&format!(
                "$ ww --websocket {SIM_URL} call {what}\n{printed}\n"
            ));
        }
        book_outputs::check("blinky/call.txt", &transcript);
    })
    .await;
}
