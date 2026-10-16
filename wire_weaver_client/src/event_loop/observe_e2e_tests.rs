//! Observing properties (CLT-28) through the host's event loop against a hand-written device that answers
//! `Observe` requests with what it was asked for and sends `PropertyUpdate` events when told to, with real framers
//! over an in-memory packet pipe: a reconnect after `disconnect().keep_streams()` subscribes again, a link that goes
//! down for good ends the observers, a device with an older API model gets `Error::Unsupported`.

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, Error as ShrinkWrapError, RefVec, SerializeShrinkWrap,
    SerializeShrinkWrapOwned, UNib32, UVlq32,
};
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::observe::{ObserveEntry, ObserveGranted, ObserveMode, ObserveRefused};
use ww_client_server::{Event, EventKind, PathKind, Request, RequestKind};
use ww_device::{LinkConfig, LinkEvent};
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use super::device_e2e_tests::{
    DevSink, DevSource, HostRx, HostTx, PACKET, Packet, PipeTransport, RxFramer, TokioClock,
    TxFramer,
};
use crate::event_loop::command::{Command, EventLoopResidual};
use crate::event_loop::transport::Opened as TransportOpened;
use crate::observe::{LevelConfig, LevelUpdate, ObserveEvent, Sample, Second, on_change, periodic};
use crate::{Commander, Error, StreamError};

const LEVEL: u32 = 3;

/// A level with two properties, `x` at resource index 0 and `y` at 2
#[wire_weaver::shrink_wrap::derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq))]
pub struct LevelData {
    #[slot = 0]
    x: Option<u32>,
    #[slot = 2]
    y: Option<u8>,
}

#[derive(Debug, Default)]
pub struct Latest {
    x: Option<Sample<u32>>,
    y: Option<Sample<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cfg {
    x: Option<ObserveMode>,
    y: Option<ObserveMode>,
}

impl LevelConfig for Cfg {
    fn to_entries(&self) -> BTreeMap<u32, ObserveMode> {
        [(0, self.x), (2, self.y)]
            .into_iter()
            .filter_map(|(i, m)| Some((i, m?)))
            .collect()
    }

    fn from_entries(entries: &BTreeMap<u32, ObserveMode>) -> Self {
        Cfg {
            x: entries.get(&0).copied(),
            y: entries.get(&2).copied(),
        }
    }
}

impl LevelUpdate for LevelData {
    type Latest = Latest;
    type Config = Cfg;

    fn update_latest(self, latest: &mut Latest, received: std::time::Instant, t: Option<Second>) {
        if let Some(value) = self.x {
            latest.x = Some(Sample { value, received, t });
        }
        if let Some(value) = self.y {
            latest.y = Some(Sample { value, received, t });
        }
    }
}

/// Answers `Observe` with the entries asked for, tells the test about each request
struct Dev {
    requests: mpsc::UnboundedSender<Vec<ObserveEntry>>,
}

impl WireWeaverAsyncApiBackend for Dev {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        _out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let request = Request::from_ww_bytes(data)?;
        let RequestKind::Observe { cfg } = request.kind else {
            return Err(ShrinkWrapError::OutOfBoundsRev);
        };
        let entries = cfg.entries.iter().collect::<Result<Vec<_>, _>>()?;
        _ = self.requests.send(entries.clone());
        let granted = ObserveGranted {
            entries: RefVec::Slice { slice: &entries },
            refused: RefVec::Slice {
                slice: &[] as &[ObserveRefused],
            },
            budget: cfg.budget.unwrap_or(UNib32(1_000)),
        };
        let mut buf = [0u8; 128];
        let granted = granted.to_ww_bytes(&mut buf)?;
        Event {
            seq: UVlq32(request.seq.0),
            result: Ok(EventKind::Value {
                data: TailBytes(granted),
            }),
        }
        .to_ww_bytes(scratch)
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

fn update_event(data: &LevelData) -> Vec<u8> {
    let data = data.to_ww_bytes_owned().unwrap();
    let path = [UNib32(LEVEL)];
    let mut buf = [0u8; 128];
    Event {
        seq: UVlq32(0),
        result: Ok(EventKind::PropertyUpdate {
            path: RefVec::Slice { slice: &path },
            t: None,
            missed: Some(UNib32(2)),
            data: TailBytes(&data),
        }),
    }
    .to_ww_bytes(&mut buf)
    .unwrap()
    .to_vec()
}

/// One connection of the device, serving until the host goes. `model` is its `ww_client_server` version.
async fn run_device(
    mut dev: Dev,
    model: (u32, u32),
    mut send: mpsc::UnboundedReceiver<Vec<u8>>,
    to_host: mpsc::Sender<Packet>,
    from_host: mpsc::Receiver<Packet>,
) -> (Dev, mpsc::UnboundedReceiver<Vec<u8>>) {
    let mut tx_frame = [0u8; PACKET];
    let mut rx = ww_device::RxBuffer::<{ 2 * PACKET }, 1024>::new();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let config = LinkConfig::new(
        FullVersion::new("test_api", Version::new(0, 3, 1)),
        ApiHashPair::empty(),
        CompactVersion::new(GlobalTypeId::new(513), model.0, model.1, 0),
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
        tokio::select! {
            ready = server.wait() => {
                if let Some(LinkEvent::Down(_)) = server.handle(ready, &mut dev).await {
                    return (dev, send);
                }
            }
            Some(ev) = send.recv() => {
                server.send(&ev).await.unwrap();
            }
        }
    }
}

type DeviceTask = tokio::task::JoinHandle<(Dev, mpsc::UnboundedReceiver<Vec<u8>>)>;

/// Start an event loop on `cmd_rx` and connect it to the device; the residual comes back when it exits, to start
/// the next one on the same command channel (a reconnect).
async fn start_event_loop(
    dev: Dev,
    model: (u32, u32),
    send: mpsc::UnboundedReceiver<Vec<u8>>,
    cmd: &mut Commander,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: mpsc::Receiver<Command>,
) -> (DeviceTask, oneshot::Receiver<EventLoopResidual>) {
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(4);
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64);
    let device = tokio::spawn(run_device(dev, model, send, dev_to_host_tx, host_to_dev_rx));
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
    cmd.connected_device = info.result.expect("connected");
    (device, residual_rx)
}

fn level_path() -> PathKind<'static> {
    PathKind::Absolute {
        path: RefVec::Slice {
            slice: &[UNib32(LEVEL)],
        },
    }
}

async fn next<T: std::fmt::Debug>(
    o: &mut crate::observe::Observer<LevelData, T>,
) -> ObserveEvent<T> {
    tokio::time::timeout(Duration::from_secs(2), o.recv_event())
        .await
        .expect("an event")
        .unwrap()
}

struct Bench {
    cmd: Commander,
    cmd_tx: mpsc::Sender<Command>,
    send: mpsc::UnboundedSender<Vec<u8>>,
    requests: mpsc::UnboundedReceiver<Vec<ObserveEntry>>,
    device: DeviceTask,
    residual: oneshot::Receiver<EventLoopResidual>,
}

async fn bench(model: (u32, u32)) -> Bench {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let mut cmd = Commander::new(cmd_tx.clone());
    let (requests_tx, requests) = mpsc::unbounded_channel();
    let (send, send_rx) = mpsc::unbounded_channel();
    let dev = Dev {
        requests: requests_tx,
    };
    let (device, residual) = start_event_loop(dev, model, send_rx, &mut cmd, &cmd_tx, cmd_rx).await;
    Bench {
        cmd,
        cmd_tx,
        send,
        requests,
        device,
        residual,
    }
}

#[tokio::test]
async fn reconnect_with_keep_streams_observes_again() {
    let mut b = bench((0, 2)).await;
    let x_mode = periodic(Duration::from_millis(10));
    let mut level = b
        .cmd
        .prepare_observe::<LevelData, LevelData>(
            level_path(),
            Cfg {
                x: Some(x_mode),
                y: None,
            },
            Some,
        )
        .subscribe()
        .await
        .unwrap();
    let mut y = b
        .cmd
        .prepare_observe::<LevelData, u8>(
            level_path(),
            Cfg {
                x: None,
                y: Some(on_change(Duration::from_millis(1))),
            },
            |u| u.y,
        )
        .subscribe()
        .await
        .unwrap();
    assert_eq!(b.requests.recv().await.unwrap().len(), 1);
    let both = b.requests.recv().await.unwrap();
    assert_eq!(both.len(), 2, "one subscription for both: {both:?}");
    assert_eq!(level.granted().unwrap().modes.x, Some(x_mode));

    b.send
        .send(update_event(&LevelData {
            x: Some(5),
            y: None,
        }))
        .unwrap();
    b.send
        .send(update_event(&LevelData {
            x: None,
            y: Some(7),
        }))
        .unwrap();
    let ObserveEvent::Update(u) = next(&mut level).await else {
        panic!()
    };
    assert_eq!((u.value.x, u.value.y, u.missed), (Some(5), None, 2));
    let ObserveEvent::Update(u) = next(&mut y).await else {
        panic!()
    };
    assert_eq!(u.value, 7, "the update without y skipped");
    next(&mut level).await;
    level.with_latest(|l| {
        assert_eq!(l.x.as_ref().unwrap().value, 5);
        assert_eq!(l.y.as_ref().unwrap().value, 7);
    });

    b.cmd.disconnect().keep_streams().await.unwrap();
    let residual = b.residual.await.unwrap();
    let (dev, send_rx) = b.device.await.unwrap();
    assert_eq!(next(&mut level).await, ObserveEvent::Disconnected);
    assert_eq!(next(&mut y).await, ObserveEvent::Disconnected);

    let (_device, _residual) =
        start_event_loop(dev, (0, 2), send_rx, &mut b.cmd, &b.cmd_tx, residual.cmd_rx).await;
    assert_eq!(b.cmd.resume_streams().await.unwrap(), 1, "one level");
    assert_eq!(
        b.requests.recv().await.unwrap(),
        both,
        "the same subscription"
    );
    assert_eq!(next(&mut level).await, ObserveEvent::Resumed);
    assert_eq!(next(&mut y).await, ObserveEvent::Resumed);
    b.send
        .send(update_event(&LevelData {
            x: Some(6),
            y: Some(8),
        }))
        .unwrap();
    let ObserveEvent::Update(u) = next(&mut y).await else {
        panic!()
    };
    assert_eq!(u.value, 8);

    // the last observers go: the level is turned off
    drop(level);
    assert_eq!(b.requests.recv().await.unwrap().len(), 1);
    drop(y);
    assert!(b.requests.recv().await.unwrap().is_empty());
    tokio::task::spawn_blocking(move || drop(b.cmd))
        .await
        .unwrap();
}

#[tokio::test]
async fn link_down_ends_observers() {
    let b = bench((0, 2)).await;
    let mut level = b
        .cmd
        .prepare_observe::<LevelData, LevelData>(
            level_path(),
            Cfg {
                x: Some(on_change(Duration::ZERO)),
                y: None,
            },
            Some,
        )
        .subscribe()
        .await
        .unwrap();
    b.cmd.disconnect().asynch().await.unwrap();
    assert_eq!(next(&mut level).await, ObserveEvent::Disconnected);
    let end = tokio::time::timeout(Duration::from_secs(2), level.recv_event())
        .await
        .unwrap();
    assert!(matches!(end, Err(StreamError::Closed)), "{end:?}");
    tokio::task::spawn_blocking(move || drop(b.cmd))
        .await
        .unwrap();
}

#[tokio::test]
async fn device_before_api_38_is_unsupported() {
    let b = bench((0, 1)).await;
    let r = b
        .cmd
        .prepare_observe::<LevelData, LevelData>(level_path(), Cfg::default(), Some)
        .subscribe()
        .await;
    match r {
        Err(Error::Unsupported { what, reason }) => {
            assert_eq!(what, "observing properties");
            assert!(reason.contains("0.1.0"), "{reason}");
        }
        r => panic!("{:?}", r.map(|_| ())),
    }
    tokio::task::spawn_blocking(move || drop(b.cmd))
        .await
        .unwrap();
}

/// `ww info` builds trait clients (Commander clones) and drops them, then disconnects once: a dropped clone must
/// leave the connection alone, and the last one to drop is the one that disconnects.
#[tokio::test]
async fn dropping_a_commander_clone_keeps_the_connection() {
    let b = bench((0, 2)).await;
    let clone = b.cmd.clone();
    tokio::task::spawn_blocking(move || drop(clone))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let level = b
        .cmd
        .prepare_observe::<LevelData, LevelData>(level_path(), Cfg::default(), Some)
        .subscribe()
        .await;
    assert!(level.is_ok(), "{:?}", level.err());
    drop(level);
    // the explicit disconnect finds the event loop running and the later drop of the last clone is a no-op
    b.cmd.disconnect().asynch().await.unwrap();
    tokio::task::spawn_blocking(move || drop(b.cmd))
        .await
        .unwrap();
}
