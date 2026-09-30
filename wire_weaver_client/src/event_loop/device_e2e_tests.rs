//! Host event loop against the device side ([ww_device::Server]), both with real framers, over an
//! in-memory packet pipe. Covers everything between `Commander` and the device backend apart from USB
//! itself and the generated code.

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{Error as ShrinkWrapError, SerializeShrinkWrap};
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::{Event, EventKind};
use ww_device::{DownReason, LinkConfig, LinkEvent};
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use crate::device_info::ConnectionInfo;
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
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
    fn connect(&mut self, _: DeviceHandle) -> Result<Opened<HostTx, HostRx>, String> {
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
        if data.get(2) == Some(&0xFF) {
            return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
        }
        let mut value = data[2..].to_vec();
        value.resize(300, 0xEE);
        Event {
            seq: u16::from_le_bytes([data[0], data[1]]),
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
    let mut scratch = [0u8; 512];
    let config = LinkConfig::new(
        FullVersion::new("test_api", Version::new(0, 3, 1)),
        ApiHashPair::empty(),
        CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
    );
    let mut server = ww_device::Server::new(
        config,
        ww_device::FramedTx::new(DevSink(to_host), &mut tx_frame),
        ww_device::FramedRx::new(DevSource(from_host), rx.assembly_buf(PACKET)),
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

#[tokio::test]
async fn host_and_device_talk() {
    let (cmd_tx, mut events_rx, dev, host) = start();

    let info = connect(&cmd_tx, (0, 3)).await.result.unwrap();
    assert_eq!(info.max_message_size, DEV_MAX_MESSAGE);
    assert_eq!(info.user_api_version.crate_id, "test_api");
    assert_eq!(events_rx.recv().await, Some(LinkEvent::Up));

    // many requests at once, replies span several packets and are accumulated into shared ones
    const N: u32 = 200;
    let mut done = vec![];
    for i in 0..N {
        let (done_tx, done_rx) = oneshot::channel();
        let bytes = [0u8, 0].into_iter().chain(i.to_le_bytes()).collect();
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
            bytes: vec![0, 0, 0xFF],
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
            bytes: vec![0; DEV_MAX_MESSAGE + 1],
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
