//! Credit sinks (API-34) resumed end to end: the host's event loop and [Sink] against a hand-written device
//! ([ww_device::Server] with `ww_stream`'s [SinkCredit], answering like generated code), with real framers over an
//! in-memory packet pipe. A reconnect with `disconnect().keep_streams()` in the middle of a transfer, a restarted
//! host continuing from a saved cursor, a device that lost the session, a late duplicate open.

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::tail_bytes::{TailBytes, TailBytesOwned};
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, Error as ShrinkWrapError, RefVec, SerializeShrinkWrap, UNib32, UVlq32,
    UVlq64,
};
use wire_weaver::{EventOut, EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::{
    Event, EventKind, PathKind, PathKindOwned, Request, RequestKind, StreamSideband,
};
use ww_device::{LinkConfig, LinkEvent};
use ww_stream::StreamCursor;
use ww_stream::sink::SinkCredit;
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use super::device_e2e_tests::{
    DevSink, DevSource, HostRx, HostTx, PACKET, Packet, PipeTransport, RxFramer, TokioClock,
    TxFramer,
};
use crate::event_loop::command::{Command, EventLoopResidual};
use crate::event_loop::transport::Opened as TransportOpened;
use crate::{Commander, Sink, StreamError};

const CAPACITY: u32 = 512;
const CHUNK: usize = 50;
const SINK: u32 = 0;

/// Device state that outlives a connection, like a firmware's while the link goes and comes back
struct Dev {
    credit: SinkCredit,
    /// Bytes accepted, in order
    got: Vec<u8>,
    /// Writes are lost on the way once this many bytes arrived, until the link goes
    lose_from: Option<usize>,
}

impl Dev {
    fn new() -> Box<Self> {
        Box::new(Dev {
            credit: SinkCredit::new(CAPACITY).with_threshold(CAPACITY / 2),
            got: vec![],
            lose_from: None,
        })
    }

    /// The device reboots: the session is gone, what it got stays for the test to look at.
    fn reboot(&mut self) {
        self.credit = SinkCredit::new(CAPACITY).with_threshold(CAPACITY / 2);
    }
}

async fn send_sideband(out: &mut EventWriter<'_, impl MessageSink>, sideband: StreamSideband) {
    let path = [UNib32(SINK)];
    // a lost Ack is recovered by the host's next Query
    let _ = out
        .send_event(&Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamSideband {
                path: RefVec::Slice { slice: &path },
                sideband,
            }),
        })
        .await;
}

/// Answers like generated server code for a `#[credit]` byte sink that consumes every write right away.
impl WireWeaverAsyncApiBackend for Dev {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let request = Request::from_ww_bytes(data)?;
        match request.kind {
            RequestKind::StreamOpen {
                resume: Some(cursor),
                ..
            } => {
                let opened = self.credit.open(cursor);
                if let Some(sideband) = self.credit.take_ack() {
                    send_sideband(out, sideband).await;
                }
                let mut buf = [0u8; 64];
                let opened = opened
                    .to_wire(&[])
                    .with_capacity(Some(self.credit.capacity()))
                    .to_ww_bytes(&mut buf)?;
                return Event {
                    seq: UVlq32(request.seq.0),
                    result: Ok(EventKind::Value {
                        data: TailBytes(opened),
                    }),
                }
                .to_ww_bytes(scratch);
            }
            RequestKind::StreamSideband {
                sideband: StreamSideband::Query { sent, .. },
            } => {
                if let Some(sideband) = self.credit.on_query(sent.0) {
                    send_sideband(out, sideband).await;
                }
            }
            RequestKind::StreamSideband {
                sideband: StreamSideband::Close,
            } => self.credit.close(),
            ref kind => {
                let Some((units, pos, data)) = kind.as_sink_write() else {
                    return Err(ShrinkWrapError::OutOfBoundsRev);
                };
                if self.lose_from.is_some_and(|from| self.got.len() >= from) {
                    return Ok(&[]);
                }
                use ww_stream::sink::SinkVerdict;
                match self.credit.on_write(pos, units, data.len() as u32) {
                    SinkVerdict::Accept => {
                        self.got.extend_from_slice(data);
                        self.credit.free(data.len() as u32);
                        if let Some(sideband) = self.credit.take_ack() {
                            send_sideband(out, sideband).await;
                        }
                    }
                    SinkVerdict::Gap(Some(sideband)) | SinkVerdict::NoSession(Some(sideband)) => {
                        send_sideband(out, sideband).await
                    }
                    _ => {}
                }
            }
        }
        Ok(&[])
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

/// One connection of the device: serves until the host goes, the state stays.
async fn run_device(
    mut dev: Box<Dev>,
    to_host: mpsc::Sender<Packet>,
    from_host: mpsc::Receiver<Packet>,
) -> Box<Dev> {
    let mut tx_frame = [0u8; PACKET];
    let mut rx = ww_device::RxBuffer::<{ 2 * PACKET }, 1024>::new();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
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
        &mut event_scratch,
    );
    loop {
        let ready = server.wait().await;
        if let Some(LinkEvent::Down(_)) = server.handle(ready, &mut *dev).await {
            dev.lose_from = None;
            return dev;
        }
    }
}

type DeviceTask = tokio::task::JoinHandle<Box<Dev>>;

/// Start an event loop on `cmd_rx` and connect it to the device over a new pipe; the residual comes back when it
/// exits, to start the next one on the same command channel (a reconnect).
async fn start_event_loop(
    dev: Box<Dev>,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: mpsc::Receiver<Command>,
) -> (DeviceTask, oneshot::Receiver<EventLoopResidual>) {
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(4);
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64);
    let device = tokio::spawn(run_device(dev, dev_to_host_tx, host_to_dev_rx));
    tokio::spawn(crate::event_loop::core::worker(
        cmd_rx,
        PipeTransport(Some(TransportOpened {
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
    let (connected_tx, connected_rx) = oneshot::channel();
    let (residual_tx, residual_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle: Box::new(()),
            max_seq: crate::DEFAULT_MAX_SEQ,
            link_setup_timeout_ms: None,
            tap: None,
            client_version: Box::new(FullVersionOwned::new(
                "test_api".into(),
                VersionOwned::new(0, 3, 0),
            )),
            connected_tx: Some(connected_tx),
            failed_tx: Some(residual_tx),
        })
        .await
        .unwrap();
    let info = connected_rx.await.unwrap();
    assert!(info.result.is_ok(), "{:?}", info.result);
    (device, residual_rx)
}

/// A host connected to the device, its event loop can be restarted on the same commands (a reconnect).
struct Host {
    cmd: Commander,
    cmd_tx: mpsc::Sender<Command>,
    device: Option<DeviceTask>,
    residual: Option<oneshot::Receiver<EventLoopResidual>>,
}

impl Host {
    async fn connect(dev: Box<Dev>) -> Host {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let (device, residual) = start_event_loop(dev, &cmd_tx, cmd_rx).await;
        Host {
            cmd: Commander::new(cmd_tx.clone()),
            cmd_tx,
            device: Some(device),
            residual: Some(residual),
        }
    }

    /// `disconnect().keep_streams()`: the link goes, the program goes on. Returns the device's state.
    async fn disconnect_keep(&mut self) -> Box<Dev> {
        self.cmd.disconnect().keep_streams().await.unwrap();
        let device = self.device.take().expect("connected");
        tokio::time::timeout(Duration::from_secs(2), device)
            .await
            .expect("device notices the link is gone")
            .unwrap()
    }

    /// Connect the same commander again (after [Self::disconnect_keep]).
    async fn reconnect(&mut self, dev: Box<Dev>) {
        let residual = self.residual.take().expect("disconnected").await.unwrap();
        let (device, residual) = start_event_loop(dev, &self.cmd_tx, residual.cmd_rx).await;
        self.device = Some(device);
        self.residual = Some(residual);
    }

    /// The host program exits: the device notices, keeps its state.
    async fn exits(self) -> Box<Dev> {
        let Host { cmd, device, .. } = self;
        tokio::task::spawn_blocking(move || drop(cmd))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), device.expect("connected"))
            .await
            .expect("device notices the host is gone")
            .unwrap()
    }

    async fn sink(&self) -> Sink<TailBytesOwned> {
        let path = [UNib32(SINK)];
        self.cmd
            .prepare_credit_sink(PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            })
            .await
            .unwrap()
    }
}

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
}

async fn send_all(sink: &mut Sink<TailBytesOwned>, bytes: &[u8]) {
    for chunk in bytes.chunks(CHUNK) {
        tokio::time::timeout(Duration::from_secs(2), sink.send_bytes(chunk))
            .await
            .expect("credit in time")
            .unwrap();
    }
}

async fn flush(sink: &mut Sink<TailBytesOwned>) {
    tokio::time::timeout(Duration::from_secs(2), sink.flush())
        .await
        .expect("flushed in time")
        .unwrap();
}

/// The link goes in the middle of a transfer with writes lost on the way, the program keeps its sink: after the
/// reconnect, `resume_streams()` opens the session again from where the device is and the sink sends what it
/// didn't acknowledge. Every byte arrives exactly once, in order. A `send` while the link is down waits for the
/// resume.
#[tokio::test]
async fn reconnect_in_the_middle_of_a_transfer() {
    let mut dev = Dev::new();
    // the last writes before the link goes are lost
    dev.lose_from = Some(800);
    let mut host = Host::connect(dev).await;
    let mut sink = host.sink().await;
    sink.open().await.unwrap();
    let session = sink.cursor().unwrap().session;
    let bytes = data(3000);
    send_all(&mut sink, &bytes[..1000]).await;
    let dev = host.disconnect_keep().await;
    assert_eq!(dev.got, bytes[..800]);
    assert!(sink.cursor().unwrap().pos.0 <= 800);

    // the program goes on sending: it waits for the resume
    let rest = bytes[1000..].to_vec();
    let sender = tokio::spawn(async move {
        send_all(&mut sink, &rest).await;
        flush(&mut sink).await;
        sink
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!sender.is_finished(), "waits for the resume");

    host.reconnect(dev).await;
    assert_eq!(host.cmd.resume_streams().await.unwrap(), 1);
    let sink = tokio::time::timeout(Duration::from_secs(5), sender)
        .await
        .expect("the transfer goes on")
        .unwrap();
    assert_eq!(
        sink.cursor(),
        Some(StreamCursor {
            session,
            pos: UVlq64(3000)
        })
    );
    let dev = host.exits().await;
    assert_eq!(dev.got.len(), bytes.len(), "nothing repeated, nothing lost");
    assert_eq!(dev.got, bytes);
}

/// A device that rebooted while the link was down lost the session: the resume doesn't count it, the sink's next
/// call says so, and a new session works.
#[tokio::test]
async fn reconnect_to_a_rebooted_device_reports_the_lost_session() {
    let mut host = Host::connect(Dev::new()).await;
    let mut sink = host.sink().await;
    sink.open().await.unwrap();
    let bytes = data(1000);
    send_all(&mut sink, &bytes).await;
    flush(&mut sink).await;
    let mut dev = host.disconnect_keep().await;
    dev.reboot();
    host.reconnect(dev).await;
    assert_eq!(host.cmd.resume_streams().await.unwrap(), 0);
    match sink.send_bytes(&bytes[..CHUNK]).await {
        Err(StreamError::SinkSessionLost { device_at, acked }) => {
            assert_eq!((device_at, acked), (0, 1000));
        }
        r => panic!("{r:?}"),
    }
    assert_eq!(sink.cursor(), None);
    // user code starts over
    sink.open().await.unwrap();
    send_all(&mut sink, &bytes).await;
    flush(&mut sink).await;
    let dev = host.exits().await;
    assert_eq!(dev.got.len(), 2000);
    assert_eq!(dev.got[1000..], bytes);
}

/// The host program restarts: the new one continues the session from the cursor the old one saved, from where
/// the device is, also when the device got more than the saved cursor says.
#[tokio::test]
async fn restarted_host_resumes_from_a_saved_cursor() {
    let host = Host::connect(Dev::new()).await;
    let mut sink = host.sink().await;
    sink.open().await.unwrap();
    let bytes = data(3000);
    send_all(&mut sink, &bytes[..1000]).await;
    flush(&mut sink).await;
    // saved with the program's own progress, then more goes out before the program dies
    let saved = sink.cursor().unwrap();
    assert_eq!(saved.pos.0, 1000);
    send_all(&mut sink, &bytes[1000..1500]).await;
    flush(&mut sink).await;
    drop(sink);
    let dev = host.exits().await;

    let host = Host::connect(dev).await;
    let mut sink = host.sink().await;
    let at = sink.resume(saved).await.unwrap();
    assert_eq!(at, 1500, "where the device is");
    send_all(&mut sink, &bytes[at as usize..]).await;
    flush(&mut sink).await;
    assert_eq!(sink.cursor().unwrap().session, saved.session);
    let dev = host.exits().await;
    assert_eq!(dev.got, bytes);

    // a cursor of a session the device no longer has
    let host = Host::connect(dev).await;
    let mut sink = host.sink().await;
    let lost = StreamCursor {
        session: UVlq32(saved.session.0.wrapping_add(1).max(1)),
        pos: UVlq64(200),
    };
    match sink.resume(lost).await {
        Err(StreamError::SinkSessionLost { device_at, acked }) => {
            assert_eq!((device_at, acked), (0, 200))
        }
        r => panic!("{r:?}"),
    }
    host.exits().await;
}

/// An open of the running session arrives again late (a duplicate on UDP): the device answers where it is and
/// resets nothing, the transfer goes on.
#[tokio::test]
async fn late_duplicate_open_resets_nothing() {
    let host = Host::connect(Dev::new()).await;
    let mut sink = host.sink().await;
    sink.open().await.unwrap();
    let first = sink.cursor().unwrap();
    let bytes = data(2000);
    send_all(&mut sink, &bytes[..1000]).await;
    flush(&mut sink).await;
    // the first open once more
    let path = PathKindOwned::Absolute {
        path: vec![UNib32(SINK)],
    };
    let (open, answer) = host
        .cmd
        .transport()
        .sink_open_command(
            path,
            StreamCursor {
                pos: UVlq64(0),
                ..first
            },
        )
        .unwrap();
    host.cmd.transport().send_command(open).await.unwrap();
    let answer = answer.await.unwrap().unwrap();
    let opened = ww_client_server::stream::StreamOpened::from_ww_bytes(&answer).unwrap();
    assert_eq!(opened.live_from.0, 1000, "answered where it is");
    send_all(&mut sink, &bytes[1000..]).await;
    flush(&mut sink).await;
    let dev = host.exits().await;
    assert_eq!(dev.got, bytes);
}
