//! Helpers for tests of the tap: a live tap on a device printed as `--monitor=api` does and captured to a file, and
//! the capture read back as `ww explain FILE` would print it.

use super::{FileFeed, Layer, ViewArgs, replay};
use crate::cmd::tap::{Attached, TapFlags};
use std::io::Write;
use std::sync::Arc;
use wire_weaver_client::{ClientConfig, DynClient};

/// A `Write` the test reads back.
#[derive(Clone, Default)]
pub struct Shared(pub Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A live tap on the in-process device at `device_path`, printed as `--monitor=api` does and captured to a file:
/// the monitor's text and the file. `traits_only`: the client is connected as `--traits-only` does.
pub async fn tapped_session(
    name: &str,
    device_path: String,
    traits_only: bool,
    session: impl AsyncFnOnce(&DynClient),
) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let flags = TapFlags {
        monitor: Some(Layer::Api),
        tap_bytes: false,
        tap_bits: false,
        capture: Some(path.clone()),
    };
    let printed = Shared::default();
    let attached = Attached::start_with(&flags, Box::new(printed.clone())).unwrap();
    let config = ClientConfig::new()
        .in_process_path(device_path)
        .tap(attached.recorder());
    let config = if traits_only {
        config.traits_only()
    } else {
        config
    };
    let client = DynClient::from_config(config)
        .connect()
        .await
        .expect("connect");
    attached.add_bundles(&client);
    session(&client).await;
    client.disconnect().asynch().await.unwrap();
    attached.finish().unwrap();
    let printed = String::from_utf8(printed.0.lock().unwrap().clone()).unwrap();
    (printed, path)
}

/// What `ww explain FILE` prints of a capture.
pub fn replayed(path: &std::path::Path, layer: Layer) -> String {
    replayed_view(path, layer, false)
}

/// What `ww explain FILE` prints of a capture, with `--bytes` when `bytes`.
pub fn replayed_view(path: &std::path::Path, layer: Layer, bytes: bool) -> String {
    let view = ViewArgs {
        layer,
        bytes,
        bits: false,
    };
    let mut out = vec![];
    replay(
        &mut FileFeed::open(path, false).unwrap(),
        view,
        false,
        &mut out,
    )
    .unwrap();
    String::from_utf8(out).unwrap()
}
