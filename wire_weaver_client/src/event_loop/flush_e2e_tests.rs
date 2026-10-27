//! Latency of the frame accumulation window, host event loop against [ww_device::Server] over the in-memory packet
//! pipe of [device_e2e_tests](super::device_e2e_tests), on paused time: default priority traffic goes out when a
//! frame is full or its window is over, on both sides, also a lone request and a deferred reply; an urgent request
//! (CLT-50) and its reply, immediate or deferred (SRV-42), go out at once.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    BufReader, Error as ShrinkWrapError, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use wire_weaver::{EventOut, EventWriter, MessageSink, Priority, WireWeaverAsyncApiBackend};
use ww_client_server::{Event, EventKind};
use ww_device::LinkConfig;
use ww_version::{ApiHashPair, CompactVersion, FullVersion, GlobalTypeId, Version};

use super::command::Command;
use super::device_e2e_tests::{
    DevSink, HostRx, HostTx, PACKET, Packet, PipeTransport, RxFramer, TokioClock, TxFramer, connect,
};
use super::transport::Opened;

/// Much longer than anything in these tests takes on paused time: waiting for it shows in the elapsed time.
const WINDOW: Duration = Duration::from_millis(50);

/// Requests with this byte after the priority nibble's byte are answered later, from outside of `Server::handle`.
const DEFERRED: u8 = 0xDE;

/// Answers with the request's bytes after its priority byte and the kind byte, at once, or deferred: seq and
/// priority go to the device loop.
struct Backend {
    deferred: mpsc::UnboundedSender<(u32, Priority)>,
}

impl WireWeaverAsyncApiBackend for Backend {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        _out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        let mut rd = BufReader::new(data);
        let seq = rd.read_uvlq32()?;
        let priority = Priority::from_nibble(rd.read_nib()?);
        let data = &data[data.len() - rd.bytes_left()..];
        if data.first() == Some(&DEFERRED) {
            _ = self.deferred.send((seq, priority));
            return Ok(&[]);
        }
        reply(seq, &data[1..]).to_ww_bytes(scratch)
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("test_api", Version::new(0, 3, 1))
    }
}

fn reply(seq: u32, data: &[u8]) -> Event<'_> {
    Event {
        seq: UVlq32(seq),
        result: Ok(EventKind::Value {
            data: TailBytes(data),
        }),
    }
}

/// Counts the packets the host sent.
struct CountingSource {
    rx: mpsc::Receiver<Packet>,
    packets: Arc<AtomicU32>,
}

impl ww_device::PacketSource for CountingSource {
    type Error = ();
    type Framing = ww_link::UsbFraming;
    fn max_packet_len(&self) -> usize {
        PACKET
    }
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let p = self.rx.recv().await.ok_or(())?;
        self.packets.fetch_add(1, Ordering::Relaxed);
        buf[..p.len()].copy_from_slice(&p);
        Ok(p.len())
    }
    async fn wait_connected(&mut self) {}
}

async fn device(to_host: mpsc::Sender<Packet>, from_host: CountingSource) {
    let mut tx_frame = [0u8; PACKET];
    let mut rx = ww_device::RxBuffer::<{ 2 * PACKET }, 1024>::new();
    let tx = ww_device::FramedTx::new(DevSink(to_host), &mut tx_frame);
    let rx = ww_device::FramedRx::new(from_host, rx.assembly_buf(PACKET));
    let mut scratch = [0u8; 1024];
    let mut event_scratch = [0u8; 512];
    let mut config = LinkConfig::new(
        FullVersion::new("test_api", Version::new(0, 3, 1)),
        ApiHashPair::empty(),
        CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
    );
    config.accumulation_time = WINDOW;
    let mut server = ww_device::Server::new(
        config,
        tx,
        rx,
        TokioClock(Instant::now()),
        &mut scratch,
        &mut event_scratch,
    );
    let (deferred_tx, mut deferred_rx) = mpsc::unbounded_channel();
    let mut backend = Backend {
        deferred: deferred_tx,
    };
    loop {
        tokio::select! {
            ready = server.wait() => {
                server.handle(ready, &mut backend).await;
            }
            deferred = deferred_rx.recv() => {
                // what a generated `<method>_send_return` does
                let (seq, priority) = deferred.unwrap();
                server
                    .sink()
                    .send_reply_with(priority, |scratch| {
                        Ok(reply(seq, b"later").to_ww_bytes(scratch)?.len())
                    })
                    .await
                    .unwrap();
            }
        }
    }
}

/// Host event loop and device, connected; the count of packets the host sent.
async fn start() -> (mpsc::Sender<Command>, Arc<AtomicU32>) {
    let (host_to_dev_tx, host_to_dev_rx) = mpsc::channel::<Packet>(64);
    let (dev_to_host_tx, dev_to_host_rx) = mpsc::channel::<Packet>(64);
    let packets = Arc::new(AtomicU32::new(0));
    tokio::spawn(device(
        dev_to_host_tx,
        CountingSource {
            rx: host_to_dev_rx,
            packets: packets.clone(),
        },
    ));
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    tokio::spawn(crate::event_loop::core::worker(
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
    connect(&cmd_tx, (0, 3)).await.result.unwrap();
    (cmd_tx, packets)
}

/// Sends a request at `priority`, `deferred` or answered at once, with `data` to echo back.
async fn send(
    cmd_tx: &mpsc::Sender<Command>,
    priority: Priority,
    deferred: bool,
    data: &[u8],
) -> oneshot::Receiver<Result<Vec<u8>, crate::Error>> {
    let (done_tx, done_rx) = oneshot::channel();
    let kind = if deferred { DEFERRED } else { 0 };
    // the priority nibble, then (nibble aligned) the kind byte
    let bytes = [0u8; UVlq32Backfill::LEN]
        .into_iter()
        .chain([priority.level() << 4, kind])
        .chain(data.iter().copied())
        .collect();
    cmd_tx
        .send(Command::SendMessage {
            bytes,
            done_tx: Some((done_tx, Duration::from_secs(5))),
        })
        .await
        .unwrap();
    done_rx
}

/// How long a request and its answer took, checking the answer.
async fn round_trip(
    cmd_tx: &mpsc::Sender<Command>,
    priority: Priority,
    deferred: bool,
    i: u8,
) -> Duration {
    let t = Instant::now();
    let bytes = send(cmd_tx, priority, deferred, &[i])
        .await
        .await
        .unwrap()
        .unwrap();
    match deferred {
        true => assert_eq!(bytes, b"later"),
        false => assert_eq!(bytes, [i]),
    }
    t.elapsed()
}

/// tokio's paused clock moves in whole milliseconds: a window may end up to one later, per side.
const TICKS: Duration = Duration::from_millis(2);

/// A lone default priority request waits for the window on the host, its reply waits for it on the device: no
/// faster path for a request alone, and not later than the window either.
#[tokio::test(start_paused = true)]
async fn default_request_waits_the_window_on_both_sides() {
    let (cmd_tx, _) = start().await;
    for i in 0..3u8 {
        let rtt = round_trip(&cmd_tx, Priority::NORMAL, false, i).await;
        assert!(rtt >= 2 * WINDOW, "{rtt:?}");
        assert!(rtt <= 2 * WINDOW + TICKS, "{rtt:?}");
    }
}

/// A deferred reply of default priority, sent through `Server::sink()`, waits for the window as any other.
#[tokio::test(start_paused = true)]
async fn default_deferred_reply_waits_the_window() {
    let (cmd_tx, _) = start().await;
    let rtt = round_trip(&cmd_tx, Priority::NORMAL, true, 0).await;
    assert!(rtt >= 2 * WINDOW, "{rtt:?}");
    assert!(rtt <= 2 * WINDOW + TICKS, "{rtt:?}");
}

/// An urgent request leaves the host at once, and its reply the device.
#[tokio::test(start_paused = true)]
async fn urgent_request_and_reply_go_at_once() {
    let (cmd_tx, _) = start().await;
    for (i, p) in [Priority::URGENT, Priority::HIGHEST]
        .into_iter()
        .enumerate()
    {
        let rtt = round_trip(&cmd_tx, p, false, i as u8).await;
        assert!(rtt < TICKS, "{p:?}: {rtt:?}");
    }
}

/// A deferred reply to an urgent request goes out at once, from outside of `Server::handle`.
#[tokio::test(start_paused = true)]
async fn urgent_deferred_reply_goes_at_once() {
    let (cmd_tx, _) = start().await;
    for i in 0..3u8 {
        let rtt = round_trip(&cmd_tx, Priority::URGENT, true, i).await;
        assert!(rtt < TICKS, "{rtt:?}");
    }
}

/// Requests sent together share frames: far fewer packets than requests.
#[tokio::test(start_paused = true)]
async fn requests_sent_together_share_frames() {
    let (cmd_tx, packets) = start().await;
    const N: u8 = 30;
    let before = packets.load(Ordering::Relaxed);
    let mut done = vec![];
    for i in 0..N {
        done.push(send(&cmd_tx, Priority::NORMAL, false, &[i]).await);
    }
    for (i, rx) in done.into_iter().enumerate() {
        assert_eq!(rx.await.unwrap().unwrap(), [i as u8]);
    }
    let sent = packets.load(Ordering::Relaxed) - before;
    // a request is 4 bytes here (seq, priority, kind and one data byte) plus framing: several fit a packet
    assert!(sent <= N as u32 / 4, "{N} requests in {sent} packets");
}
