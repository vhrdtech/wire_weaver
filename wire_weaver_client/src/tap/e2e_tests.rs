//! The tap end to end (TR-14): the host event loop against `ww_device::Server` over simulated USB (64 B packets,
//! real framers on both ends), recorded by a [Recorder], decoded live and again from the captured bytes.

use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use wire_weaver::shrink_wrap::UVlq32Backfill;
use ww_tape::FileBlocks;
use ww_version::{FullVersionOwned, VersionOwned};

use super::*;
use crate::event_loop::command::Command;
use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve};
use crate::in_process::{UsbSink, usb_device_framed};
use ww_link::{UsbFraming, UsbFramingProtocol, UsbSeqFraming};

const PACKET: usize = 64;

/// The device's IN endpoint, dropping one packet when told: `drop_at` counts packets down, the one at 0 is lost.
struct LossySink<F> {
    inner: UsbSink<F>,
    drop_at: Arc<AtomicIsize>,
}

impl<F: UsbFramingProtocol> ww_device::PacketSink for LossySink<F> {
    type Error = crate::in_process::Disconnected;
    type Framing = F;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), Self::Error> {
        if self.drop_at.fetch_sub(1, Ordering::SeqCst) == 0 {
            return Ok(());
        }
        self.inner.write_packet(packet).await
    }
}

fn request(i: u32) -> Vec<u8> {
    [0u8; UVlq32Backfill::LEN]
        .into_iter()
        .chain(i.to_le_bytes())
        .collect()
}

async fn send(
    cmd_tx: &mpsc::Sender<Command>,
    bytes: Vec<u8>,
    timeout: Duration,
) -> oneshot::Receiver<Result<Vec<u8>, crate::Error>> {
    let (done_tx, done_rx) = oneshot::channel();
    cmd_tx
        .send(Command::SendMessage {
            bytes,
            done_tx: Some((done_tx, timeout)),
        })
        .await
        .unwrap();
    done_rx
}

/// Everything decoded from `chunks`, as text to compare.
fn decode_all<'a>(blocks: impl Iterator<Item = ww_tape::Block<'a>>) -> (Vec<Decoded>, Stats) {
    let mut d = Decoder::default();
    for b in blocks {
        d.feed(&b);
    }
    d.finish();
    let mut out = vec![];
    while let Some(x) = d.next() {
        out.push(x);
    }
    (out, d.stats.take())
}

#[tokio::test]
async fn linkage_split_shared_lost_and_replay() {
    let stats = linkage::<UsbFraming>("tap/e2e").await;
    let rx = &stats.links[&0].rx;
    assert_eq!(rx.prefix, PrefixCounts::default(), "no prefix to check");
    assert!(!stats.report().contains("frame prefix"));
}

/// The same with the frame counter diagnostic: the lost packet shows as lost by its prefix, nothing else does.
#[tokio::test]
async fn frame_counter_counts_the_lost_packet() {
    let stats = linkage::<UsbSeqFraming>("tap/e2e_seq").await;
    let link = &stats.links[&0];
    assert_eq!(link.rx.prefix.lost, 1, "{:?}", link.rx.prefix);
    assert_eq!(link.rx.prefix.dropped(), 0);
    assert_eq!(link.rx.prefix.checked, link.rx.frames);
    assert_eq!((link.tx.prefix.lost, link.tx.prefix.dropped()), (0, 0));
    assert_eq!(link.rx.prefix_bytes, link.rx.frames);
    assert!(stats.report().contains("frame prefix: 1 lost, 0 dropped"));
}

/// Requests and replies over a simulated USB device with framing `F`, one reply packet lost; checks what both
/// framings have in common and returns the stats.
async fn linkage<F: UsbFramingProtocol + 'static>(path: &str) -> Stats {
    let (sink, source) = usb_device_framed::<F>(path, PACKET);
    let drop_at = Arc::new(AtomicIsize::new(isize::MAX));
    let sink = LossySink {
        inner: sink,
        drop_at: drop_at.clone(),
    };
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let dev = tokio::spawn(async move {
        let mut tx_frame = [0u8; PACKET];
        let mut rx = ww_device::RxBuffer::<PACKET, DEV_MAX_MESSAGE>::new();
        serve(
            ww_device::FramedTx::new(sink, &mut tx_frame),
            ww_device::FramedRx::new(source, rx.assembly_buf(PACKET)),
            events_tx,
        )
        .await
    });

    let recorder = Recorder::new(RecorderConfig::default());
    let sub = recorder.subscribe(Layers::ALL, 4096);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(1024);
    let host = tokio::spawn(crate::event_loop::core::worker(
        cmd_rx,
        crate::in_process::test_transport(path),
    ));
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle: Box::new(()),
            max_seq: crate::DEFAULT_MAX_SEQ,
            link_setup_timeout_ms: None,
            tap: Some(recorder.clone()),
            client_version: Box::new(FullVersionOwned::new(
                "test_api".into(),
                VersionOwned::new(0, 3, 0),
            )),
            connected_tx: Some(connected_tx),
            failed_tx: None,
        })
        .await
        .unwrap();
    connected_rx.await.unwrap().result.unwrap();
    assert!(events_rx.recv().await.is_some(), "link up on the device");

    // many small requests at once: several per host packet; 300 B replies split over several device packets
    const N: u32 = 40;
    let mut done = vec![];
    for i in 0..N {
        done.push(send(&cmd_tx, request(i), Duration::from_secs(5)).await);
    }
    for rx in done {
        assert_eq!(rx.await.unwrap().unwrap().len(), 300);
    }

    // one request alone, the second packet of its reply lost: the reply never comes
    drop_at.store(1, Ordering::SeqCst);
    let lost = send(&cmd_tx, request(1000), Duration::from_millis(300)).await;
    assert!(lost.await.unwrap().is_err(), "reply lost with its packet");
    // the link goes on
    let after = send(&cmd_tx, request(2000), Duration::from_secs(5)).await;
    assert_eq!(after.await.unwrap().unwrap().len(), 300);

    let (disconnected_tx, disconnected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::DisconnectAndExit {
            disconnected_tx: Some(disconnected_tx),
            reason: ww_link::DisconnectReason::RequestByUser,
        })
        .await
        .unwrap();
    disconnected_rx.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), host)
        .await
        .unwrap()
        .unwrap();
    dev.abort();
    recorder.flush();

    let mut chunks = vec![];
    while let Some(c) = sub.try_recv() {
        chunks.push(c);
    }
    let (live, stats) = decode_all(chunks.iter().map(|c| c.block()));

    // frame <-> message linkage
    let frames: Vec<&FrameInfo> = live
        .iter()
        .filter_map(|d| match &d.what {
            What::Frame(f) => Some(f),
            _ => None,
        })
        .collect();
    let messages: Vec<&MessageInfo> = live
        .iter()
        .filter_map(|d| match &d.what {
            What::Message(m) => Some(m),
            _ => None,
        })
        .collect();
    let shared = frames
        .iter()
        .find(|f| f.dir == Dir::Tx && f.outcome.completed.len() >= 3)
        .expect("a host packet with several requests");
    let in_shared = messages
        .iter()
        .filter(|m| m.dir == Dir::Tx && m.frames.iter().any(|p| p.frame_no == shared.no))
        .count();
    assert!(in_shared >= 3, "requests link back to their shared packet");
    let split: Vec<&&MessageInfo> = messages
        .iter()
        .filter(|m| m.dir == Dir::Rx && m.frames.len() >= 2)
        .collect();
    assert!(
        split.len() >= N as usize,
        "300 B replies over several packets"
    );
    for m in &split {
        let total: u32 = m.frames.iter().map(|p| p.len).sum();
        assert_eq!(total as usize, m.bytes.len(), "parts add up to the message");
    }
    // cross-check: every message that went through the framer is the one the transport saw
    for m in &messages {
        assert!(
            matches!(m.check, Check::Match | Check::NoFrames),
            "{:?} {:?} no {:?}",
            m.dir,
            m.check,
            m.no
        );
    }
    let linked = messages.iter().filter(|m| m.check == Check::Match).count();
    assert!(linked >= 2 * N as usize, "requests and replies are linked");
    // replies paired with their requests by seq
    let replies = messages
        .iter()
        .filter(|m| m.api.as_ref().is_some_and(|a| a.reply_to.is_some()))
        .count();
    assert_eq!(replies, N as usize + 1);

    // the lost packet: the reply's later pieces don't fit, its partial message is dropped
    let rx = &stats.links[&0].rx;
    assert!(rx.partial_dropped + rx.dropped_pieces >= 1, "{rx:?}");
    assert_eq!(rx.mismatches, 0);
    let tx = &stats.links[&0].tx;
    assert!(tx.messages >= N as u64 && tx.frames < tx.messages, "{tx:?}");
    assert!(rx.split_messages >= N as u64 && rx.split_frames > rx.split_messages);
    assert!(rx.fill_ratio().unwrap() > 0.5);
    assert!(tx.latency_count > 0);
    assert!(stats.report().contains("split over"));

    // a capture is the blocks back to back; read back, it decodes to the same
    let mut file = vec![];
    for c in &chunks {
        file.extend_from_slice(c.bytes());
    }
    let mut rd = FileBlocks::new(&file[..]);
    let mut blocks = vec![];
    while let Some(b) = rd.next_block() {
        blocks.push(ww_tape::BlockBuf::new(b.unwrap().bytes().to_vec()).unwrap());
    }
    let (replayed, replay_stats) = decode_all(blocks.iter().map(|b| b.block()));
    assert_eq!(format!("{replayed:?}"), format!("{live:?}"));
    assert_eq!(replay_stats, stats);
    stats
}
