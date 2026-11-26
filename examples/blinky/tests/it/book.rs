//! What the book shows of the blinky device, checked against `docs/generated/blinky/`: the `ww` transcripts are the
//! text `ww` prints, and the host program's output is what `blinky_host` prints, from the same device the chapters
//! run, served in process, and the handlers `ww api scaffold` writes for its API.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use blinky::Led;
use tokio::sync::oneshot;
use wire_weaver_cli::cmd::api::scaffold;
use wire_weaver_cli::cmd::call::call;
use wire_weaver_cli::cmd::introspect::{IntrospectView, render};
use wire_weaver_client::{DynClient, in_process};
use wire_weaver_core::ServerScaffoldConfig;

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
        // an argument of the wrong type never leaves the host
        assert!(call(&dev, "set_led(1)").await.is_err());
        assert_eq!(call(&dev, "blinks").await.expect("calls"), "2 item");
    })
    .await;
}

/// What `blinky_host` prints against a simulated device that just started: its `run`, the code the chapters show.
/// The board run of the same program is a manual step of the chapter "Running on a board".
#[tokio::test(start_paused = true)]
async fn book_output_host() {
    let printed = Rc::new(Cell::new(Vec::new()));
    let sink = printed.clone();
    crate::with_device("blinky/book_host", |device| async move {
        let mut out = Vec::new();
        crate::host::run(&device, &mut out).await.expect("runs");
        sink.set(out);
    })
    .await;
    let printed = String::from_utf8(printed.take()).expect("prints text");
    book_outputs::check(
        "blinky/host.txt",
        &format!("$ cargo run -p blinky --example blinky_host\n{printed}"),
    );
}

/// `ww api scaffold examples/blinky_api --use-async`: the options the CLI takes when only `--use-async` is given.
#[test]
fn book_output_scaffold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::set_current_dir(&root).expect("the repository root");
    let config = ServerScaffoldConfig {
        server_struct: "Server".to_owned(),
        no_alloc: true,
        use_async: true,
        handler_model: String::new(),
        receiver_model: String::new(),
        method_model: String::new(),
        property_model: String::new(),
        const_model: String::new(),
        medium: None,
        omit: vec![],
        observe_cells: vec![],
        observe_samples: true,
        introspect: String::new(),
        introspect_compression: String::new(),
    };
    let scaffold =
        scaffold::render("examples/blinky_api".into(), None, &config, false).expect("scaffolds");
    assert!(scaffold.hints.is_empty(), "{:?}", scaffold.hints);
    book_outputs::check("blinky/scaffold.txt", &scaffold.code);
}
