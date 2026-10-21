#![no_main]

//! `ww_framer` with `SeqPrefix` and a CRC-32 on every message (the UDP configuration of `ww_link`) over a channel
//! that loses, duplicates, delays (reorders) and corrupts frames, and starts new sessions on the sender. Whatever
//! the channel does, the receiver must not panic and every message it delivers must be a sent one (the CRC); unless
//! a frame was corrupted, each comes at most once and in the order they were sent (the prefix: a corrupted prefix
//! can move the receiver to a wrong position before the CRC rejects its frame). Over an unimpaired channel every
//! message must arrive.
//!
//! Sessions last at least `MIN_SESSION_FRAMES`, longer than the channel delays a frame: a frame delayed across two
//! session changes can't be told from one two sessions ahead with a 2-bit epoch (see `SeqRx`), and real sessions
//! (a link setup each) last far longer than any reordering.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use ww_framer::crc::{Crc32IsoHdlc, CrcChecksum};
use ww_framer::prefix::SeqPrefix;
use ww_framer::{FramedRx, Tx, framed::U2Head, traits::NopTail};

const MAX_FRAMES: usize = 256;
const MAX_SMALL_MSG: usize = 32;
const MAX_LARGE_MSG: usize = 600;
/// prefix + an End of 1 byte with an extended user kind head (2) and its CRC-32 (4); a Start with the longest head
/// (5) and 1 byte fits as well
const MIN_FRAME: usize = 8;
const MAX_FRAME: usize = 256;
const RX_BUF: usize = MAX_LARGE_MSG + MAX_FRAME;
const MAX_DELAY: usize = 4;
const MIN_SESSION_FRAMES: usize = 2 * MAX_DELAY;
type Checksum = CrcChecksum<Crc32IsoHdlc, true>;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let num_frames: usize = u.int_in_range(1..=MAX_FRAMES).unwrap_or(1);
    let frame_size: usize = u.int_in_range(MIN_FRAME..=MAX_FRAME).unwrap_or(MIN_FRAME);

    // ---------- Tx side ----------
    let mut tx_buf = vec![0u8; frame_size];
    let mut tx = Tx::<U2Head, Checksum, NopTail, SeqPrefix>::new(&mut tx_buf);
    let mut sent: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut new_sessions = false;
    let mut session_start = 0;
    // the last message was cut off when the frame budget ran out
    let mut cut = false;

    'outer: while frames.len() < num_frames {
        let user_kind: u8 = u.arbitrary().unwrap_or(0);
        let len: usize = if u.ratio(1, 8).unwrap_or(false) {
            u.int_in_range(0..=MAX_LARGE_MSG).unwrap_or(0)
        } else {
            u.int_in_range(0..=MAX_SMALL_MSG).unwrap_or(0)
        };
        let message: Vec<u8> = (0..len)
            .map(|i| (i as u8).wrapping_mul(31) ^ (sent.len() as u8))
            .collect();
        loop {
            match tx.write(user_kind, &message) {
                Ok(true) => {
                    sent.push((user_kind, message));
                    break;
                }
                Ok(false) => {
                    let n = tx.flush();
                    assert!(n > 0, "Ok(false) without any bytes written");
                    frames.push(tx.buf()[..n].to_vec());
                    // a new session may abandon the message half-sent
                    if frames.len() - session_start >= MIN_SESSION_FRAMES
                        && u.ratio(1, 16).unwrap_or(false)
                    {
                        session_start = frames.len();
                        tx.new_session();
                        new_sessions = true;
                        break;
                    }
                    if frames.len() >= num_frames {
                        cut = true;
                        break 'outer;
                    }
                }
                Err(()) => panic!("Tx refused message: frame_size={frame_size} len={len}"),
            }
        }
        if u.ratio(1, 4).unwrap_or(false) {
            let n = tx.flush();
            if n > 0 {
                frames.push(tx.buf()[..n].to_vec());
            }
        }
    }
    let n = tx.flush();
    if n > 0 {
        frames.push(tx.buf()[..n].to_vec());
    }

    // ---------- the channel ----------
    let mut delivered_frames: Vec<Vec<u8>> = Vec::new();
    let mut delayed: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut impaired = new_sessions;
    let mut corrupted = false;
    let debug = std::env::var_os("FUZZ_DEBUG").is_some();
    if debug {
        for (i, f) in frames.iter().enumerate() {
            eprintln!("frame {i}: {f:02x?}");
        }
        for (i, (k, m)) in sent.iter().enumerate() {
            eprintln!("sent {i}: kind {k} {m:02x?}");
        }
    }
    for frame in frames {
        if debug {
            eprintln!("channel <- {:02x?}", &frame[..frame.len().min(4)]);
        }
        match u.int_in_range(0u8..=15).unwrap_or(0) {
            0 => impaired = true, // lost
            1 => {
                delivered_frames.push(frame.clone());
                delivered_frames.push(frame);
                impaired = true;
            }
            2 => {
                let after = u.int_in_range(1..=MAX_DELAY).unwrap_or(1);
                delayed.push((after, frame));
                impaired = true;
            }
            3 => {
                let mut frame = frame;
                let bit = u.int_in_range(0..=frame.len() * 8 - 1).unwrap_or(0);
                frame[bit / 8] ^= 1 << (bit % 8);
                delivered_frames.push(frame);
                impaired = true;
                corrupted = true;
            }
            _ => delivered_frames.push(frame),
        }
        for (after, _) in delayed.iter_mut() {
            *after -= 1;
        }
        while let Some(i) = delayed.iter().position(|(after, _)| *after == 0) {
            delivered_frames.push(delayed.remove(i).1);
        }
    }
    delivered_frames.extend(delayed.into_iter().map(|(_, f)| f));

    // ---------- Rx side ----------
    let mut rx_buf = vec![0u8; RX_BUF];
    let mut rx = FramedRx::<U2Head, Checksum, NopTail, SeqPrefix>::new(&mut rx_buf);
    // messages must come out as a subsequence of the sent ones
    let mut next = 0usize;
    let mut received = 0usize;
    for frame in &delivered_frames {
        if rx.free() < frame.len() {
            // a corrupted Start claimed more than the largest message: drop it as ww_device's FramedRx does
            assert!(impaired, "overflow over an unimpaired channel");
            rx.reset();
        }
        let report = rx.stage(frame);
        if debug {
            eprintln!("stage {frame:02x?}: {report:?}");
        }
        if report.is_err() {
            panic!("frame of {} B does not fit, free {}", frame.len(), rx.free());
        }
        loop {
            rx.reassemble();
            let Some((user_kind, message)) = rx.message() else {
                break;
            };
            if debug {
                eprintln!("  -> kind {user_kind} {message:02x?}");
            }
            let is_it = |(k, m): &(u8, Vec<u8>)| *k == user_kind && m.as_slice() == message;
            if corrupted {
                assert!(
                    sent.iter().any(is_it),
                    "message #{received} (kind {user_kind}, {} B) was not sent",
                    message.len()
                );
            } else {
                let Some(at) = sent[next..].iter().position(is_it) else {
                    panic!(
                        "message #{received} (kind {user_kind}, {} B) was not sent, or out of order",
                        message.len()
                    );
                };
                next += at + 1;
            }
            received += 1;
        }
    }
    if !impaired {
        assert_eq!(received, sent.len(), "unimpaired channel lost messages");
        if !cut {
            assert_eq!(rx.free(), RX_BUF, "rx did not return to an idle state");
        }
    }
});
