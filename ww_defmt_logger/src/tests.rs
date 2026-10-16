//! The ring, the frame packing and the loss accounting on the host, single threaded (exact positions of losses)
//! and with concurrent producers (std `critical-section`: one global lock, like interrupts that can't nest).

use super::*;
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::vec::Vec;
use wire_weaver::shrink_wrap::DeserializeShrinkWrap;
use ww_stream::StreamHead;
use ww_stream::store::{PushError, StoreState, StoredEvent};

/// The logger and the stores never wait: poll until ready.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// Keeps every event, never full.
#[derive(Default)]
struct VecStore {
    events: Vec<(StreamHead, Vec<u8>)>,
    session: u32,
}

impl StreamStore for VecStore {
    type Error = ();

    async fn push(&mut self, head: &StreamHead, item: &[u8]) -> Result<(), PushError<()>> {
        self.events.push((*head, item.to_vec()));
        Ok(())
    }

    async fn read<'b>(&mut self, _: u64, _: &'b mut [u8]) -> Result<Option<StoredEvent<'b>>, ()> {
        Ok(None)
    }

    async fn release(&mut self, _: u64) -> Result<(), ()> {
        Ok(())
    }

    fn state(&self) -> StoreState {
        StoreState {
            session: self.session,
            oldest: self.events.first().map(|e| e.0.pos),
            newest: self.events.last().map(|e| e.0.pos),
            next: self.events.last().map(|e| e.0.end()).unwrap_or(0),
            ..StoreState::default()
        }
    }

    async fn reset(&mut self, session: u32) -> Result<(), ()> {
        self.session = session;
        self.events.clear();
        Ok(())
    }
}

fn stream() -> Reliable<VecStore> {
    let mut stream = Reliable::new(VecStore::default());
    block_on(stream.start_session(1)).unwrap();
    stream
}

fn log<const N: usize>(logger: &DefmtLogger<N>, payload: &[u8]) {
    logger.acquire();
    // safety: between acquire and release
    unsafe {
        logger.write(payload);
        logger.release();
    }
}

/// The frame `defmt` would send for `payload`, in the encoding it is built with, after its first frame (rzCOBS
/// puts a lone `0x00` in front of the very first one).
fn encoded(payload: &[u8]) -> Vec<u8> {
    let mut mirror = Mirror::new();
    mirror.frame(&[]);
    mirror.frame(payload)
}

/// Encodes like the logger does, one encoder for all frames.
struct Mirror(defmt::Encoder);

impl Mirror {
    fn new() -> Self {
        Mirror(defmt::Encoder::new())
    }

    fn frame(&mut self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        self.0.start_frame(|b| out.extend_from_slice(b));
        self.0.write(payload, |b| out.extend_from_slice(b));
        self.0.end_frame(|b| out.extend_from_slice(b));
        out
    }
}

/// Item bytes of every stored event.
fn items(stream: &Reliable<VecStore>) -> Vec<(StreamHead, Vec<u8>)> {
    stream
        .store()
        .events
        .iter()
        .map(|(head, item)| {
            let frames = DefmtFrames::from_ww_bytes(item).unwrap();
            (
                *head,
                frames
                    .bytes
                    .into_inner()
                    .iter()
                    .map(Result::unwrap)
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn frames_packed_whole() {
    let logger = DefmtLogger::<1024>::new();
    let mut stream = stream();
    let payloads: Vec<Vec<u8>> = (0..40u8).map(|i| vec![i; 30]).collect();
    let mut mirror = Mirror::new();
    let frames: Vec<Vec<u8>> = payloads.iter().map(|p| mirror.frame(p)).collect();
    for p in &payloads {
        log(&logger, p);
    }
    // the ring holds what fits, the rest is dropped and reported after the frames
    let mut used = 0;
    let kept = frames
        .iter()
        .take_while(|f| {
            used += FRAME_OVERHEAD + f.len();
            used <= 1024
        })
        .count();
    assert!(kept < payloads.len());
    assert_eq!(block_on(logger.pump(&mut stream)), Ok(kept));
    let events = items(&stream);
    let expected: Vec<u8> = frames[..kept].concat();
    let got: Vec<u8> = events.iter().flat_map(|(_, b)| b.clone()).collect();
    assert_eq!(got, expected);
    assert!(events.iter().all(|(_, b)| b.len() <= MAX_FRAMES_LEN));
    let units: u64 = events.iter().map(|(h, _)| h.units).sum();
    assert_eq!(units, kept as u64);
    assert_eq!(logger.dropped_frames() as usize, payloads.len() - kept);
    // trailing drops wait in `pending_lost` for the next event
    assert_eq!(stream.next_pos(), payloads.len() as u64);
    log(&logger, b"after");
    block_on(logger.pump(&mut stream)).unwrap();
    let (last, bytes) = items(&stream).pop().unwrap();
    assert_eq!(last.lost, Some((payloads.len() - kept) as u64));
    assert_eq!(last.pos, payloads.len() as u64);
    assert_eq!(bytes, encoded(b"after"));
}

#[test]
fn loss_reported_in_place() {
    let logger = DefmtLogger::<256>::new();
    let mut stream = stream();
    log(&logger, &[1; 100]);
    log(&logger, &[2; 100]);
    log(&logger, &[3; 100]); // doesn't fit
    log(&logger, &[4; 100]); // doesn't fit
    block_on(logger.pump(&mut stream)).unwrap();
    log(&logger, &[5; 10]);
    log(&logger, &vec![6; MAX_FRAMES_LEN + 1]); // too long ever
    log(&logger, &[7; 10]);
    block_on(logger.pump(&mut stream)).unwrap();
    let events = items(&stream);
    let heads: Vec<(u64, u64, Option<u64>)> = events
        .iter()
        .map(|(h, _)| (h.pos, h.units, h.lost))
        .collect();
    // [1, 2] | lost 2 | [5] | lost 1 | [7]
    assert_eq!(heads, vec![(0, 2, None), (4, 1, Some(2)), (6, 1, Some(1))]);
    assert_eq!(events[1].1, encoded(&[5; 10]));
    assert_eq!(events[2].1, encoded(&[7; 10]));
    assert_eq!(logger.dropped_frames(), 3);
    assert_eq!(logger.pending_bytes(), 0);
}

#[test]
fn ring_wraps() {
    let logger = DefmtLogger::<64>::new();
    let mut stream = stream();
    let mut expected = Vec::new();
    let mut mirror = Mirror::new();
    for i in 0..200u8 {
        let p = vec![i | 1; (i % 13) as usize + 1];
        log(&logger, &p);
        expected.extend(mirror.frame(&p));
        block_on(logger.pump(&mut stream)).unwrap();
    }
    let got: Vec<u8> = items(&stream).into_iter().flat_map(|(_, b)| b).collect();
    assert_eq!(got, expected);
    assert_eq!(logger.dropped_frames(), 0);
}

#[test]
fn encoding_matches_encoder() {
    let frame = encoded(&[0x55]);
    let expected = if frame == [0x55] {
        DefmtEncoding::Raw
    } else {
        DefmtEncoding::Rzcobs
    };
    assert_eq!(encoding(), expected);
}

#[test]
fn max_item_serializes() {
    let pack = [0xffu8; MAX_FRAMES_LEN];
    let item = DefmtFrames {
        bytes: MaxLen::new_unchecked(RefVec::new_bytes(&pack)),
    };
    let mut buf = [0u8; SER_LEN];
    assert!(item.to_ww_bytes(&mut buf).is_ok());
}

/// Splits concatenated frames: rzCOBS frames end with a `0x00` (the very first one also starts with one, dropped
/// here); raw frames here start with their own length.
fn split_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    match encoding() {
        DefmtEncoding::Rzcobs => {
            let bytes = bytes.strip_prefix(&[0]).unwrap_or(bytes);
            for f in bytes.split_inclusive(|&b| b == 0) {
                assert_eq!(f.last(), Some(&0), "half a frame");
                frames.push(f.to_vec());
            }
        }
        _ => {
            let mut rest = bytes;
            while !rest.is_empty() {
                let (f, r) = rest.split_at(rest[0] as usize);
                frames.push(f.to_vec());
                rest = r;
            }
        }
    }
    frames
}

/// Payload of frame `seq` of producer `t`: its length first (raw frames are split by it), no zeros.
fn payload(t: u8, seq: u32) -> Vec<u8> {
    let len = 8 + (seq as usize * 7 + t as usize) % 41;
    let mut p = vec![len as u8, t + 1];
    p.extend((seq + 1).to_le_bytes().map(|b| b | 0x80));
    p.extend((p.len()..len).map(|i| (i as u8) | 0x40));
    p
}

/// Producers on several threads log as fast as they can into a small ring while a consumer pumps at its own pace:
/// every frame arrives whole and in order per producer, none twice, and received + lost is exactly what was logged.
/// Nothing depends on how fast the threads run (a loaded PC starves the consumer, an idle one keeps up): a burst
/// bigger than the ring before the consumer starts makes sure frames are dropped, and the consumer drains everything
/// after the producers stop.
#[test]
fn stress_concurrent_producers() {
    const THREADS: u8 = 4;
    const PER_THREAD: u32 = 20_000;
    /// Producer id and frames of the burst before the consumer starts: far more than 512 bytes
    const BURST_T: u8 = THREADS;
    const BURST: u32 = 64;
    static LOGGER: DefmtLogger<512> = DefmtLogger::new();

    let mut known = HashMap::new();
    for t in 0..THREADS {
        for seq in 0..PER_THREAD {
            known.insert(encoded(&payload(t, seq)), (t, seq));
        }
    }
    for seq in 0..BURST {
        known.insert(encoded(&payload(BURST_T, seq)), (BURST_T, seq));
        log(&LOGGER, &payload(BURST_T, seq));
    }
    assert!(LOGGER.dropped_frames() > 0, "the burst fits the ring");

    let done = Arc::new(AtomicBool::new(false));
    let consumer = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut stream = stream();
            let mut pumps = 0u64;
            loop {
                let finished = done.load(Ordering::Acquire);
                block_on(LOGGER.pump(&mut stream)).unwrap();
                pumps += 1;
                if finished {
                    break;
                }
                if pumps.is_multiple_of(3) {
                    std::thread::yield_now();
                }
            }
            stream
        })
    };
    let producers: Vec<_> = (0..THREADS)
        .map(|t| {
            std::thread::spawn(move || {
                for seq in 0..PER_THREAD {
                    log(&LOGGER, &payload(t, seq));
                }
            })
        })
        .collect();
    for p in producers {
        p.join().unwrap();
    }
    done.store(true, Ordering::Release);
    let mut stream = consumer.join().unwrap();
    // one more frame carries the trailing losses
    log(&LOGGER, &payload(0, PER_THREAD));
    known.insert(encoded(&payload(0, PER_THREAD)), (0, PER_THREAD));
    block_on(LOGGER.pump(&mut stream)).unwrap();

    let events = items(&stream);
    let mut next_pos = 0;
    let mut received = 0u64;
    let mut lost = 0u64;
    let mut last_seq: HashMap<u8, u32> = HashMap::new();
    for (head, bytes) in &events {
        assert_eq!(
            head.pos,
            next_pos + head.lost.unwrap_or(0),
            "positions contiguous"
        );
        next_pos = head.end();
        lost += head.lost.unwrap_or(0);
        let frames = split_frames(bytes);
        assert_eq!(frames.len() as u64, head.units, "units = frames");
        for f in frames {
            let &(t, seq) = known.get(&f).expect("a frame as logged, not torn or mixed");
            if let Some(prev) = last_seq.insert(t, seq) {
                assert!(seq > prev, "producer {t}: {seq} after {prev}");
            }
            received += 1;
        }
    }
    let logged = THREADS as u64 * PER_THREAD as u64 + BURST as u64 + 1;
    assert_eq!(received + lost, logged);
    assert_eq!(lost, LOGGER.dropped_frames() as u64);
    assert_eq!(LOGGER.pending_bytes(), 0, "drained");
    println!(
        "logged {logged}, received {received}, lost {lost}, events {}",
        events.len()
    );
}

/// A probe set the RTT channel to block (as `probe-rs` does when it attaches) and doesn't read: logging goes on,
/// whole frames that fit reach the channel, the rest are skipped and counted, the ring keeps them all.
#[cfg(feature = "rtt")]
#[test]
fn rtt_tee_never_blocks() {
    const RTT_LEN: usize = 64;
    let channels = rtt_target::rtt_init! {
        up: { 0: { size: RTT_LEN, mode: rtt_target::ChannelMode::BlockIfFull, name: "defmt" } }
    };
    let logger = DefmtLogger::<1024>::new();
    logger.set_rtt_channel(channels.up.0);

    let mut mirror = Mirror::new();
    // an RTT buffer holds one byte less than its size
    let mut room = RTT_LEN - 1;
    let mut expect_skipped = 0;
    for i in 0..20u8 {
        let payload = [i; 9];
        // a blocking write would spin here forever: nobody reads the channel
        log(&logger, &payload);
        let frame = mirror.frame(&payload);
        if frame.len() <= room {
            room -= frame.len();
        } else {
            expect_skipped += 1;
        }
    }
    assert!(expect_skipped > 0);
    assert_eq!(logger.rtt_skipped(), expect_skipped);
    assert_eq!(logger.dropped_frames(), 0);
    let mut stream = stream();
    assert_eq!(block_on(logger.pump(&mut stream)), Ok(20));
}
