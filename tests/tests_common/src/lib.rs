//! Runs a generated server on [ww_device::Server] in a device thread, connected to the host event loop through the
//! `in_process` transport. Tests drive it with a generated client, exactly as a real device over USB, link setup,
//! timeouts and streams included.

use std::pin::Pin;
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

    /// An event-driven device (SRV-25): what the device waits for before its next [poll](Self::poll), asked anew
    /// before every wait (its next-wake timer on a [FakeClock], its wake signal). `None`, the default: a poll every
    /// millisecond.
    fn wakeup(&mut self) -> Option<Wakeup> {
        None
    }
}

/// What an event-driven test device waits for before its next poll, see [TestProcessEventsAsync::wakeup].
pub type Wakeup = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A microsecond clock the test moves, with timers on it: the device's clock for observe tests.
#[derive(Clone)]
pub struct FakeClock(Arc<tokio::sync::watch::Sender<u64>>);

impl Default for FakeClock {
    fn default() -> Self {
        FakeClock(Arc::new(tokio::sync::watch::Sender::new(0)))
    }
}

impl FakeClock {
    pub fn now(&self) -> u64 {
        *self.0.borrow()
    }

    /// Moves the clock to `us` (never back), firing the timers that are due.
    pub fn set(&self, us: u64) {
        self.0.send_modify(|t| *t = (*t).max(us));
    }

    pub fn advance(&self, us: u64) {
        self.0.send_modify(|t| *t += us);
    }

    /// Resolves once the clock is at `at` or later.
    pub fn until(&self, at: u64) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.0.subscribe();
        async move {
            _ = rx.wait_for(|t| *t >= at).await;
        }
    }
}

/// Two servers of one API, generated with different options, answering the same requests: `a` answers the host,
/// `b` is the reference, given every request too, and has to answer with the same bytes and send the same events.
/// `b`'s events go nowhere, each of its sends gets the result `a`'s send of the same event had (a full queue
/// fails both). With `a` a `no_alloc = false` server and `b` a `no_alloc` one (API-21), a test suite written for one
/// runs against the other and checks they are the same on the wire. Give each its own state (handlers of both run),
/// sharing only what a test sets to steer the handlers.
pub struct Twin<A, B> {
    pub a: A,
    pub b: B,
}

impl<A: TestProcessEvents, B: TestProcessEvents<Medium = A::Medium>> TestProcessEvents
    for Twin<A, B>
{
    type Medium = A::Medium;

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
        medium: Self::Medium,
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let mut b_scratch = vec![0u8; scratch.len()];
        let mut a_out = Recorder { sent: vec![], out };
        let a_reply = self
            .a
            .process_request_bytes(bytes, scratch, &mut a_out, medium);
        let mut b_out = Replay {
            sent: a_out.sent.clone().into_iter(),
            events: vec![],
        };
        let b_reply = self
            .b
            .process_request_bytes(bytes, &mut b_scratch, &mut b_out, medium)
            .map(<[u8]>::to_vec);
        assert_eq!(
            a_reply.as_deref().map(<[u8]>::to_vec).map_err(|e| *e),
            b_reply,
            "replies to {bytes:02x?}"
        );
        let a_events: Vec<_> = a_out.sent.into_iter().map(|(event, _)| event).collect();
        assert_eq!(a_events, b_out.events, "events of {bytes:02x?}");
        a_reply
    }
}

/// An event sent through [Recorder], and what sending it gave.
type Sent = (Vec<u8>, Result<(), wire_weaver::SendError>);

/// Keeps the events sent through it, passed on to `out`, and what sending them gave.
struct Recorder<'o, O> {
    sent: Vec<Sent>,
    out: &'o mut O,
}

impl<O: BlockingEventOut> BlockingEventOut for Recorder<'_, O> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), wire_weaver::SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut buf = vec![0u8; MAX_MESSAGE_LEN];
        let len = ser(&mut buf).map_err(wire_weaver::SendError::Serialize)?;
        let event = buf[..len].to_vec();
        let r = self.out.send_with_blocking(|scratch| {
            scratch
                .get_mut(..len)
                .ok_or(ShrinkWrapError::OutOfBoundsWriteRawSlice)?
                .copy_from_slice(&event);
            Ok(len)
        });
        self.sent.push((event, r));
        r
    }
}

/// Keeps the events sent through it, each send gives what the same send of the other server of a [Twin] gave.
struct Replay {
    sent: std::vec::IntoIter<Sent>,
    events: Vec<Vec<u8>>,
}

impl BlockingEventOut for Replay {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), wire_weaver::SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut buf = vec![0u8; MAX_MESSAGE_LEN];
        let len = ser(&mut buf).map_err(wire_weaver::SendError::Serialize)?;
        self.events.push(buf[..len].to_vec());
        // more events than the other server sent: the comparison of the events fails
        self.sent.next().map_or(Ok(()), |(_, r)| r)
    }
}

/// Decides whether a request is dropped (returns true) before the server sees it, see [TestDevice::filter_requests].
pub type RequestFilter = Box<dyn FnMut(&[u8]) -> bool + Send>;

/// Device running a test server, stopped when dropped.
pub struct TestDevice {
    path: String,
    /// Link setups (`LinkEvent::Up`) and losses (`Down`) the device saw
    links: Arc<(std::sync::atomic::AtomicU32, std::sync::atomic::AtomicU32)>,
    /// The simulated USB device's control, for [start_device_async_usb_sim]
    #[cfg(feature = "local_socket")]
    usb_sim: Option<wire_weaver_client::usb_sim::UsbSimControl>,
    drop_requests: Arc<AtomicBool>,
    filter: Arc<Mutex<Option<RequestFilter>>>,
    to_device: mpsc::UnboundedSender<Vec<u8>>,
    /// [TestDevice::slow_requests], µs
    slow: Arc<std::sync::atomic::AtomicU64>,
}

impl TestDevice {
    /// Path to connect to: `Client::config(|c| c.in_process_path(device.path()))`.
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// The simulated USB device's control ([start_device_async_usb_sim] only).
    #[cfg(feature = "local_socket")]
    pub fn usb_sim(&self) -> Option<&wire_weaver_client::usb_sim::UsbSimControl> {
        self.usb_sim.as_ref()
    }

    /// How many times the device's link came up (a host's link setup) and went down so far.
    pub fn link_events(&self) -> (u32, u32) {
        (
            self.links.0.load(Ordering::Relaxed),
            self.links.1.load(Ordering::Relaxed),
        )
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

    /// Take this long over every request from now on (the device's loop waits, as a busy device's does), so that
    /// requests stay in flight on the host. `Duration::ZERO` back to normal.
    pub fn slow_requests(&self, by: std::time::Duration) {
        self.slow.store(by.as_micros() as u64, Ordering::Relaxed);
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

/// Same as [start_device], with the device asking for an accumulation window of `window` instead of the default
/// 1 ms: long enough, a test sees which messages waited for it (CLT-50).
pub fn start_device_with_window<S: TestProcessEvents<Medium = ()> + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    window: std::time::Duration,
) -> TestDevice {
    start_with(
        path,
        Media::Messages,
        SyncServer(server),
        version,
        api_hash,
        (),
        Some(window),
    )
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

    fn wakeup(&mut self) -> Option<Wakeup>;
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

    fn wakeup(&mut self) -> Option<Wakeup> {
        None
    }
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

    fn wakeup(&mut self) -> Option<Wakeup> {
        self.0.wakeup()
    }
}

/// Same as [start_device], over simulated USB (`in_process::usb_device`): messages are packed into packets of at most
/// `packet_size` bytes by the real framers on both ends, as over a USB bulk endpoint, so a link tap sees frames.
pub fn start_device_usb<S: TestProcessEvents<Medium = ()> + Send + 'static>(
    path: &str,
    packet_size: usize,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start_on(
        path,
        Media::Usb(packet_size),
        SyncServer(server),
        version,
        api_hash,
        (),
    )
}

/// Same as [start_device_async], over simulated USB like [start_device_usb].
pub fn start_device_async_usb<S: TestProcessEventsAsync<Medium = ()> + Send + 'static>(
    path: &str,
    packet_size: usize,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start_on(
        path,
        Media::Usb(packet_size),
        AsyncServer(server),
        version,
        api_hash,
        (),
    )
}

/// How the device is connected to the host.
#[derive(Clone)]
enum Media {
    /// Whole messages.
    Messages,
    /// Simulated USB packets of at most this many bytes.
    Usb(usize),
    /// Whole messages on a local socket at this path, one client at a time (a hub in another process).
    #[cfg(feature = "local_socket")]
    LocalSocket(std::path::PathBuf),
    /// A simulated USB device with packets of at most this many bytes and the frame counter, on a local socket at
    /// this path (`wire_weaver_client::usb_sim`): hosts in other processes take turns holding it.
    #[cfg(feature = "local_socket")]
    UsbSim(std::path::PathBuf, usize),
}

/// Like [start_device_async], but a simulated USB device on a local socket at `socket` (feature `local_socket`,
/// `wire_weaver_client::usb_sim`, `ww_link::UsbSeqFraming`, packets of `packet` bytes): programs in other processes
/// reach it as `usbsim:<socket>`, one holding it at a time, and it outlives them, as a real USB device does.
#[cfg(feature = "local_socket")]
pub fn start_device_async_usb_sim<S: TestProcessEventsAsync<Medium = ()> + Send + 'static>(
    socket: &std::path::Path,
    packet: usize,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start_on(
        &socket.display().to_string(),
        Media::UsbSim(socket.to_path_buf(), packet),
        AsyncServer(server),
        version,
        api_hash,
        (),
    )
}

/// Like [start_device_async], but served on a local socket at `socket` (feature `local_socket`), one client at a
/// time: a hub in another process opens it as `ipc:<socket>` and the device outlives the hub.
#[cfg(feature = "local_socket")]
pub fn start_device_async_ipc<S: TestProcessEventsAsync<Medium = ()> + Send + 'static>(
    socket: &std::path::Path,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
) -> TestDevice {
    start_on(
        &socket.display().to_string(),
        Media::LocalSocket(socket.to_path_buf()),
        AsyncServer(server),
        version,
        api_hash,
        (),
    )
}

/// Largest simulated USB packet [start_device_usb] supports.
const MAX_USB_PACKET: usize = 512;

fn start<S: Process + Send + 'static>(
    path: &str,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    medium: S::Medium,
) -> TestDevice {
    start_on(path, Media::Messages, server, version, api_hash, medium)
}

fn start_on<S: Process + Send + 'static>(
    path: &str,
    media: Media,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    medium: S::Medium,
) -> TestDevice {
    start_with(path, media, server, version, api_hash, medium, None)
}

fn start_with<S: Process + Send + 'static>(
    path: &str,
    media: Media,
    server: S,
    version: FullVersion<'static>,
    api_hash: ApiHashPair<'static>,
    medium: S::Medium,
    accumulation_time: Option<std::time::Duration>,
) -> TestDevice {
    // a device returning an error outside a method's #[errors(..)] fails the test (API-32)
    wire_weaver_client::set_strict_error_lists(true);
    let drop_requests = Arc::new(AtomicBool::new(false));
    let links: Arc<(std::sync::atomic::AtomicU32, std::sync::atomic::AtomicU32)> = Arc::default();
    let links_out = links.clone();
    #[cfg(feature = "local_socket")]
    let mut usb_sim = None;
    let filter = Arc::new(Mutex::new(None));
    let (to_device, from_test) = mpsc::unbounded_channel::<Vec<u8>>();
    let slow = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let backend = TestBackend {
        server,
        slow: slow.clone(),
        drop_requests: drop_requests.clone(),
        filter: filter.clone(),
        version,
    };
    let mut config =
        ww_device::LinkConfig::new(version, api_hash, ww_client_server::COMPACT_VERSION);
    if let Some(t) = accumulation_time {
        config.accumulation_time = t;
    }
    match media {
        Media::Messages => {
            let (tx, rx) = in_process::device(path, MAX_MESSAGE_LEN);
            std::thread::spawn(move || {
                block_on(serve(config, tx, rx, backend, medium, from_test, links));
            });
        }
        Media::Usb(packet) => {
            assert!(packet <= MAX_USB_PACKET, "packets up to {MAX_USB_PACKET} B");
            let (sink, source) = in_process::usb_device(path, packet);
            std::thread::spawn(move || {
                block_on(async move {
                    let mut tx_frame = [0u8; MAX_USB_PACKET];
                    let mut rx_buf = ww_device::RxBuffer::<MAX_USB_PACKET, MAX_MESSAGE_LEN>::new();
                    let tx = ww_device::FramedTx::new(sink, &mut tx_frame[..packet]);
                    let rx = ww_device::FramedRx::new(source, rx_buf.assembly_buf(packet));
                    serve(config, tx, rx, backend, medium, from_test, links).await
                })
            });
        }
        #[cfg(feature = "local_socket")]
        Media::LocalSocket(socket) => {
            let (ready_tx, ready) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                block_on(async move {
                    // the listener belongs to this thread's runtime
                    let listener = ww_local_socket::listen(&socket, true)
                        .expect("listen on the device socket");
                    let _ = ready_tx.send(());
                    let (tx, rx) = wire_weaver_client::local_socket::device::device_medium(
                        listener,
                        "test device",
                        MAX_MESSAGE_LEN,
                    );
                    serve(config, tx, rx, backend, medium, from_test, links).await
                })
            });
            ready.recv().expect("the device listens");
        }
        #[cfg(feature = "local_socket")]
        Media::UsbSim(socket, packet) => {
            assert!(packet <= MAX_USB_PACKET, "packets up to {MAX_USB_PACKET} B");
            let (ready_tx, ready) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                block_on(async move {
                    let listener = ww_local_socket::listen(&socket, true)
                        .expect("listen on the device socket");
                    let (sink, source) = wire_weaver_client::usb_sim::device_framed::<
                        ww_device::ww_link::UsbSeqFraming,
                    >(listener, packet);
                    let _ = ready_tx.send(source.control());
                    let mut tx_frame = [0u8; MAX_USB_PACKET];
                    let mut rx_buf = ww_device::RxBuffer::<MAX_USB_PACKET, MAX_MESSAGE_LEN>::new();
                    let tx = ww_device::FramedTx::new(sink, &mut tx_frame[..packet]);
                    let rx = ww_device::FramedRx::new(source, rx_buf.assembly_buf(packet));
                    serve(config, tx, rx, backend, medium, from_test, links).await
                })
            });
            usb_sim = Some(ready.recv().expect("the device listens"));
        }
    }
    TestDevice {
        #[cfg(feature = "local_socket")]
        usb_sim,
        path: path.to_string(),
        links: links_out,
        slow,
        drop_requests,
        filter,
        to_device,
    }
}

fn block_on(f: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// The device's event loop, until the [TestDevice] is dropped.
async fn serve<S: Process>(
    config: ww_device::LinkConfig<'static>,
    tx: impl ww_device::MessageTx,
    rx: impl ww_device::MessageRx,
    mut backend: TestBackend<S>,
    medium: S::Medium,
    mut from_test: mpsc::UnboundedReceiver<Vec<u8>>,
    links: Arc<(std::sync::atomic::AtomicU32, std::sync::atomic::AtomicU32)>,
) {
    let mut scratch = [0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = [0u8; MAX_MESSAGE_LEN];
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
        // an event-driven device sleeps until its own wakeup, the others poll every millisecond
        let wakeup = backend.server.wakeup();
        let due = async {
            match wakeup {
                Some(wakeup) => wakeup.await,
                None => {
                    tick.tick().await;
                }
            }
        };
        tokio::select! {
            ready = server.wait() => {
                match server.handle(ready, &mut backend).await {
                    Some(ww_device::LinkEvent::Up) => _ = links.0.fetch_add(1, Ordering::Relaxed),
                    Some(ww_device::LinkEvent::Down(_)) => _ = links.1.fetch_add(1, Ordering::Relaxed),
                    None => {}
                }
            }
            _ = due => {
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
}

struct TestBackend<S> {
    server: S,
    slow: Arc<std::sync::atomic::AtomicU64>,
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
        let slow = self.slow.load(Ordering::Relaxed);
        if slow > 0 {
            tokio::time::sleep(std::time::Duration::from_micros(slow)).await;
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
