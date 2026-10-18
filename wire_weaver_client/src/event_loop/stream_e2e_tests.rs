//! Counted and reliable streams (CLT-23) end to end: the host's event loop and [PositionedStream] against a
//! hand-written device ([ww_device::Server] with `ww_stream`'s `Reliable` over a `RamRing` and `Counted`), with real
//! framers over an in-memory packet pipe. Scenarios of `docs/api/stream_reliability.md`: S4 (host stuck, bounded
//! memory) and S6 (host restart, resume from a saved cursor).

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::tail_bytes::{TailBytes, TailBytesOwned};
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, Error as ShrinkWrapError, RefVec, SerializeShrinkWrap, UNib32, UVlq32,
};
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use ww_client_server::{Event, EventKind, PathKind, Request, RequestKind, StreamSideband};
use ww_device::{LinkConfig, LinkEvent};
use ww_stream::store::RamRing;
use ww_stream::tx::{Counted, Reliable};
use ww_stream::{Opened, Run, StreamHead};
use ww_version::{
    ApiHashPair, CompactVersion, FullVersion, FullVersionOwned, GlobalTypeId, Version, VersionOwned,
};

use super::device_e2e_tests::{
    DevSink, DevSource, HostRx, HostTx, PACKET, Packet, PipeTransport, RxFramer, TokioClock,
    TxFramer,
};
use crate::event_loop::command::{Command, EventLoopResidual};
use crate::event_loop::transport::Opened as TransportOpened;
use crate::{AckMode, Cause, Commander, PositionedStream, StreamEvent};

const STORE: usize = 16 * 1024;
const ITEM: usize = 64;
const RELIABLE: u32 = 0;
const COUNTED: u32 = 1;

/// Device state that outlives a connection, like a firmware's while the host program restarts
struct Dev {
    reliable: Reliable<RamRing<STORE>>,
    counted: Counted,
    sessions: u32,
    epoch: tokio::time::Instant,
}

impl Dev {
    fn new() -> Box<Self> {
        Box::new(Dev {
            reliable: Reliable::new(RamRing::new()),
            counted: Counted::new(),
            sessions: 0,
            epoch: tokio::time::Instant::now(),
        })
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

fn item(i: u32) -> Vec<u8> {
    let mut item = vec![0xAA; ITEM];
    item[..4].copy_from_slice(&i.to_le_bytes());
    item
}

fn index(value: &TailBytesOwned) -> u32 {
    u32::from_le_bytes(value.0[..4].try_into().unwrap())
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
        let PathKind::Absolute { path } = request.path_kind else {
            return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
        };
        let stream = path
            .iter()
            .next()
            .ok_or(ShrinkWrapError::OutOfBoundsRev)??
            .0;
        let now_ms = self.now_ms();
        let opened: Opened = match request.kind {
            RequestKind::StreamOpen { resume, .. } => match (stream, resume) {
                (RELIABLE, Some(cursor)) => self
                    .reliable
                    .resume(cursor)
                    .await
                    .map_err(|_| ShrinkWrapError::OutOfBoundsRev)?,
                (RELIABLE, None) => {
                    self.sessions += 1;
                    self.reliable
                        .open(self.sessions)
                        .await
                        .map_err(|_| ShrinkWrapError::OutOfBoundsRev)?
                }
                (_, Some(cursor)) => self
                    .counted
                    .resume(cursor)
                    .map_err(|_| ShrinkWrapError::OutOfBoundsRev)?,
                (_, None) => {
                    self.sessions += 1;
                    self.counted.open(self.sessions)
                }
            },
            RequestKind::StreamSideband { sideband } => {
                match (stream, sideband) {
                    (RELIABLE, StreamSideband::Close) => self.reliable.detach(),
                    (RELIABLE, sideband) => self
                        .reliable
                        .on_sideband(&sideband, now_ms)
                        .await
                        .map_err(|_| ShrinkWrapError::OutOfBoundsRev)?,
                    (_, StreamSideband::Close) => self.counted.detach(),
                    _ => {}
                }
                return Ok(&[]);
            }
            _ => return Err(ShrinkWrapError::OutOfBoundsRev),
        };
        let mut buf = [0u8; 64];
        let opened = opened.to_wire(&[]).to_ww_bytes(&mut buf)?;
        Event {
            seq: UVlq32(request.seq.0),
            result: Ok(EventKind::Value {
                data: TailBytes(opened),
            }),
        }
        .to_ww_bytes(scratch)
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

/// What the device's producer does
enum Produce {
    /// Items `from..to` into a stream
    Items { stream: u32, from: u32, to: u32 },
    /// Units `from..to` into the reliable stream, `units` per item (a batch), each item holding its first unit
    Batches { from: u32, to: u32, units: u32 },
}

fn stream_event(stream: u32, head: StreamHead, item: &[u8], buf: &mut [u8]) -> Vec<u8> {
    let path = [UNib32(stream)];
    Event {
        seq: UVlq32(0),
        result: Ok(EventKind::StreamItem {
            path: RefVec::Slice { slice: &path },
            head,
            data: TailBytes(item),
        }),
    }
    .to_ww_bytes(buf)
    .unwrap()
    .to_vec()
}

fn sideband_event(stream: u32, sideband: StreamSideband, buf: &mut [u8]) -> Vec<u8> {
    let path = [UNib32(stream)];
    Event {
        seq: UVlq32(0),
        result: Ok(EventKind::StreamSideband {
            path: RefVec::Slice { slice: &path },
            sideband,
        }),
    }
    .to_ww_bytes(buf)
    .unwrap()
    .to_vec()
}

/// One connection of the device: serves until the host goes, the state stays.
async fn run_device(
    mut dev: Box<Dev>,
    mut produce: mpsc::UnboundedReceiver<Produce>,
    to_host: mpsc::Sender<Packet>,
    from_host: mpsc::Receiver<Packet>,
) -> (Box<Dev>, mpsc::UnboundedReceiver<Produce>) {
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
    let mut tick = tokio::time::interval(Duration::from_millis(1));
    let mut read_buf = [0u8; 256];
    let mut event_buf = [0u8; 512];
    loop {
        tokio::select! {
            ready = server.wait() => {
                if let Some(LinkEvent::Down(_)) = server.handle(ready, &mut *dev).await {
                    dev.reliable.detach();
                    dev.counted.detach();
                    return (dev, produce);
                }
            }
            Some(p) = produce.recv() => {
                let (stream, from, to) = match p {
                    Produce::Items { stream, from, to } => (stream, from, to),
                    Produce::Batches { from, to, units } => {
                        for first in (from..to).step_by(units as usize) {
                            let units = units.min(to - first);
                            dev.reliable.push(&item(first), units as u64, None).await.unwrap();
                        }
                        continue;
                    }
                };
                for i in from..to {
                    if stream == RELIABLE {
                        dev.reliable.push(&item(i), 1, None).await.unwrap();
                    } else if let Some(head) = dev.counted.head(1, None) {
                        let ev = stream_event(COUNTED, head, &item(i), &mut event_buf);
                        if server.send(&ev).await.is_err() {
                            dev.counted.unsent(&head);
                        }
                    }
                }
            }
            _ = tick.tick() => {
                for run in [Run::Live, Run::Backfill] {
                    while let Some(out) = dev.reliable.next_event(run, &mut read_buf).await.unwrap() {
                        let ev = stream_event(RELIABLE, out.head, out.item, &mut event_buf);
                        _ = server.send(&ev).await;
                    }
                }
                let now_ms = dev.now_ms();
                if let Some(query) = dev.reliable.poll_query(now_ms) {
                    let ev = sideband_event(RELIABLE, query, &mut event_buf);
                    _ = server.send(&ev).await;
                }
            }
        }
    }
}

struct Connection {
    cmd: Commander,
    device: DeviceTask,
}

/// Connect a new host event loop to the device.
async fn connect_host(dev: Box<Dev>, produce: mpsc::UnboundedReceiver<Produce>) -> Connection {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let (device, _) = start_event_loop(dev, produce, &cmd_tx, cmd_rx).await;
    Connection {
        cmd: Commander::new(cmd_tx),
        device,
    }
}

type DeviceTask = tokio::task::JoinHandle<(Box<Dev>, mpsc::UnboundedReceiver<Produce>)>;

/// Start an event loop on `cmd_rx` and connect it to the device over a new pipe; the residual comes back when it
/// exits, to start the next one on the same command channel (a reconnect).
async fn start_event_loop(
    dev: Box<Dev>,
    produce: mpsc::UnboundedReceiver<Produce>,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: mpsc::Receiver<Command>,
) -> (DeviceTask, oneshot::Receiver<EventLoopResidual>) {
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(4);
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64);
    let device = tokio::spawn(run_device(dev, produce, dev_to_host_tx, host_to_dev_rx));
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

impl Connection {
    /// The host program exits: the device notices, keeps its state.
    async fn host_exits(self) -> (Box<Dev>, mpsc::UnboundedReceiver<Produce>) {
        let Connection { cmd, device } = self;
        tokio::task::spawn_blocking(move || drop(cmd))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), device)
            .await
            .expect("device notices the host is gone")
            .unwrap()
    }
}

fn path(stream: u32) -> PathKind<'static> {
    PathKind::Absolute {
        path: RefVec::Slice {
            slice: if stream == RELIABLE {
                &[UNib32(RELIABLE)]
            } else {
                &[UNib32(COUNTED)]
            },
        },
    }
}

async fn next(s: &mut PositionedStream<TailBytesOwned>) -> StreamEvent<TailBytesOwned> {
    tokio::time::timeout(Duration::from_secs(2), s.recv_event())
        .await
        .expect("an event in time")
        .unwrap()
}

/// Wait until the device's events stopped coming (the host queue doesn't grow anymore).
async fn settle(s: &PositionedStream<TailBytesOwned>) {
    let mut last = usize::MAX;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let queued = s.stats().queued;
        if queued == last {
            return;
        }
        last = queued;
    }
}

/// S4, reliable: a host that doesn't read stops granting, the device keeps the rest in its store; nothing lost when
/// it reads again, and the host's queue never holds more than the window.
#[tokio::test]
async fn s4_reliable_stuck_host_keeps_memory_bounded_and_loses_nothing() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let conn = connect_host(Dev::new(), produce_rx).await;
    let window = 1024;
    let mut s = conn
        .cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(RELIABLE),
            Some(ww_self::Delivery::Reliable),
            Ok(vec![]),
        )
        .window(window)
        .call()
        .await
        .unwrap();
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 0,
            to: 150,
        })
        .unwrap();
    settle(&s).await;
    let stats = s.stats();
    assert!(stats.queued > 0);
    // before the host's first Ack arrives, the device may send its initial grant
    let bound = (window as u64).max(ww_stream::tx::DEFAULT_INITIAL_GRANT) as usize + ITEM;
    assert!(
        stats.queued_bytes <= bound,
        "queue holds {} bytes, window {window}",
        stats.queued_bytes
    );
    for i in 0..150 {
        match next(&mut s).await {
            StreamEvent::Item {
                pos, value, run, ..
            } => {
                assert_eq!((pos, index(&value), run), (i, i as u32, Run::Live));
            }
            ev => panic!("item {i} expected, got {ev:?}"),
        }
    }
    let stats = s.stats();
    assert_eq!(stats.received, 150);
    assert_eq!(stats.lost_device + stats.lost_link + stats.lost_store, 0);
    conn.host_exits().await;
}

/// S4, reliable, more than the store holds: what didn't fit is reported exactly, as the device's loss.
#[tokio::test]
async fn s4_reliable_store_full_reports_exactly_what_was_dropped() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let conn = connect_host(Dev::new(), produce_rx).await;
    let mut s = conn
        .cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(RELIABLE),
            Some(ww_self::Delivery::Reliable),
            Ok(vec![]),
        )
        .window(1024)
        .call()
        .await
        .unwrap();
    let total = 600;
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 0,
            to: total,
        })
        .unwrap();
    settle(&s).await;
    let mut expect = 0u64;
    let mut lost = 0;
    let mut items = 0;
    let mut take = |ev: StreamEvent<TailBytesOwned>| match ev {
        StreamEvent::Item { pos, value, .. } => {
            assert_eq!(pos, expect);
            assert_eq!(index(&value) as u64, pos);
            expect = pos + 1;
            items += 1;
            pos
        }
        StreamEvent::Gap {
            at, units, cause, ..
        } => {
            assert_eq!((at, cause), (expect, Cause::Device));
            lost += units;
            expect = at + units;
            at
        }
        ev => panic!("{ev:?}"),
    };
    // what the store kept; the units dropped after it are only known with the next event
    while let Ok(ev) = tokio::time::timeout(Duration::from_millis(300), s.recv_event()).await {
        take(ev.unwrap());
    }
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: total,
            to: total + 1,
        })
        .unwrap();
    while take(next(&mut s).await) != total as u64 {}
    assert!(lost > 0);
    assert_eq!(items as u64 + lost, total as u64 + 1);
    assert_eq!(s.stats().lost_device, lost);
    conn.host_exits().await;
}

/// S4, counted: the host's bounded queue drops the newest and reports them as the host's loss.
#[tokio::test]
async fn s4_counted_stuck_host_drops_newest_and_reports_them() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let conn = connect_host(Dev::new(), produce_rx).await;
    let queue = 1024;
    let mut s = conn
        .cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(COUNTED),
            Some(ww_self::Delivery::Counted),
            Ok(vec![]),
        )
        .queue_bytes(queue)
        .call()
        .await
        .unwrap();
    let total = 200;
    produce_tx
        .send(Produce::Items {
            stream: COUNTED,
            from: 0,
            to: total,
        })
        .unwrap();
    settle(&s).await;
    assert!(s.stats().queued_bytes <= queue);
    // the next item that fits reports the ones dropped before it
    let mut expect = 0;
    let mut lost = 0;
    let mut items = 0;
    loop {
        match s.try_recv_event().unwrap() {
            Some(StreamEvent::Item { pos, value, .. }) => {
                assert_eq!(pos, expect);
                assert_eq!(index(&value) as u64, pos);
                expect = pos + 1;
                items += 1;
            }
            Some(ev) => panic!("{ev:?}"),
            None => break,
        }
    }
    assert_eq!(items, queue / ITEM);
    produce_tx
        .send(Produce::Items {
            stream: COUNTED,
            from: total,
            to: total + 1,
        })
        .unwrap();
    match next(&mut s).await {
        StreamEvent::Gap {
            at, units, cause, ..
        } => {
            assert_eq!((at, cause), (expect, Cause::Host));
            lost += units;
        }
        ev => panic!("{ev:?}"),
    }
    match next(&mut s).await {
        StreamEvent::Item { pos, .. } => assert_eq!(pos, total as u64),
        ev => panic!("{ev:?}"),
    }
    assert_eq!(items as u64 + lost, total as u64);
    assert_eq!(s.stats().lost_host, lost);
    conn.host_exits().await;
}

/// S6: a recorder acknowledges what it saved ([AckMode::Explicit]) and saves the cursor with it; after its host
/// program restarted it resumes from that cursor and gets everything after it once, nothing before.
#[tokio::test]
async fn s6_resume_after_host_restart_from_saved_cursor() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let conn = connect_host(Dev::new(), produce_rx).await;
    let mut s = conn
        .cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(RELIABLE),
            Some(ww_self::Delivery::Reliable),
            Ok(vec![]),
        )
        .ack_mode(AckMode::Explicit)
        .call()
        .await
        .unwrap();
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 0,
            to: 100,
        })
        .unwrap();
    for i in 0..100 {
        let StreamEvent::Item { pos, .. } = next(&mut s).await else {
            panic!("item {i}")
        };
        assert_eq!(pos, i);
    }
    // saved the first 60 to disk, crashed before saving the rest
    s.ack(60);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let cursor = s.cursor().unwrap();
    assert_eq!(cursor.pos.0, 60);
    drop(s);
    let (dev, produce_rx) = conn.host_exits().await;

    // the device goes on while the recorder is restarted
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 100,
            to: 150,
        })
        .unwrap();
    let conn = connect_host(dev, produce_rx).await;
    let mut s = conn
        .cmd
        .prepare_stream_resume::<TailBytesOwned>(
            path(RELIABLE),
            ww_self::Delivery::Reliable,
            cursor,
        )
        .call()
        .await
        .unwrap();
    match next(&mut s).await {
        StreamEvent::Resumed {
            cursor: c, ended, ..
        } => assert_eq!((c, ended), (cursor, None)),
        ev => panic!("{ev:?}"),
    }
    let mut seen = [false; 150];
    let mut backfill_done = false;
    let mut live_next = None;
    let mut backfill_next = 60;
    while seen[60..].iter().any(|s| !s) || !backfill_done {
        match next(&mut s).await {
            StreamEvent::Item {
                pos, value, run, ..
            } => {
                assert_eq!(index(&value) as u64, pos);
                assert!(!seen[pos as usize], "{pos} twice");
                seen[pos as usize] = true;
                // each run in order
                match run {
                    Run::Live => {
                        assert!(live_next.is_none_or(|n| n == pos));
                        live_next = Some(pos + 1);
                    }
                    Run::Backfill => {
                        assert_eq!(pos, backfill_next);
                        backfill_next = pos + 1;
                    }
                }
            }
            StreamEvent::BackfillDone => backfill_done = true,
            ev => panic!("{ev:?}"),
        }
    }
    assert!(seen[..60].iter().all(|s| !s), "nothing before the cursor");
    assert_eq!(s.stats().received, 90);
    conn.host_exits().await;
}

/// S6 with batches: the recorder saved part of a batch (the items of a batch one by one) and resumes from a cursor
/// inside it; the batch comes again whole, the stream gives it with only the units after the cursor, and every unit
/// after the cursor arrives once.
#[tokio::test]
async fn s6_resume_from_a_cursor_inside_a_batch() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let conn = connect_host(Dev::new(), produce_rx).await;
    let mut s = conn
        .cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(RELIABLE),
            Some(ww_self::Delivery::Reliable),
            Ok(vec![]),
        )
        .ack_mode(AckMode::Explicit)
        .call()
        .await
        .unwrap();
    produce_tx
        .send(Produce::Batches {
            from: 0,
            to: 100,
            units: 10,
        })
        .unwrap();
    for i in 0..10 {
        let StreamEvent::Item { pos, units, .. } = next(&mut s).await else {
            panic!("batch {i}")
        };
        assert_eq!((pos, units), (i * 10, 10));
    }
    // saved the first 35 units, crashed in the middle of the fourth batch
    s.ack(35);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let cursor = s.cursor().unwrap();
    assert_eq!(cursor.pos.0, 35);
    drop(s);
    let (dev, produce_rx) = conn.host_exits().await;

    produce_tx
        .send(Produce::Batches {
            from: 100,
            to: 150,
            units: 10,
        })
        .unwrap();
    let conn = connect_host(dev, produce_rx).await;
    let mut s = conn
        .cmd
        .prepare_stream_resume::<TailBytesOwned>(
            path(RELIABLE),
            ww_self::Delivery::Reliable,
            cursor,
        )
        .call()
        .await
        .unwrap();
    assert!(matches!(next(&mut s).await, StreamEvent::Resumed { .. }));
    let mut seen = [0u8; 150];
    let mut backfill_done = false;
    while seen[35..].contains(&0) || !backfill_done {
        match next(&mut s).await {
            StreamEvent::Item {
                pos, units, value, ..
            } => {
                // the value is the whole batch's, `pos` and `units` the part new to the host
                assert_eq!(index(&value) as u64, pos / 10 * 10);
                if pos == 35 {
                    assert_eq!(units, 5);
                }
                for u in pos..pos + units {
                    seen[u as usize] += 1;
                }
            }
            StreamEvent::BackfillDone => backfill_done = true,
            ev => panic!("{ev:?}"),
        }
    }
    assert!(
        seen[..35].iter().all(|&n| n == 0),
        "nothing before the cursor"
    );
    assert!(seen[35..].iter().all(|&n| n == 1), "{seen:?}");
    assert_eq!(s.stats().received, 115);
    conn.host_exits().await;
}

/// Reconnect after `disconnect().keep_streams()` (the device unplugged and plugged back, the host program kept
/// running): `resume_streams()` resumes the stream from where the handle is, the handle goes on with everything
/// the device kept meanwhile.
#[tokio::test]
async fn reconnect_with_keep_streams_resumes_streams() {
    let (produce_tx, produce_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let (device, residual) = start_event_loop(Dev::new(), produce_rx, &cmd_tx, cmd_rx).await;
    let cmd = Commander::new(cmd_tx.clone());
    let mut s = cmd
        .prepare_stream_open::<(), PositionedStream<TailBytesOwned>>(
            path(RELIABLE),
            Some(ww_self::Delivery::Reliable),
            Ok(vec![]),
        )
        .call()
        .await
        .unwrap();
    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 0,
            to: 50,
        })
        .unwrap();
    for i in 0..50 {
        let StreamEvent::Item { pos, .. } = next(&mut s).await else {
            panic!("item {i}")
        };
        assert_eq!(pos, i);
    }
    // acknowledged before the link goes
    tokio::time::sleep(Duration::from_millis(150)).await;
    cmd.disconnect().keep_streams().await.unwrap();
    let residual = residual.await.unwrap();
    let (dev, produce_rx) = device.await.unwrap();
    assert!(matches!(next(&mut s).await, StreamEvent::Disconnected));
    // the handle waits for a resume, it doesn't end
    assert!(s.try_recv_event().unwrap().is_none());

    produce_tx
        .send(Produce::Items {
            stream: RELIABLE,
            from: 50,
            to: 100,
        })
        .unwrap();
    let (_device, _residual) = start_event_loop(dev, produce_rx, &cmd_tx, residual.cmd_rx).await;
    assert_eq!(cmd.resume_streams().await.unwrap(), 1);
    match next(&mut s).await {
        StreamEvent::Resumed { cursor, ended, .. } => {
            assert_eq!((cursor.pos.0, ended), (50, None));
        }
        ev => panic!("{ev:?}"),
    }
    let mut seen = [false; 100];
    let mut backfill_done = false;
    while seen[50..].iter().any(|s| !s) || !backfill_done {
        match next(&mut s).await {
            StreamEvent::Item { pos, value, .. } => {
                assert_eq!(index(&value) as u64, pos);
                assert!(!seen[pos as usize], "{pos} twice");
                seen[pos as usize] = true;
            }
            StreamEvent::BackfillDone => backfill_done = true,
            ev => panic!("{ev:?}"),
        }
    }
    assert_eq!(s.stats().lost_device + s.stats().lost_store, 0);
    tokio::task::spawn_blocking(move || drop(cmd))
        .await
        .unwrap();
}
