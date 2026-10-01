//! Runs a generated server on [ww_device::Server] in a device thread, connected to the host event loop through the
//! `in_process` transport. Tests drive it with a generated client, exactly as a real device over USB, link setup,
//! timeouts and streams included.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::{ApiHashPair, FullVersion};
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{self, TokioClock};

const MAX_MESSAGE_LEN: usize = 1024;

/// Implemented by test servers generated with `use_async = false`, forwards to the generated `process_request_bytes`.
pub trait TestProcessEvents {
    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        msg_tx: &mut impl MessageSink,
    ) -> Result<&'a [u8], ShrinkWrapError>;
}

/// Device running a test server, stopped when dropped.
pub struct TestDevice {
    path: String,
    drop_requests: Arc<AtomicBool>,
    to_device: mpsc::UnboundedSender<Vec<u8>>,
}

impl TestDevice {
    /// Path to connect to: `Client::config(|c| c.in_process_path(device.path()))`.
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// Ignore all requests from now on, without replying, as if a device was stuck.
    pub fn drop_requests(&self, drop: bool) {
        self.drop_requests.store(drop, Ordering::Relaxed);
    }

    /// Send a serialized `ww_client_server::Event` to the host, e.g., stream data from generated `stream_data_ser()`.
    pub fn send(&self, event: &[u8]) {
        self.to_device
            .send(event.to_vec())
            .expect("device is running");
    }
}

/// Start a device thread serving `server` under `path` (unique per test).
/// `version` is the API crate's `<TRAIT>_FULL_GID`, `api_hash` is the server's generated `api_hash()`.
pub fn start_device<S: TestProcessEvents + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    let drop_requests = Arc::new(AtomicBool::new(false));
    let (to_device, mut from_test) = mpsc::unbounded_channel::<Vec<u8>>();
    let (tx, rx) = in_process::device(path, MAX_MESSAGE_LEN);
    let mut backend = TestBackend {
        server,
        drop_requests: drop_requests.clone(),
        version,
    };
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut scratch = [0u8; MAX_MESSAGE_LEN];
            let config =
                ww_device::LinkConfig::new(version, api_hash, ww_client_server::COMPACT_VERSION);
            let mut server =
                ww_device::Server::new(config, tx, rx, TokioClock::new(), &mut scratch);
            loop {
                tokio::select! {
                    ready = server.wait() => {
                        _ = server.handle(ready, &mut backend).await;
                    }
                    event = from_test.recv() => match event {
                        Some(event) => server.send(&event).await.expect("send event to host"),
                        None => break, // TestDevice dropped
                    }
                }
            }
        });
    });
    TestDevice {
        path: path.to_string(),
        drop_requests,
        to_device,
    }
}

struct TestBackend<S> {
    server: S,
    drop_requests: Arc<AtomicBool>,
    version: FullVersion<'static>,
}

impl<S: TestProcessEvents> WireWeaverAsyncApiBackend for TestBackend<S> {
    async fn process_bytes<'a>(
        &mut self,
        sink: &mut impl MessageSink,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        if self.drop_requests.load(Ordering::Relaxed) {
            return Ok(&[]);
        }
        self.server.process_request_bytes(data, scratch, sink)
    }

    fn version(&self) -> FullVersion<'_> {
        self.version
    }
}

/// Poll a promise from synchronous code until it is done or failed, panics after 5 s.
pub fn wait_promise<T: DeserializeShrinkWrapOwned + core::fmt::Debug>(
    promise: &mut wire_weaver_client::Promise<T>,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        promise.sync_poll();
        if promise.is_ready() || promise.is_err() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "promise is still pending"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
