//! Compressed counted streams (API-55): `Compressed` with the real `ww_lz4` encoder, its blocks decoded by
//! `ww_lz4::Decoder` as a host does.

use crate::codec::{Compressed, CountedState};
use crate::tx::Counted;
use crate::{StreamCursor, StreamHead};
use std::vec::Vec;
use wire_weaver::shrink_wrap::{Error, UVlq32, UVlq64};
use ww_client_server::stream::{StreamCodec, StreamCodecs};
use ww_lz4::{Decoder, Encoder};

type Enc = Encoder<1024, 4096>;

const LZ4: StreamCodecs = StreamCodecs { lz4: true };

fn line(i: usize) -> Vec<u8> {
    format!(
        "[{:5}.{:06}] usb 1-1: new full-speed USB device number {} using xhci_hcd\r\n",
        i / 7,
        i * 37 % 1_000_000,
        i % 9
    )
    .into_bytes()
}

/// One event as generated `_send` makes it: the head, then the data through `write_item`.
fn send(s: &mut impl CountedState, item: &[u8]) -> Option<(StreamHead, Vec<u8>)> {
    let head = s.head(item.len() as u64, None)?;
    let mut buf = [0u8; 1024];
    let n = s
        .write_item(&mut buf, |b: &mut [u8]| {
            b[..item.len()].copy_from_slice(item);
            Ok(item.len())
        })
        .unwrap();
    Some((head, buf[..n].to_vec()))
}

fn restarts(head: &StreamHead) -> bool {
    head.ext.is_some_and(|e| e.codec_restart)
}

#[test]
fn counted_is_a_counted_state_without_codec() {
    let mut c = Counted::new();
    let (opened, codec) = c.open_offered(3, Some(&LZ4));
    assert_eq!((opened.session, codec), (3, None));
    let (head, data) = send(&mut c, b"abc").unwrap();
    assert_eq!(data, b"abc");
    assert_eq!(head.ext, None);
}

#[test]
fn no_offer_sends_items_as_they_are() {
    let mut s = Compressed::new(Enc::new(1024));
    let (_, codec) = s.open_offered(1, None);
    assert_eq!(codec, None);
    assert!(!s.is_compressing());
    let (head, data) = send(&mut s, &line(0)).unwrap();
    assert_eq!(data, line(0));
    assert_eq!(head.ext, None);
    let (_, codec) = s.open_offered(2, Some(&StreamCodecs::default()));
    assert_eq!(codec, None, "an empty offer");
}

#[test]
fn round_trip_with_history() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(0);
    let (_, codec) = s.open_offered(1, Some(&LZ4));
    assert_eq!(
        codec,
        Some(StreamCodec::Lz4 {
            history: UVlq32(4095)
        })
    );
    let mut dec = Decoder::new();
    let (mut raw, mut sent) = (0, 0);
    for i in 0..400 {
        let item = line(i);
        let (head, data) = send(&mut s, &item).unwrap();
        assert_eq!(restarts(&head), i == 0, "only the first event restarts");
        if restarts(&head) {
            dec.reset();
        }
        assert_eq!(dec.decode(&data).unwrap(), item);
        raw += item.len();
        sent += data.len();
    }
    assert!(raw > 3 * sent, "{raw} bytes in {sent}");
}

#[test]
fn restarts_every_n_bytes() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(2000);
    s.open_offered(1, Some(&LZ4));
    let mut dec = Decoder::new();
    let mut since = 0;
    let mut marks = 0;
    for i in 0..200 {
        let item = line(i);
        let (head, data) = send(&mut s, &item).unwrap();
        if restarts(&head) {
            assert!(i == 0 || since >= 2000, "restart after {since} bytes");
            since = 0;
            marks += 1;
            dec.reset();
        }
        since += item.len();
        assert!(since < 2000 + 100);
        assert_eq!(dec.decode(&data).unwrap(), item);
    }
    assert!(marks > 5);
}

/// An event that went through the codec but wasn't sent: the next one starts the history over, and the units are
/// the next event's `lost`.
#[test]
fn unsent_event_restarts() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(0);
    s.open_offered(1, Some(&LZ4));
    let mut dec = Decoder::new();
    for i in 0..5 {
        let (head, data) = send(&mut s, &line(i)).unwrap();
        if restarts(&head) {
            dec.reset();
        }
        dec.decode(&data).unwrap();
    }
    let (head, _) = send(&mut s, &line(5)).unwrap();
    s.unsent(&head);
    let (head, data) = send(&mut s, &line(5)).unwrap();
    assert!(restarts(&head));
    assert_eq!(head.lost, Some(line(5).len() as u64));
    dec.reset();
    assert_eq!(dec.decode(&data).unwrap(), line(5));
    // only one marker: the unsent event's isn't carried over
    let (head, _) = send(&mut s, &line(6)).unwrap();
    assert!(head.ext.is_none());
}

/// Units the producer dropped never went through the codec: no restart.
#[test]
fn producer_loss_keeps_history() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(0);
    s.open_offered(1, Some(&LZ4));
    let mut dec = Decoder::new();
    for i in 0..10 {
        if i == 5 {
            s.counted().lost(100);
        }
        let (head, data) = send(&mut s, &line(i)).unwrap();
        assert_eq!(restarts(&head), i == 0);
        if restarts(&head) {
            dec.reset();
        }
        assert_eq!(dec.decode(&data).unwrap(), line(i));
    }
}

/// A host that loses an event on the link drops what it can't decode until the next restart, then goes on.
#[test]
fn link_loss_heals_at_the_next_restart() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(1500);
    s.open_offered(1, Some(&LZ4));
    let mut dec = Decoder::new();
    let mut expect = 0u64;
    let mut in_step = false;
    let (mut decoded, mut dropped) = (0, 0);
    for i in 0..300 {
        let item = line(i);
        let (head, data) = send(&mut s, &item).unwrap();
        if i % 37 == 20 {
            continue; // lost on the link
        }
        let gap = head.after_previous() != expect;
        expect = head.end();
        if restarts(&head) {
            dec.reset();
            in_step = true;
        } else if gap {
            in_step = false;
        }
        if in_step {
            assert_eq!(dec.decode(&data).unwrap(), item);
            decoded += 1;
        } else {
            dropped += 1;
        }
    }
    // 8 events lost on the link, each costs at most the events up to the next restart (1500 bytes, ~19 lines)
    assert_eq!(decoded + dropped, 300 - 8);
    assert!(
        dropped > 0 && dropped <= 8 * 20,
        "{decoded} decoded, {dropped} dropped"
    );
}

#[test]
fn resume_restarts_and_takes_the_new_offer() {
    let mut s = Compressed::new(Enc::new(1024));
    s.open_offered(1, Some(&LZ4));
    send(&mut s, &line(0)).unwrap();
    send(&mut s, &line(1)).unwrap();
    s.detach();
    let cursor = StreamCursor {
        session: UVlq32(1),
        pos: UVlq64(0),
    };
    let (_, codec) = s.resume_offered(cursor, None).unwrap();
    assert_eq!(codec, None, "an older host resumes");
    let (head, data) = send(&mut s, &line(2)).unwrap();
    assert_eq!(data, line(2));
    assert!(head.ext.is_none());
    let (_, codec) = s.resume_offered(cursor, Some(&LZ4)).unwrap();
    assert!(codec.is_some());
    let (head, data) = send(&mut s, &line(3)).unwrap();
    assert!(restarts(&head));
    assert_eq!(Decoder::new().decode(&data).unwrap(), line(3));
}

/// An item that fails to serialize: nothing sent, the next event restarts (the codec may hold part of it).
#[test]
fn item_error_restarts() {
    let mut s = Compressed::new(Enc::new(1024)).restart_every(0);
    s.open_offered(1, Some(&LZ4));
    send(&mut s, &line(0)).unwrap();
    let head = s.head(5, None).unwrap();
    let mut buf = [0u8; 256];
    let r = s.write_item(&mut buf, |_| Err(Error::OutOfBoundsWriteRawSlice));
    assert!(r.is_err());
    s.unsent(&head);
    let (head, data) = send(&mut s, &line(1)).unwrap();
    assert!(restarts(&head));
    assert_eq!(Decoder::new().decode(&data).unwrap(), line(1));
}

#[test]
fn init_in_place_is_new() {
    let mut a = std::boxed::Box::new(Compressed::new(Enc::new(1024)));
    let mut slot = std::boxed::Box::new(core::mem::MaybeUninit::<Compressed<Enc>>::uninit());
    let b = Compressed::init(&mut slot, |c| Enc::init(c, 1024));
    assert!(!b.is_compressing());
    a.open_offered(1, Some(&LZ4));
    b.open_offered(1, Some(&LZ4));
    for i in 0..200 {
        assert_eq!(send(&mut *a, &line(i)), send(b, &line(i)));
    }
}
