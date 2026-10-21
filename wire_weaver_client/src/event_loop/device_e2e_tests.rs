//! Host event loop against the device side ([ww_device::Server]), both with real framers, over an
//! in-memory packet pipe. Covers everything between `Commander` and the device backend apart from USB
//! itself and the generated code.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    BufReader, Error as ShrinkWrapError, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::{Event, EventKind};
use ww_device::{DownReason, LinkConfig, LinkEvent};
use ww_link::{RttChecksum, RttHead, RttTail};
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use crate::device_info::ConnectionInfo;
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::{Command, LoopbackParams, TestProgress};
use crate::event_loop::stream::{self, BlockingStreamIo, StreamConfig, StreamRx, StreamTx};
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Transport};

pub(crate) const PACKET: usize = 64;
pub(crate) const DEV_MAX_MESSAGE: usize = 1024;

pub(crate) type Packet = Vec<u8>;
pub(crate) type TxFramer = ww_link::FramingTxOwned<ww_link::UsbFraming>;
pub(crate) type RxFramer = ww_link::FramingRxOwned<ww_link::UsbFraming>;

// Host side: same framing as NusbTx / NusbRx, over channels

pub(crate) struct HostTx {
    pub(crate) framer: TxFramer,
    pub(crate) packets: mpsc::Sender<Packet>,
}

impl MessageTx for HostTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => self.flush().await?,
                Err(()) => return Err("too big".into()),
            }
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        if let Some(p) = self.framer.flush_to_vec() {
            self.packets
                .send(p)
                .await
                .map_err(|_| "closed".to_string())?;
        }
        Ok(())
    }
}

pub(crate) struct HostRx {
    pub(crate) framer: RxFramer,
    pub(crate) packets: mpsc::Receiver<Packet>,
}

impl MessageRx for HostRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            self.framer.reassemble();
            if self.framer.message().is_some() {
                break;
            }
            let p = self.packets.recv().await.ok_or("closed")?;
            self.framer.stage(&p).map_err(|_| "overflow")?;
        }
        Ok(self.framer.message().unwrap())
    }
}

pub(crate) struct PipeTransport(pub(crate) Option<Opened<HostTx, HostRx>>);

impl Transport for PipeTransport {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::IN_PROCESS
    }

    type Tx = HostTx;
    type Rx = HostRx;
    async fn connect(
        &mut self,
        _: DeviceHandle,
        _: ww_tap::RecorderLink,
    ) -> Result<Opened<HostTx, HostRx>, String> {
        self.0.take().ok_or("already connected".into())
    }
}

// Device side: packet IO over channels, tokio clock

pub(crate) struct DevSink(pub(crate) mpsc::Sender<Packet>);

impl ww_device::PacketSink for DevSink {
    type Error = ();
    type Framing = ww_link::UsbFraming;
    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.0.send(packet.to_vec()).await.map_err(|_| ())
    }
}

pub(crate) struct DevSource(pub(crate) mpsc::Receiver<Packet>);

impl ww_device::PacketSource for DevSource {
    type Error = ();
    type Framing = ww_link::UsbFraming;
    fn max_packet_len(&self) -> usize {
        PACKET
    }
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let p = self.0.recv().await.ok_or(())?;
        buf[..p.len()].copy_from_slice(&p);
        Ok(p.len())
    }
    async fn wait_connected(&mut self) {}
}

pub(crate) struct TokioClock(pub(crate) tokio::time::Instant);

impl ww_device::Clock for TokioClock {
    fn now(&self) -> ww_device::Instant {
        ww_device::Instant::from_micros(self.0.elapsed().as_micros() as u64)
    }
    async fn wait_until(&self, at: ww_device::Instant) {
        tokio::time::sleep_until(self.0 + Duration::from_micros(at.as_micros())).await
    }
}

/// Echoes request bytes (after seq) back as a Value, padded to 300 bytes to span several packets.
/// Fails on requests starting with 0xFF, to check the generic error reply.
struct Echo;

impl WireWeaverAsyncApiBackend for Echo {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        _out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let mut rd = BufReader::new(data);
        let seq = UVlq32(rd.read_uvlq32()?);
        let data = &data[data.len() - rd.bytes_left()..];
        if data.first() == Some(&0xFF) {
            return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
        }
        let mut value = data.to_vec();
        value.resize(300, 0xEE);
        Event {
            seq,
            result: Ok(EventKind::Value {
                data: TailBytes(&value),
            }),
        }
        .to_ww_bytes(scratch)
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

/// Device source and sink at a fixed rate: 1 ms every `every` packets, so that a test on paused time gets as much
/// done per (virtual) second as a real device, instead of everything at once.
struct PacedSink {
    tx: mpsc::Sender<Packet>,
    every: u32,
    n: u32,
}

impl ww_device::PacketSink for PacedSink {
    type Error = ();
    type Framing = ww_link::UsbFraming;
    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.n += 1;
        if self.n.is_multiple_of(self.every) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        self.tx.send(packet.to_vec()).await.map_err(|_| ())
    }
}

struct PacedSource {
    rx: mpsc::Receiver<Packet>,
    every: u32,
    n: u32,
}

impl ww_device::PacketSource for PacedSource {
    type Error = ();
    type Framing = ww_link::UsbFraming;
    fn max_packet_len(&self) -> usize {
        PACKET
    }
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.n += 1;
        if self.n.is_multiple_of(self.every) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let p = self.rx.recv().await.ok_or(())?;
        buf[..p.len()].copy_from_slice(&p);
        Ok(p.len())
    }
    async fn wait_connected(&mut self) {}
}

async fn device(
    to_host: mpsc::Sender<Packet>,
    from_host: mpsc::Receiver<Packet>,
    events_tx: mpsc::UnboundedSender<LinkEvent>,
    pace_every: Option<u32>,
) {
    let mut tx_frame = [0u8; PACKET];
    // sized for bigger packets than the source delivers (e.g., bulk endpoint capped at 512),
    // the host must still see exactly DEV_MAX_MESSAGE
    let mut rx = ww_device::RxBuffer::<{ 2 * PACKET }, DEV_MAX_MESSAGE>::new();
    match pace_every {
        None => {
            let tx = ww_device::FramedTx::new(DevSink(to_host), &mut tx_frame);
            let rx = ww_device::FramedRx::new(DevSource(from_host), rx.assembly_buf(PACKET));
            serve(tx, rx, events_tx).await
        }
        Some(every) => {
            let sink = PacedSink {
                tx: to_host,
                every,
                n: 0,
            };
            let source = PacedSource {
                rx: from_host,
                every,
                n: 0,
            };
            let tx = ww_device::FramedTx::new(sink, &mut tx_frame);
            let rx = ww_device::FramedRx::new(source, rx.assembly_buf(PACKET));
            serve(tx, rx, events_tx).await
        }
    }
}

/// The last client meta a test device saw (only [client_meta_reaches_the_device] sends one).
static PEER_META: Mutex<Option<ww_device::PeerMeta>> = Mutex::new(None);

pub(crate) async fn serve(
    tx: impl ww_device::MessageTx,
    rx: impl ww_device::MessageRx,
    events_tx: mpsc::UnboundedSender<LinkEvent>,
) {
    // a reply as big as the largest request fits, e.g. a loopback echo
    let mut scratch = [0u8; DEV_MAX_MESSAGE];
    let mut event_scratch = [0u8; 512];
    let config = LinkConfig::new(
        FullVersion::new("test_api", Version::new(0, 3, 1)),
        ApiHashPair::empty(),
        CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
    );
    let mut server = ww_device::Server::new(
        config,
        tx,
        rx,
        TokioClock(tokio::time::Instant::now()),
        &mut scratch,
        &mut event_scratch,
    );
    let mut backend = Echo;
    loop {
        // user loop: anything else could be selected on here
        let ready = server.wait().await;
        if let Some(event) = server.handle(ready, &mut backend).await {
            if event == LinkEvent::Up
                && let Some(meta) = server.link().peer_meta()
            {
                *PEER_META.lock().unwrap() = Some(meta.clone());
            }
            _ = events_tx.send(event);
        }
    }
}

pub(crate) async fn connect(cmd_tx: &mpsc::Sender<Command>, version: (u32, u32)) -> ConnectionInfo {
    connect_with_meta(cmd_tx, version, None).await
}

async fn connect_with_meta(
    cmd_tx: &mpsc::Sender<Command>,
    version: (u32, u32),
    client_meta: Option<Box<ww_link::ClientMetaOwned>>,
) -> ConnectionInfo {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle: Box::new(()),
            max_seq: crate::DEFAULT_MAX_SEQ,
            link_setup_timeout_ms: None,
            tap: None,
            client_meta,
            client_version: Box::new(FullVersionOwned::new(
                "test_api".into(),
                VersionOwned::new(version.0, version.1, 0),
            )),
            connected_tx: Some(connected_tx),
            failed_tx: None,
        })
        .await
        .unwrap();
    connected_rx.await.unwrap()
}

fn start() -> (
    mpsc::Sender<Command>,
    mpsc::UnboundedReceiver<LinkEvent>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    start_paced(None)
}

fn start_paced(
    pace_every: Option<u32>,
) -> (
    mpsc::Sender<Command>,
    mpsc::UnboundedReceiver<LinkEvent>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    start_with(pace_every, vec![], vec![])
}

/// Same as [start_paced], with packets of a previous session already on the way: `to_host` as if left in the
/// device's IN buffer, `to_device` as if left in its OUT buffer.
fn start_with(
    pace_every: Option<u32>,
    to_host: Vec<Packet>,
    to_device: Vec<Packet>,
) -> (
    mpsc::Sender<Command>,
    mpsc::UnboundedReceiver<LinkEvent>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(4 + to_device.len());
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64 + to_host.len());
    for p in to_device {
        host_to_dev_tx.try_send(p).unwrap();
    }
    for p in to_host {
        dev_to_host_tx.try_send(p).unwrap();
    }
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let dev = tokio::spawn(device(
        dev_to_host_tx,
        host_to_dev_rx,
        events_tx,
        pace_every,
    ));
    let host = tokio::spawn(crate::event_loop::core::worker(
        cmd_rx,
        PipeTransport(Some(Opened {
            tx: HostTx {
                framer: TxFramer::new(PACKET),
                packets: host_to_dev_tx,
            },
            rx: HostRx {
                framer: RxFramer::new(crate::DEFAULT_MAX_MESSAGE_SIZE + PACKET),
                packets: dev_to_host_rx,
            },
        })),
    ));
    (cmd_tx, events_rx, dev, host)
}

// Stream medium (RTT-like): host IO thread and device poll two small rings, so reads and writes
// are partial and end anywhere in a message

/// Ring buffer in "RAM", both ends only take or put what fits, never wait
#[derive(Clone)]
struct Ring(Arc<Mutex<VecDeque<u8>>>, usize);

impl Ring {
    fn new(capacity: usize) -> Self {
        Ring(Arc::new(Mutex::new(VecDeque::new())), capacity)
    }

    fn write(&self, bytes: &[u8]) -> usize {
        let mut q = self.0.lock().unwrap();
        let n = bytes.len().min(self.1 - q.len());
        q.extend(&bytes[..n]);
        n
    }

    fn read(&self, buf: &mut [u8]) -> usize {
        let mut q = self.0.lock().unwrap();
        let n = buf.len().min(q.len());
        for (b, q) in buf.iter_mut().zip(q.drain(..n)) {
            *b = q;
        }
        n
    }
}

/// Host side medium, what a debug probe does
struct HostIo {
    up: Ring,
    down: Ring,
}

impl BlockingStreamIo for HostIo {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        // a probe reads in small pieces as well
        let len = buf.len().min(37);
        Ok(self.up.read(&mut buf[..len]))
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
        Ok(self.down.write(bytes))
    }
}

struct StreamPipe(Option<(Ring, Ring)>);

impl Transport for StreamPipe {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::IN_PROCESS
    }

    type Tx = StreamTx<RttHead, RttChecksum, RttTail>;
    type Rx = StreamRx<RttHead, RttChecksum, RttTail>;
    async fn connect(
        &mut self,
        _: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> Result<Opened<Self::Tx, Self::Rx>, String> {
        let (up, down) = self.0.take().ok_or("already connected")?;
        let config = StreamConfig {
            name: "test_stream",
            poll_interval: Duration::from_millis(1),
            read_chunk: 256,
        };
        stream::open(config, tap, move || Ok(HostIo { up, down })).await
    }
}

/// Device side, same as RttSink / RttSource: poll every 1ms
struct DevStreamSink(Ring);

impl ww_device::StreamSink for DevStreamSink {
    type Error = ();
    async fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), ()> {
        while !bytes.is_empty() {
            bytes = &bytes[self.0.write(bytes)..];
            if !bytes.is_empty() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        Ok(())
    }
}

struct DevStreamSource(Ring);

impl ww_device::StreamSource for DevStreamSource {
    type Error = ();
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        loop {
            let n = self.0.read(buf);
            if n > 0 {
                return Ok(n);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    async fn wait_connected(&mut self) {}
}

async fn stream_device(up: Ring, down: Ring, events_tx: mpsc::UnboundedSender<LinkEvent>) {
    const OVERHEAD: usize =
        ww_device::transport::stream_overhead::<RttHead, RttChecksum, RttTail>();
    let mut tx_chunk = [0u8; DEV_MAX_MESSAGE + OVERHEAD];
    let mut rx_buf = [0u8; DEV_MAX_MESSAGE + OVERHEAD];
    serve(
        ww_device::StreamTx::<_, RttHead, RttChecksum, RttTail>::new(
            DevStreamSink(up),
            &mut tx_chunk,
        ),
        ww_device::StreamRx::<_, RttHead, RttChecksum, RttTail>::new(
            DevStreamSource(down),
            &mut rx_buf,
        ),
        events_tx,
    )
    .await
}

fn start_stream() -> (
    mpsc::Sender<Command>,
    mpsc::UnboundedReceiver<LinkEvent>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    // up is smaller than a reply chunk, down than a request chunk
    let (up, down) = (Ring::new(500), Ring::new(100));
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let dev = tokio::spawn(stream_device(up.clone(), down.clone(), events_tx));
    let host = tokio::spawn(crate::event_loop::core::worker(
        cmd_rx,
        StreamPipe(Some((up, down))),
    ));
    (cmd_tx, events_rx, dev, host)
}

#[tokio::test]
async fn client_meta_reaches_the_device() {
    let (cmd_tx, mut events_rx, _dev, _host) = start();
    let meta = crate::ClientMeta::new()
        .program("e2e-prog")
        .user("anna")
        .host("pc7")
        .kind(crate::ClientKind::Test)
        .pid(4242);
    let sent = Box::new(meta.resolve());
    connect_with_meta(&cmd_tx, (0, 3), Some(sent.clone()))
        .await
        .result
        .unwrap();
    assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));
    let seen = PEER_META.lock().unwrap().clone().expect("device got meta");
    assert_eq!(seen.program.as_str(), "e2e-prog");
    assert_eq!((seen.user.as_str(), seen.host.as_str()), ("anna", "pc7"));
    assert_eq!(seen.kind, Some(crate::ClientKind::Test));
    assert_eq!((seen.pid, seen.instance_id), (4242, sent.instance_id));
}

#[tokio::test]
async fn host_and_device_talk() {
    talk(start()).await
}

#[tokio::test]
async fn host_and_device_talk_over_stream() {
    talk(start_stream()).await
}

pub(crate) async fn talk(
    (cmd_tx, mut events_rx, dev, host): (
        mpsc::Sender<Command>,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ),
) {
    let info = connect(&cmd_tx, (0, 3)).await.result.unwrap();
    assert_eq!(info.max_message_size, DEV_MAX_MESSAGE);
    assert_eq!(info.user_api_version.crate_id, "test_api");
    assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));

    // many requests at once, replies span several packets and are accumulated into shared ones
    const N: u32 = 200;
    let mut done = vec![];
    for i in 0..N {
        let (done_tx, done_rx) = oneshot::channel();
        let bytes = [0u8; UVlq32Backfill::LEN]
            .into_iter()
            .chain(i.to_le_bytes())
            .collect();
        cmd_tx
            .send(Command::SendMessage {
                bytes,
                done_tx: Some((done_tx, Duration::from_secs(5))),
            })
            .await
            .unwrap();
        done.push((i, done_rx));
    }
    for (i, rx) in done {
        let bytes = rx.await.unwrap().unwrap();
        assert_eq!(&bytes[..4], i.to_le_bytes());
        assert_eq!(bytes.len(), 300);
    }

    // backend could not process a request at all: generic error instead of a timeout
    let (done_tx, done_rx) = oneshot::channel();
    cmd_tx
        .send(Command::SendMessage {
            bytes: vec![0, 0, 0, 0, 0, 0xFF],
            done_tx: Some((done_tx, Duration::from_secs(5))),
        })
        .await
        .unwrap();
    let r = tokio::time::timeout(Duration::from_secs(1), done_rx)
        .await
        .expect("generic error reply, not a timeout")
        .unwrap();
    match r {
        Err(crate::Error::RemoteError(e)) => {
            assert!(format!("{e:?}").contains("ResponseSerFailed"), "{e:?}")
        }
        other => panic!("expected RemoteError, got {other:?}"),
    }

    // oversized request is rejected by the host, device limit came from DeviceInfo
    let (done_tx, done_rx) = oneshot::channel();
    cmd_tx
        .send(Command::SendMessage {
            bytes: vec![0; UVlq32Backfill::LEN + DEV_MAX_MESSAGE], // + 1 byte of seq
            done_tx: Some((done_tx, Duration::from_secs(5))),
        })
        .await
        .unwrap();
    assert!(done_rx.await.unwrap().is_err());

    let (disconnected_tx, disconnected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::DisconnectAndExit {
            disconnected_tx: Some(disconnected_tx),
            reason: ww_link::DisconnectReason::RequestByUser,
        })
        .await
        .unwrap();
    disconnected_rx.await.unwrap();
    assert_eq!(
        events_rx.recv().await,
        Some(LinkEvent::Down(DownReason::Disconnect(
            ww_link::DisconnectReason::RequestByUser
        )))
    );
    tokio::time::timeout(Duration::from_secs(5), host)
        .await
        .expect("host did not exit")
        .unwrap();
    dev.abort();
}

/// All three stages of the loopback test against the device's loopback replies (repeat 1, 0 and N), over the
/// packet pipe with real framers; a request sent meanwhile waits for the test to end.
#[tokio::test(start_paused = true)]
async fn loopback_test() {
    run_loopback_test(16, Default::default()).await;
}

/// A slow device that answers many requests in flight blocks host writes (it doesn't read while writing) for longer
/// than the echo timeout: echoes that arrived meanwhile must count before the timeout does. This failed on a
/// full-speed board with 8 batches in flight.
#[tokio::test(start_paused = true)]
async fn loopback_test_slow_device_many_in_flight() {
    let params = LoopbackParams {
        echo_timeout: Duration::from_millis(100),
        batches_in_flight: 8,
        markers_in_flight: 8,
        ..Default::default()
    };
    run_loopback_test(8, params).await;
}

/// On paused time: the device is paced, 1 ms every `pace_every` packets each way, and stages run for 300 ms of
/// virtual time, whatever the load on the machine.
async fn run_loopback_test(pace_every: u32, params: LoopbackParams) {
    let (cmd_tx, mut events_rx, dev, _host) = start_paced(Some(pace_every));
    connect(&cmd_tx, (0, 3)).await.result.unwrap();
    assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    cmd_tx
        .send(Command::LoopbackTest {
            test_duration: Duration::from_millis(300),
            packet_size: None,
            params: Box::new(params),
            progress_tx,
        })
        .await
        .unwrap();
    let (done_tx, mut done_rx) = oneshot::channel();
    cmd_tx
        .send(Command::SendMessage {
            bytes: [0u8; UVlq32Backfill::LEN].into_iter().chain([7]).collect(),
            done_tx: Some((done_tx, Duration::from_secs(1))),
        })
        .await
        .unwrap();

    let mut log = vec![];
    // progress_tx is dropped when the test is over
    while let Some(p) = tokio::time::timeout(Duration::from_secs(5), progress_rx.recv())
        .await
        .expect("test is stuck")
    {
        match p {
            TestProgress::Completion(..) => {
                assert!(done_rx.try_recv().is_err(), "request held during the test");
            }
            TestProgress::FatalError(e) => panic!("{e}"),
            TestProgress::LoopbackReport {
                tx_count,
                lost_count,
                data_corrupted_count,
                ..
            } => {
                assert!(tx_count > 10, "{tx_count}");
                assert_eq!((lost_count, data_corrupted_count), (0, 0));
                log.push("loopback report".to_string());
            }
            TestProgress::SpeedReport {
                name,
                count,
                bytes_per_s,
                per_s,
                lost_count,
                data_corrupted_count,
            } => {
                assert!(count > 10, "{name}: {count}");
                assert_eq!(
                    bytes_per_s,
                    per_s * DEV_MAX_MESSAGE as f32,
                    "max by default"
                );
                assert_eq!((lost_count, data_corrupted_count), (0, 0), "{name}");
                log.push(format!("{name} report"));
            }
            TestProgress::TestStarted(name) => log.push(format!("{name} started")),
            TestProgress::TestCompleted(name) => log.push(format!("{name} completed")),
        }
    }
    assert_eq!(
        log,
        [
            "loopback started",
            "loopback report",
            "loopback completed",
            "tx_speed started",
            "tx_speed report",
            "tx_speed completed",
            "rx_speed started",
            "rx_speed report",
            "rx_speed completed",
        ]
    );
    let reply = tokio::time::timeout(Duration::from_secs(1), done_rx)
        .await
        .expect("held request is sent after the test")
        .unwrap()
        .unwrap();
    assert_eq!(reply[0], 7);
    dev.abort();
}

#[tokio::test]
async fn loopback_test_needs_a_connection() {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let _host = tokio::spawn(crate::event_loop::core::worker(cmd_rx, PipeTransport(None)));
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    cmd_tx
        .send(Command::LoopbackTest {
            test_duration: Duration::from_millis(300),
            packet_size: None,
            params: Default::default(),
            progress_tx,
        })
        .await
        .unwrap();
    assert!(matches!(
        progress_rx.recv().await,
        Some(TestProgress::FatalError(_))
    ));
    assert!(progress_rx.recv().await.is_none());
}

/// Packets of `messages` as a framer of an earlier session wrote them; `keep` of them, as if the rest never left.
fn old_session_packets(messages: &[ww_link::Message<'_>], keep: usize) -> Vec<Packet> {
    let mut framer = TxFramer::new(PACKET);
    let mut packets = vec![];
    let mut scratch = [0u8; 512];
    for m in messages {
        let (kind, payload) = m.encode(&mut scratch).unwrap();
        while !framer.write(kind, payload).unwrap() {
            packets.extend(framer.flush_to_vec());
        }
        packets.extend(framer.flush_to_vec());
    }
    packets.truncate(keep);
    packets
}

/// What a previous session left on the way is never taken for the new one (TR-15): a DeviceInfo and LinkReady
/// already in the device's IN buffer (from a device that would be incompatible, so taking it fails the connection),
/// the first packets of a split message the old host never read, and on the other side a half-sent request and a
/// LinkSetup of the old host. The setup token sorts them out; the device's framer starts a new session on
/// GetDeviceInfo.
#[tokio::test]
async fn previous_session_leftovers_are_dropped() {
    use ww_link::{DeviceInfo, LinkSetup, Message};
    let old_token = 0xDEAD_BEEF;
    let stale_info = Message::DeviceInfo(DeviceInfo {
        dev_link_version: ww_link::LINK_VERSION,
        api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
        user_api_version: FullVersion::new("test_api", Version::new(9, 0, 0)),
        hash: ApiHashPair::empty(),
        dev_max_message_len: 64,
        packet_accumulation_time_us: 1000,
        token: old_token,
    });
    let big = [0x5Au8; 300];
    let mut to_host = old_session_packets(
        &[stale_info, Message::LinkReady { token: old_token }],
        usize::MAX,
    );
    // Start and one Continue of a 300 byte reply
    let half = old_session_packets(
        &[Message::Data {
            channel: 0,
            bytes: &big,
        }],
        2,
    );
    assert_eq!(half.len(), 2);
    to_host.extend(half);
    let mut to_device = old_session_packets(
        &[Message::Data {
            channel: 0,
            bytes: &big[..200],
        }],
        1,
    );
    to_device.extend(old_session_packets(
        &[Message::LinkSetup(LinkSetup {
            host_user_version: FullVersion::new("test_api", Version::new(0, 3, 0)),
            host_max_message_len: 4096,
            client_meta: None,
            token: old_token,
        })],
        usize::MAX,
    ));

    let (cmd_tx, mut events_rx, dev, host) = start_with(None, to_host, to_device);
    let info = tokio::time::timeout(Duration::from_secs(5), connect(&cmd_tx, (0, 3)))
        .await
        .expect("connects, no timeout")
        .result
        .expect("the stale, incompatible DeviceInfo is not taken");
    assert_eq!(info.user_api_version.version.minor.0, 3);
    assert_eq!(info.max_message_size, DEV_MAX_MESSAGE);
    assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));

    // and data flows: the reply is not mixed up with the half-sent one
    let (done_tx, done_rx) = oneshot::channel();
    cmd_tx
        .send(Command::SendMessage {
            bytes: vec![0, 0, 0, 0, 0, 0x42],
            done_tx: Some((done_tx, Duration::from_secs(5))),
        })
        .await
        .unwrap();
    let bytes = done_rx.await.unwrap().unwrap();
    assert_eq!(bytes[0], 0x42);
    assert_eq!(bytes.len(), 300);
    dev.abort();
    host.abort();
}

#[tokio::test]
async fn incompatible_host_is_refused() {
    let (cmd_tx, _events_rx, dev, _host) = start();
    // device is 0.3.x, host 0.4.x: host refuses on DeviceInfo already
    let info = connect(&cmd_tx, (0, 4)).await;
    assert!(info.result.is_err());
    dev.abort();
}

/// Opening happens on the IO thread (e.g., attaching to a probe), its error must fail the connection with its reason
#[tokio::test]
async fn stream_open_error_fails_connect() {
    struct Failing;
    impl Transport for Failing {
        fn link_setup_timeout(&self) -> std::time::Duration {
            crate::event_loop::transport::link_setup_timeout::IN_PROCESS
        }

        type Tx = StreamTx<RttHead, RttChecksum, RttTail>;
        type Rx = StreamRx<RttHead, RttChecksum, RttTail>;
        async fn connect(
            &mut self,
            _: DeviceHandle,
            tap: ww_tap::RecorderLink,
        ) -> Result<Opened<Self::Tx, Self::Rx>, String> {
            let config = StreamConfig {
                name: "test_stream",
                poll_interval: Duration::from_millis(1),
                read_chunk: 256,
            };
            stream::open(config, tap, || Err::<HostIo, _>("no probe".to_string())).await
        }
    }
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let host = tokio::spawn(crate::event_loop::core::worker(cmd_rx, Failing));
    let info = tokio::time::timeout(Duration::from_secs(5), connect(&cmd_tx, (0, 3)))
        .await
        .expect("error, not a timeout");
    let e = info.result.unwrap_err();
    assert!(format!("{e:#}").contains("no probe"), "{e:#}");
    tokio::time::timeout(Duration::from_secs(5), host)
        .await
        .expect("host did not exit")
        .unwrap();
}
