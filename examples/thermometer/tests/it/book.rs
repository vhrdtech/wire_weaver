//! What the book shows of the thermometer, checked against `docs/generated/thermometer/`: the `ww` transcripts are
//! the text `ww` prints, from the same device the chapters run, served in process.

use thermometer_api::Gain;
use tokio::sync::oneshot;
use wire_weaver_cli::cmd::introspect::{IntrospectView, render};
use wire_weaver_client::{DynClient, Error, in_process};
use ww_client_server::{ErrorKindOwned, ErrorOwned, PathKind};

/// The URL a reader's simulated device serves at: `thermometer_sim`'s default.
const SIM_URL: &str = "ws://127.0.0.1:8080/ww";

/// Runs `test` against a simulated thermometer at `path`, through the dynamic client `ww` and Python use.
async fn with_dyn_device<F: Future<Output = ()>>(path: &str, test: impl FnOnce(DynClient) -> F) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let incoming = in_process::ws_device(path);
    let device = local.spawn_local(thermometer::simulated(incoming, async {
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
    with_dyn_device("thermometer/book_introspect", |dev| async move {
        let view = IntrospectView {
            skip_docs: true,
            ..IntrospectView::default()
        };
        let tree = render(&dev, view).await.expect("renders");
        book_outputs::check(
            "thermometer/introspect.txt",
            &format!("$ ww --websocket {SIM_URL} introspect -d\n{tree}"),
        );
    })
    .await;
}

/// A value outside a property's bounds never reaches its handler: the device refuses the bytes when it decodes them.
/// The typed client can't even build one (`Gain::try_new(20)` fails), so the test writes the bytes itself.
#[tokio::test(start_paused = true)]
async fn the_device_refuses_a_gain_out_of_bounds() {
    assert!(Gain::try_new(20).is_err());
    with_dyn_device("thermometer/gain_bounds", |dev| async move {
        let root = dev
            .device_introspect()
            .expect("introspection data")
            .api_bundle;
        let gain = wire_weaver_client::DynResource::root(root)
            .child("gain")
            .expect("gain");
        let path = gain.index_chain().to_vec();
        let write = |byte: u8| {
            dev.cmd()
                .prepare_write::<Error>(
                    PathKind::Absolute {
                        path: wire_weaver::prelude::RefVec::Slice { slice: &path },
                    },
                    Ok(vec![byte]),
                    "gain",
                )
                .write()
        };
        let refused = write(20).await;
        assert!(
            matches!(
                &refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::PropertyDesFailed,
                    ..
                }))
            ),
            "{refused:?}"
        );
        write(8).await.expect("8 is in bounds");
    })
    .await;
}
