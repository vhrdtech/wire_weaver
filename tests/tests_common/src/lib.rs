//! Runs a generated server on [ww_device::Server] in a device thread, connected to the host event loop through the
//! `in_process` transport. Tests drive it with a generated client, exactly as a real device over USB, link setup,
//! timeouts and streams included.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::{ApiHashPair, FullVersion};
use wire_weaver::{
    BlockingEventOut, EventOut, EventWriter, MessageSink, WireWeaverAsyncApiBackend,
};
use wire_weaver_client::in_process::{self, TokioClock};

/// Message buffers of the test device (`ww_device::Server` scratch, in-process link).
pub const MAX_MESSAGE_LEN: usize = 1024;

/// Implemented by test servers generated with `use_async = false`, forwards to the generated `process_request_bytes`.
pub trait TestProcessEvents {
    /// `medium = ".."` given to `ww_codegen!`, `()` by default.
    type Medium: Copy + Send + 'static;

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
        medium: Self::Medium,
    ) -> Result<&'a [u8], ShrinkWrapError>;
}

/// Implemented by test servers generated with `use_async = true`, forwards to the generated `process_request_bytes`.
pub trait TestProcessEventsAsync {
    /// `medium = ".."` given to `ww_codegen!`, `()` by default.
    type Medium: Copy + Send + 'static;

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl EventOut,
        medium: Self::Medium,
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;

    /// Called every millisecond between requests, `now_ms` since the device started: where a server with reliable
    /// streams calls its generated `poll_streams`.
    fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) -> impl Future<Output = ()> {
        let _ = (out, now_ms);
        async {}
    }
}

/// Decides whether a request is dropped (returns true) before the server sees it, see [TestDevice::filter_requests].
pub type RequestFilter = Box<dyn FnMut(&[u8]) -> bool + Send>;

/// Device running a test server, stopped when dropped.
pub struct TestDevice {
    path: String,
    drop_requests: Arc<AtomicBool>,
    filter: Arc<Mutex<Option<RequestFilter>>>,
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

    /// Look at every request before the server does, drop the ones `filter` returns true for, as a lossy medium
    /// would. Requests are serialized `ww_client_server::Request`s.
    pub fn filter_requests(&self, filter: impl FnMut(&[u8]) -> bool + Send + 'static) {
        *self.filter.lock().unwrap() = Some(Box::new(filter));
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
pub fn start_device<S: TestProcessEvents<Medium = ()> + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start_device_on(path, server, version, api_hash, ())
}

/// Same as [start_device], for servers generated with `medium = ".."`: `medium` is passed to every handler.
pub fn start_device_on<S: TestProcessEvents + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    medium: S::Medium,
) -> TestDevice {
    start(path, SyncServer(server), version, api_hash, medium)
}

/// Same as [start_device], for servers generated with `use_async = true`.
pub fn start_device_async<S: TestProcessEventsAsync<Medium = ()> + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start(path, AsyncServer(server), version, api_hash, ())
}

/// Sync or async server
trait Process {
    type Medium: Copy + Send + 'static;

    fn process<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: Self::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;

    fn poll(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        now_ms: u64,
    ) -> impl Future<Output = ()>;
}

struct SyncServer<S>(S);

impl<S: TestProcessEvents> Process for SyncServer<S> {
    type Medium = S::Medium;

    async fn process<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: S::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        // sync server: events sent by handlers are queued and sent out after it returns
        out.queued(|queue| self.0.process_request_bytes(data, scratch, queue, medium))
            .await
    }

    async fn poll(&mut self, _out: &mut EventWriter<'_, impl MessageSink>, _now_ms: u64) {}
}

struct AsyncServer<S>(S);

impl<S: TestProcessEventsAsync> Process for AsyncServer<S> {
    type Medium = S::Medium;

    async fn process<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: S::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.0
            .process_request_bytes(data, scratch, out, medium)
            .await
    }

    async fn poll(&mut self, out: &mut EventWriter<'_, impl MessageSink>, now_ms: u64) {
        self.0.poll(out, now_ms).await
    }
}

fn start<S: Process + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    medium: S::Medium,
) -> TestDevice {
    // a device returning an error outside a method's #[errors(..)] fails the test (API-32)
    wire_weaver_client::set_strict_error_lists(true);
    let drop_requests = Arc::new(AtomicBool::new(false));
    let filter = Arc::new(Mutex::new(None));
    let (to_device, mut from_test) = mpsc::unbounded_channel::<Vec<u8>>();
    let (tx, rx) = in_process::device(path, MAX_MESSAGE_LEN);
    let mut backend = TestBackend {
        server,
        drop_requests: drop_requests.clone(),
        filter: filter.clone(),
        version,
    };
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut scratch = [0u8; MAX_MESSAGE_LEN];
            let mut event_scratch = [0u8; MAX_MESSAGE_LEN];
            let config =
                ww_device::LinkConfig::new(version, api_hash, ww_client_server::COMPACT_VERSION);
            let mut server = ww_device::Server::new(
                config,
                tx,
                rx,
                TokioClock::new(),
                &mut scratch,
                &mut event_scratch,
            )
            .with_medium(medium);
            let started = std::time::Instant::now();
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(1));
            loop {
                tokio::select! {
                    ready = server.wait() => {
                        _ = server.handle(ready, &mut backend).await;
                    }
                    _ = tick.tick() => {
                        if server.is_up() {
                            let now_ms = started.elapsed().as_millis() as u64;
                            backend.server.poll(&mut server.sink(), now_ms).await;
                        }
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
        filter,
        to_device,
    }
}

struct TestBackend<S> {
    server: S,
    drop_requests: Arc<AtomicBool>,
    filter: Arc<Mutex<Option<RequestFilter>>>,
    version: FullVersion<'static>,
}

impl<S: Process> WireWeaverAsyncApiBackend for TestBackend<S> {
    type Medium = S::Medium;

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: S::Medium,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        if self.drop_requests.load(Ordering::Relaxed) {
            return Ok(&[]);
        }
        if let Some(filter) = self.filter.lock().unwrap().as_mut()
            && filter(data)
        {
            return Ok(&[]);
        }
        self.server.process(out, medium, data, scratch).await
    }

    fn version(&self) -> FullVersion<'_> {
        self.version
    }
}

/// Poll a promise from synchronous code until it is done or failed, panics after 5 s.
pub fn wait_promise<T: DeserializeShrinkWrapOwned, E: wire_weaver_client::OutcomeError>(
    promise: &mut wire_weaver_client::Promise<T, E>,
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
