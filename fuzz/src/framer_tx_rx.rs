#![no_main]

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use ww_framer::{FramedRx, Tx, framed::U2Head, traits::NopTail};

const MAX_FRAMES: usize = 512;
const MAX_SMALL_MSG: usize = 32;
const MAX_LARGE_MSG: usize = 1024 + 128;
const MIN_FRAME: usize = 6; // 4 enough with default features, 5 for "large", 6 for "very_large" (U2Head)
const MAX_FRAME: usize = 1024;
/// max message + max frame + some headroom
const RX_BUF: usize = MAX_LARGE_MSG + MAX_FRAME;
// type CHECKSUM = ww_framer::traits::NopChecksum;
type CHECKSUM = ww_framer::crc::CrcChecksum<ww_framer::crc::Crc16IbmSdlc>;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);

    let num_frames: usize = u.int_in_range(1..=MAX_FRAMES).unwrap_or(1);
    let frame_size: usize = u.int_in_range(MIN_FRAME..=MAX_FRAME).unwrap_or(MIN_FRAME);

    // ---------- Tx side: generate messages, collect frames ----------
    let mut tx_buf = vec![0u8; frame_size];
    let mut tx = Tx::<U2Head, CHECKSUM, NopTail>::new(&mut tx_buf);

    let mut expected: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut frames: Vec<Vec<u8>> = Vec::new();

    'outer: while frames.len() < num_frames {
        // number of messages to write before an explicit flush, to get partially filled frames
        let batch: usize = u.int_in_range(1..=8).unwrap_or(1);
        for _ in 0..batch {
            let user_kind: u8 = u.arbitrary().unwrap_or(0);
            let len: usize = if u.ratio(1, 16).unwrap_or(false) {
                u.int_in_range(1024..=MAX_LARGE_MSG).unwrap_or(0)
            } else {
                u.int_in_range(0..=MAX_SMALL_MSG).unwrap_or(0)
            };
            let mut message = vec![0u8; len];
            // fill with whatever is left in the seed, rest stays zero
            let avail = u.len().min(len);
            if avail > 0 {
                message[..avail].copy_from_slice(u.bytes(avail).unwrap());
            }
            // make content distinguishable even when seed is exhausted
            for (i, b) in message.iter_mut().enumerate().skip(avail) {
                *b = (i as u8).wrapping_mul(31) ^ (expected.len() as u8);
            }

            loop {
                match tx.write(user_kind, &message) {
                    Ok(true) => break,
                    Ok(false) => {
                        let n = tx.flush();
                        assert!(n > 0, "Ok(false) without any bytes written");
                        frames.push(tx.buf()[..n].to_vec());
                    }
                    Err(()) => panic!(
                        "Tx refused message: frame_size={frame_size} user_kind={user_kind} len={len}"
                    ),
                }
            }
            expected.push((user_kind, message));

            if frames.len() >= num_frames {
                break 'outer;
            }
        }
        let n = tx.flush();
        if n > 0 {
            frames.push(tx.buf()[..n].to_vec());
        }
    }
    // flush whatever is left from the last message
    let n = tx.flush();
    if n > 0 {
        frames.push(tx.buf()[..n].to_vec());
    }

    // ---------- Rx side: deliver frames in bursts, compare ----------
    let mut rx_buf = vec![0u8; RX_BUF];
    let mut rx = FramedRx::<U2Head, CHECKSUM, NopTail>::new(&mut rx_buf);

    let mut received = 0usize;
    let mut frame_idx = 0usize;
    while frame_idx < frames.len() {
        // simulate 1 or several frames arriving at once
        let burst: usize = u.int_in_range(1..=8).unwrap_or(1);
        let burst = burst.min(frames.len() - frame_idx);

        for frame in &frames[frame_idx..frame_idx + burst] {
            assert!(
                frame.len() <= rx.free(),
                "not enough space: free={} frame={} frame_idx={frame_idx}",
                rx.free(),
                frame.len()
            );
            rx.stage(frame).unwrap();

            // frame boundaries are significant for Start/Continue, so reassemble after each frame
            loop {
                rx.reassemble();
                let Some((user_kind, message)) = rx.message() else {
                    break;
                };
                let Some((exp_kind, exp_msg)) = expected.get(received) else {
                    panic!(
                        "received extra message #{received}: kind={user_kind} len={}",
                        message.len()
                    );
                };
                assert_eq!(
                    user_kind, *exp_kind,
                    "user_kind mismatch at message #{received} (frame {frame_idx})"
                );
                assert_eq!(
                    message,
                    exp_msg.as_slice(),
                    "payload mismatch at message #{received} (frame {frame_idx})"
                );
                received += 1;
            }
        }
        frame_idx += burst;
    }

    assert_eq!(
        received,
        expected.len(),
        "not all messages received: {received}/{} over {} frames of size {frame_size}",
        expected.len(),
        frames.len()
    );
    assert_eq!(rx.free(), RX_BUF, "rx did not return to an idle state");
});
