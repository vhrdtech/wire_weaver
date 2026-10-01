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
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::{Event, EventKind};
use ww_device::{DownReason, LinkConfig, LinkEvent};
use ww_link::{RttChecksum, RttHead, RttTail};
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use crate::device_info::ConnectionInfo;
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::stream::{self, BlockingStreamIo, StreamConfig, StreamRx, StreamTx};
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Transport};

const PACKET: usize = 64;
const DEV_MAX_MESSAGE: usize = 1024;

type Packet = Vec<u8>;
type TxFramer = ww_framer::TxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;
type RxFramer = ww_framer::FramedRxOwned<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>;

// Host side: same framing as NusbTx / NusbRx, over channels

struct HostTx {
    framer: TxFramer,
    packets: mpsc::Sender<Packet>,
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

struct HostRx {
    framer: RxFramer,
    packets: mpsc::Receiver<Packet>,
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

struct PipeTransport(Option<Opened<HostTx, HostRx>>);

impl Transport for PipeTransport {
    type Tx = HostTx;
    type Rx = HostRx;
    async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<HostTx, HostRx>, String> {
        self.0.take().ok_or("already connected".into())
    }
}

// Device side: packet IO over channels, tokio clock

struct DevSink(mpsc::Sender<Packet>);

impl ww_device::PacketSink for DevSink {
    type Error = ();
    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.0.send(packet.to_vec()).await.map_err(|_| ())
    }
}

struct DevSource(mpsc::Receiver<Packet>);

impl ww_device::PacketSource for DevSource {
    type Error = ();
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

struct TokioClock(tokio::time::Instant);

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
    async fn process_bytes<'a>(
        &mut self,
        _sink: &mut impl MessageSink,
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

async fn device(
    to_host: mpsc::Sender<Packet>,
    from_host: mpsc::Receiver<Packet>,
    events_tx: mpsc::UnboundedSender<LinkEvent>,
) {
    let mut tx_frame = [0u8; PACKET];
    // sized for bigger packets than the source delivers (e.g., bulk endpoint capped at 512),
    // the host must still see exactly DEV_MAX_MESSAGE
    let mut rx = ww_device::RxBuffer::<{ 2 * PACKET }, DEV_MAX_MESSAGE>::new();
    serve(
        ww_device::FramedTx::new(DevSink(to_host), &mut tx_frame),
        ww_device::FramedRx::new(DevSource(from_host), rx.assembly_buf(PACKET)),
        events_tx,
    )
    .await
}

async fn serve(
    tx: impl ww_device::MessageTx,
    rx: impl ww_device::MessageRx,
    events_tx: mpsc::UnboundedSender<LinkEvent>,
) {
    let mut scratch = [0u8; 512];
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
    );
    let mut backend = Echo;
    loop {
        // user loop: anything else could be selected on here
        let ready = server.wait().await;
        if let Some(event) = server.handle(ready, &mut backend).await {
            _ = events_tx.send(event);
        }
    }
}

async fn connect(cmd_tx: &mpsc::Sender<Command>, version: (u32, u32)) -> ConnectionInfo {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle: Box::new(()),
            max_seq: crate::DEFAULT_MAX_SEQ,
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
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(4);
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64);
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let dev = tokio::spawn(device(dev_to_host_tx, host_to_dev_rx, events_tx));
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
    type Tx = StreamTx<RttHead, RttChecksum, RttTail>;
    type Rx = StreamRx<RttHead, RttChecksum, RttTail>;
    async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<Self::Tx, Self::Rx>, String> {
        let (up, down) = self.0.take().ok_or("already connected")?;
        let config = StreamConfig {
            name: "test_stream",
            poll_interval: Duration::from_millis(1),
            read_chunk: 256,
        };
        stream::open(config, move || Ok(HostIo { up, down })).await
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
async fn host_and_device_talk() {
    talk(start()).await
}

#[tokio::test]
async fn host_and_device_talk_over_stream() {
    talk(start_stream()).await
}

async fn talk(
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
        type Tx = StreamTx<RttHead, RttChecksum, RttTail>;
        type Rx = StreamRx<RttHead, RttChecksum, RttTail>;
        async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<Self::Tx, Self::Rx>, String> {
            let config = StreamConfig {
                name: "test_stream",
                poll_interval: Duration::from_millis(1),
                read_chunk: 256,
            };
            stream::open(config, || Err::<HostIo, _>("no probe".to_string())).await
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
