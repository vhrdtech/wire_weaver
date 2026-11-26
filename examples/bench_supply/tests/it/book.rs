//! What the book shows of the bench supply, checked against `docs/generated/bench_supply/`: what the example
//! programs print, and the `ww` transcripts, the text `ww` prints, from the same device served in process.

use std::cell::Cell;
use std::rc::Rc;

use tokio::sync::oneshot;
use wire_weaver_cli::cmd::call::call;
use wire_weaver_cli::cmd::introspect::{IntrospectView, render};
use wire_weaver_client::{DynClient, in_process};

/// The URL a reader's simulated device serves at: `bench_supply_sim`'s default.
const SIM_URL: &str = "ws://127.0.0.1:8081/ww";

/// Runs `test` against a simulated bench supply at `path`, through the dynamic client `ww` and Python use.
async fn with_dyn_device<F: Future<Output = ()>>(path: &str, test: impl FnOnce(DynClient) -> F) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let incoming = in_process::ws_device(path);
    let device = local.spawn_local(bench_supply::simulated(incoming, async {
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

/// What `bench_supply_errors` prints against a simulated device that just started.
#[tokio::test(start_paused = true)]
async fn book_output_errors() {
    let printed = Rc::new(Cell::new(Vec::new()));
    let sink = printed.clone();
    crate::with_device("bench_supply/book_errors", |device| async move {
        let mut out = Vec::new();
        crate::errors_example::run(&device, &mut out)
            .await
            .expect("runs");
        sink.set(out);
    })
    .await;
    let printed = String::from_utf8(printed.take()).expect("prints text");
    book_outputs::check(
        "bench_supply/errors.txt",
        &format!("$ cargo run -p bench_supply --example bench_supply_errors\n{printed}"),
    );
}

/// `ww introspect -d` of the bench supply.
#[tokio::test(start_paused = true)]
async fn book_output_introspect() {
    console::set_colors_enabled(false);
    with_dyn_device("bench_supply/book_introspect", |dev| async move {
        let view = IntrospectView {
            skip_docs: true,
            ..IntrospectView::default()
        };
        let tree = render(&dev, view).await.expect("renders");
        book_outputs::check(
            "bench_supply/introspect.txt",
            &format!("$ ww --websocket {SIM_URL} introspect -d\n{tree}"),
        );
    })
    .await;
}

/// `ww call` on a refusal: the device's own error and a common error, each as the `ww` binary prints it.
#[tokio::test(start_paused = true)]
async fn book_output_call_errors() {
    with_dyn_device("bench_supply/book_call", |dev| async move {
        let mut transcript = String::new();
        for what in [
            "'channel[0].set_output(true)'",
            "'channel[0].voltage_limit = 5000'",
            "'channel[0].current_limit = 1000'",
            "'channel[0].set_output(true)'",
            "'channel[0].voltage_limit = 6000'",
        ] {
            let printed = match call(&dev, what.trim_matches('\'')).await {
                Ok(printed) => printed,
                Err(e) => format!("Error: {e:?}"),
            };
            transcript.push_str(&format!(
                "$ ww --websocket {SIM_URL} call {what}\n{printed}\n"
            ));
        }
        book_outputs::check("bench_supply/call_errors.txt", &transcript);
    })
    .await;
}
