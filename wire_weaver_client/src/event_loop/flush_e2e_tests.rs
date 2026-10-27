//! Latency of the frame accumulation window, host event loop against [ww_device::Server] over the in-memory packet
//! pipe of [device_e2e_tests](super::device_e2e_tests), on paused time: a device asking for a long window must not
//! make a lone request (CLT-50) or a deferred reply (SRV-42) wait for it, while requests sent together still share
//! frames.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    BufReader, Error as ShrinkWrapError, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use wire_weaver::{EventOut, EventWriter, MessageSink, WireWeaverAsyncApiBackend};
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

/// Requests starting with this byte are answered later, from outside of `Server::handle`.
const DEFERRED: u8 = 0xDE;

/// Answers with the request's bytes after seq, at once, or deferred: their seq goes to the device loop.
struct Backend {
    deferred: mpsc::UnboundedSender<u32>,
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
        let data = &data[data.len() - rd.bytes_left()..];
        if data.first() == Some(&DEFERRED) {
            _ = self.deferred.send(seq);
            return Ok(&[]);
        }
        reply(seq, data).to_ww_bytes(scratch)
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
            seq = deferred_rx.recv() => {
                // what a generated `<method>_send_return` does
                let seq = seq.unwrap();
                server
                    .sink()
                    .send_reply_with(|scratch| {
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

async fn send(
    cmd_tx: &mpsc::Sender<Command>,
    data: &[u8],
) -> oneshot::Receiver<crate::Result<Vec<u8>>> {
    let (done_tx, done_rx) = oneshot::channel();
    let bytes = [0u8; UVlq32Backfill::LEN]
        .into_iter()
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

/// A lone request leaves the host at once, and its reply the device, whatever window the device asked for.
#[tokio::test(start_paused = true)]
async fn lone_request_does_not_wait_for_the_window() {
    let (cmd_tx, _) = start().await;
    for i in 0..3u8 {
        let t = Instant::now();
        let bytes = send(&cmd_tx, &[i]).await.await.unwrap().unwrap();
        assert_eq!(bytes, [i]);
        assert!(t.elapsed() < WINDOW, "waited {:?}", t.elapsed());
    }
}

/// A deferred reply sent through `Server::sink()` goes out at once, as an immediate reply does.
#[tokio::test(start_paused = true)]
async fn deferred_reply_does_not_wait_for_the_window() {
    let (cmd_tx, _) = start().await;
    for i in 0..3u8 {
        let t = Instant::now();
        let bytes = send(&cmd_tx, &[DEFERRED, i]).await.await.unwrap().unwrap();
        assert_eq!(bytes, b"later");
        assert!(t.elapsed() < WINDOW, "waited {:?}", t.elapsed());
    }
}

/// Requests sent together still share frames: far fewer packets than requests.
#[tokio::test(start_paused = true)]
async fn requests_sent_together_share_frames() {
    let (cmd_tx, packets) = start().await;
    const N: u8 = 30;
    let before = packets.load(Ordering::Relaxed);
    let mut done = vec![];
    for i in 0..N {
        done.push(send(&cmd_tx, &[i]).await);
    }
    for (i, rx) in done.into_iter().enumerate() {
        assert_eq!(rx.await.unwrap().unwrap(), [i as u8]);
    }
    let sent = packets.load(Ordering::Relaxed) - before;
    // a request is 2 bytes here (seq and one data byte) plus framing: several fit a packet
    assert!(sent <= N as u32 / 4, "{N} requests in {sent} packets");
}
