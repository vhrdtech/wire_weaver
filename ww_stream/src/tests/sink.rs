//! `sink::SinkCredit` (device) and `sink::SinkTx` (host) on their own and face to face, without a lossy link (that
//! is `sink_sim`).

use crate::sink::{
    MAX_WINDOW, POS_MODULUS, SinkConfig, SinkCredit, SinkCreditError, SinkDue, SinkOpenError,
    SinkSideband, SinkTx, SinkTxError, SinkVerdict, SinkWriteOut, extend_pos, wire_pos,
};
use crate::{Opened, StreamCursor};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;

const M: u64 = POS_MODULUS as u64;

fn cursor(session: u32, pos: u64) -> StreamCursor {
    StreamCursor {
        session: UVlq32(session),
        pos: UVlq64(pos),
    }
}

fn ack(have: u64, grant: u32) -> StreamSideband {
    StreamSideband::Ack {
        have: UVlq64(wire_pos(have) as u64),
        backfill: None,
        grant: UVlq32(grant),
    }
}

fn resend(from: u64, grant: u32) -> StreamSideband {
    StreamSideband::ResendFrom {
        from: UVlq64(wire_pos(from) as u64),
        grant: UVlq32(grant),
    }
}

fn opened_at(session: u32, have: u64) -> Opened {
    Opened {
        session,
        oldest: have,
        live_from: have,
        resume_from: None,
    }
}

/// A device in session 1 at 0, its first Ack taken.
fn device(capacity: u32) -> SinkCredit {
    let mut c = SinkCredit::new(capacity);
    c.open(cursor(1, 0));
    assert_eq!(c.take_ack(), Some(ack(0, capacity)));
    c
}

// ---- device ----

#[test]
fn initial_ack_and_threshold() {
    let mut c = SinkCredit::new(100).with_threshold(30);
    assert_eq!(c.take_ack(), None, "nothing before an open");
    let opened = c.open(cursor(7, 0));
    assert_eq!(opened, opened_at(7, 0));
    assert_eq!(c.take_ack(), Some(ack(0, 100)));
    assert_eq!(c.take_ack(), None);
    assert_eq!(c.on_write(0, 1, 40), SinkVerdict::Accept);
    assert_eq!(c.on_write(1, 1, 40), SinkVerdict::Accept);
    assert_eq!((c.have(), c.buffered(), c.last_item_len()), (2, 80, 40));
    c.free(20);
    assert_eq!(c.take_ack(), None, "limit moved by 20 < 30");
    c.free(20);
    assert_eq!(c.take_ack(), Some(ack(2, 60)));
    assert_eq!(c.limit(), 140);
    assert_eq!(c.overruns(), 0);
}

#[test]
fn units_and_bytes_are_counted_apart() {
    // a byte sink: units are the payload's bytes, one less than the item with its length prefix
    let mut c = device(100);
    assert_eq!(c.on_write(0, 30, 31), SinkVerdict::Accept);
    assert_eq!((c.have(), c.accepted(), c.last_item_units()), (30, 31, 30));
    assert_eq!(c.on_write(wire_pos(30), 1, 2), SinkVerdict::Accept);
    assert_eq!(
        c.on_write(wire_pos(31), 5, 0),
        SinkVerdict::Accept,
        "an empty item with units"
    );
    assert_eq!((c.have(), c.buffered()), (36, 33));
    c.free(33);
    assert_eq!(c.on_query(36), Some(ack(36, 100)));
    assert_eq!(
        c.on_write(wire_pos(36), 0, 1),
        SinkVerdict::Invalid,
        "no item has 0 units"
    );
}

#[test]
fn overrun_counted() {
    let mut c = device(10);
    assert_eq!(c.on_write(0, 1, 8), SinkVerdict::Accept);
    assert_eq!(c.on_write(1, 1, 8), SinkVerdict::Accept);
    assert_eq!(c.overruns(), 6);
}

#[test]
fn gap_asks_for_a_resend_once() {
    let mut c = device(100);
    assert_eq!(c.on_write(0, 10, 10), SinkVerdict::Accept);
    // 10..20 lost
    assert_eq!(
        c.on_write(20, 10, 10),
        SinkVerdict::Gap(Some(resend(10, 90)))
    );
    assert_eq!(c.on_write(30, 10, 10), SinkVerdict::Gap(None));
    assert_eq!(c.on_write(10, 10, 10), SinkVerdict::Accept);
    assert_eq!(c.on_write(20, 10, 10), SinkVerdict::Accept);
    assert_eq!(c.on_write(10, 10, 10), SinkVerdict::Duplicate);
    // next gap re-armed
    assert!(matches!(c.on_write(50, 10, 10), SinkVerdict::Gap(Some(_))));
}

#[test]
fn query() {
    let mut c = device(100);
    c.on_write(0, 10, 10);
    assert_eq!(c.on_query(10), Some(ack(10, 90)));
    // lost at the end, nothing after it revealed the gap
    assert_eq!(c.on_query(30), Some(resend(10, 90)));
    assert_eq!(c.on_write(20, 10, 10), SinkVerdict::Gap(None));
    c.free(10);
    assert_eq!(c.on_query(10), Some(ack(10, 100)));
    assert_eq!(c.on_query(M), None, "not a position on the wire");
}

#[test]
fn the_host_names_the_session() {
    let mut c = device(16);
    c.on_write(0, 4, 16);
    c.free(16);
    // the same session again (a reconnect, a restarted host, a late duplicate open): nothing reset
    assert_eq!(c.open(cursor(1, 0)), opened_at(1, 4));
    assert_eq!(c.open(cursor(1, 4)), opened_at(1, 4));
    assert_eq!(
        c.take_ack(),
        Some(ack(4, 16)),
        "an Ack is owed after every open"
    );
    assert_eq!(c.on_write(4, 4, 8), SinkVerdict::Accept);
    // another session starts at 0, whatever its cursor says
    assert_eq!(c.open(cursor(2, 100)), opened_at(2, 0));
    assert_eq!((c.session(), c.have(), c.buffered()), (Some(2), 0, 0));
    assert_eq!(c.take_ack(), Some(ack(0, 16)));
}

#[test]
fn no_session_answers_close_once() {
    // a device after a reboot: the host keeps writing into the session it had
    let mut c = SinkCredit::new(16);
    assert_eq!(
        c.on_write(40, 1, 1),
        SinkVerdict::NoSession(Some(StreamSideband::Close))
    );
    assert_eq!(c.on_write(41, 1, 1), SinkVerdict::NoSession(None));
    assert_eq!(c.on_query(42), None);
    assert_eq!(c.take_ack(), None);
    c.open(cursor(3, 42));
    assert_eq!(c.have(), 0);
    // closed by the host: late writes are dropped without a word
    c.close();
    assert_eq!(c.on_write(0, 1, 1), SinkVerdict::NoSession(None));
    assert_eq!(c.on_query(0), None);
    assert!(!c.status().open);
}

#[test]
fn status() {
    let mut c = device(64);
    c.on_write(0, 3, 10);
    let s = c.status();
    assert!(s.open);
    assert_eq!(s.session, Some(UVlq32(1)));
    assert_eq!(s.next, UVlq64(3));
    let store = s.store.unwrap();
    assert_eq!(
        (store.used_bytes.0, store.capacity_bytes.0, store.persistent),
        (10, 64, false)
    );
}

#[test]
fn sized_for_checks_largest_item() {
    assert_eq!(
        SinkCredit::try_sized_for(100, Some(101)).unwrap_err(),
        SinkCreditError::BufferTooSmall {
            buffer_bytes: 100,
            max_item_bytes: 101
        }
    );
    assert_eq!(
        SinkCredit::try_sized_for(100, None).unwrap().threshold(),
        50
    );
    // a re-grant must make room for one more largest item: 100 - 80 + 1
    let c = SinkCredit::sized_for(100, Some(80));
    assert_eq!(c.threshold(), 21);
    assert_eq!(c.with_threshold(90).threshold(), 21);
    assert_eq!(SinkCredit::sized_for(100, Some(100)).threshold(), 1);
}

#[test]
fn regrant_fits_largest_item() {
    let mut c = SinkCredit::sized_for(100, Some(80));
    c.open(cursor(1, 0));
    c.take_ack();
    assert_eq!(c.on_write(0, 1, 30), SinkVerdict::Accept);
    assert_eq!(c.on_write(1, 1, 50), SinkVerdict::Accept);
    c.free(80);
    // limit moved by 80 >= 21: the host learns it can send 100
    assert_eq!(c.take_ack(), Some(ack(2, 100)));
}

#[test]
fn extend_pos_takes_the_nearest() {
    let half = M / 2;
    for near in [
        0,
        1,
        1000,
        M - 1,
        M,
        M + 1,
        5 * M - 3,
        1 << 40,
        u64::MAX / 2,
    ] {
        for delta in [0i64, 1, -1, 100, -100, half as i64 - 1, -(half as i64)] {
            let Some(pos) = near.checked_add_signed(delta) else {
                continue;
            };
            assert_eq!(
                extend_pos(near, wire_pos(pos) as u64),
                Some(pos),
                "{near} {delta}"
            );
        }
    }
    // half the modulus ahead is read as half behind
    assert_eq!(extend_pos(M, wire_pos(M + M / 2) as u64), Some(M / 2));
    // before the start of the session
    assert_eq!(extend_pos(10, wire_pos(M - 5) as u64), None);
    // no host sends these
    assert_eq!(extend_pos(10, M), None);
    assert_eq!(extend_pos(10, u64::MAX), None);
    assert!(wire_pos(u64::MAX) < POS_MODULUS);
}

#[test]
fn positions_wrap_on_the_wire() {
    for start in [M - 50, 2 * M - 1, 16 * M - 7, (1 << 40) - 25] {
        let mut c = SinkCredit::new(100).with_threshold(30);
        assert_eq!(c.open_at(4, start), opened_at(4, start));
        assert_eq!(c.take_ack(), Some(ack(start, 100)));
        let mut pos = start;
        for _ in 0..10 {
            assert_eq!(c.on_write(wire_pos(pos), 40, 40), SinkVerdict::Accept);
            assert_eq!(c.on_write(wire_pos(pos), 40, 40), SinkVerdict::Duplicate);
            assert_eq!(
                c.on_write(wire_pos(pos + 80), 10, 10),
                SinkVerdict::Gap(Some(resend(pos + 40, 60)))
            );
            assert_eq!(
                c.on_query(wire_pos(pos + 80) as u64),
                Some(resend(pos + 40, 60))
            );
            assert_eq!(c.on_write(wire_pos(pos + 40), 40, 40), SinkVerdict::Accept);
            assert_eq!(
                c.on_query(wire_pos(pos + 80) as u64),
                Some(ack(pos + 80, 20))
            );
            c.free(80);
            assert_eq!(c.take_ack(), Some(ack(pos + 80, 100)));
            pos += 80;
        }
        assert_eq!(c.have(), start + 800);
        assert_eq!(c.overruns(), 0);
        assert_eq!(c.on_write(POS_MODULUS, 1, 1), SinkVerdict::Invalid);
    }
}

#[test]
fn late_write_from_a_previous_session() {
    let mut c = device(100);
    // the previous session ended just below the modulus
    assert_eq!(c.on_write(wire_pos(M - 20), 10, 10), SinkVerdict::Duplicate);
    assert_eq!(c.on_write(0, 10, 10), SinkVerdict::Accept);
}

#[test]
fn window_is_bounded() {
    assert_eq!(MAX_WINDOW as u64 * 4, M);
    assert_eq!(SinkCredit::new(MAX_WINDOW).capacity(), MAX_WINDOW);
    assert_eq!(
        SinkCredit::try_sized_for(MAX_WINDOW + 1, Some(10)).unwrap_err(),
        SinkCreditError::BufferTooLarge {
            buffer_bytes: MAX_WINDOW + 1
        }
    );
    let mut c = SinkCredit::new(100);
    assert!(c.set_capacity(MAX_WINDOW + 1).is_err());
    assert_eq!(c.capacity(), 100);
    assert_eq!(c.set_capacity(MAX_WINDOW), Ok(()));
    c.open_at(1, M - 3);
    assert_eq!(c.take_ack(), Some(ack(M - 3, MAX_WINDOW)));
    let end = M - 3 + MAX_WINDOW as u64 - 1;
    assert!(matches!(
        c.on_write(wire_pos(end), 1, 1),
        SinkVerdict::Gap(Some(_))
    ));
    assert_eq!(
        c.on_query(wire_pos(end) as u64),
        Some(resend(M - 3, MAX_WINDOW))
    );
}

#[test]
fn one_session_through_many_wraps() {
    // 2.5 Gi units in items of 64 Ki units and 1 KiB, every 7th write lost once and every 5th sent twice
    const UNITS: u64 = 1 << 16;
    const ITEM: u32 = 1 << 10;
    let mut c = SinkCredit::sized_for(1 << 20, Some(ITEM));
    c.open(cursor(1, 0));
    c.take_ack();
    let mut host = 0u64;
    let mut n = 0u64;
    while host < 10 * M {
        n += 1;
        if n.is_multiple_of(7) {
            let verdict = c.on_write(wire_pos(host + UNITS), UNITS, ITEM);
            assert_eq!(verdict, SinkVerdict::Gap(Some(resend(host, 1 << 20))));
        }
        assert_eq!(c.on_write(wire_pos(host), UNITS, ITEM), SinkVerdict::Accept);
        if n.is_multiple_of(5) {
            assert_eq!(
                c.on_write(wire_pos(host), UNITS, ITEM),
                SinkVerdict::Duplicate
            );
        }
        c.free(ITEM);
        host += UNITS;
    }
    assert_eq!(c.have(), host);
    assert_eq!(c.overruns(), 0);
}

#[test]
#[should_panic(expected = "MAX_WINDOW")]
fn new_refuses_a_too_large_window() {
    SinkCredit::new(MAX_WINDOW + 1);
}

// ---- host ----

fn pos_of(w: &SinkWriteOut<'_>) -> u32 {
    w.pos.0
}

fn config() -> SinkConfig {
    SinkConfig {
        query_interval_ms: 10,
        stall_timeout_ms: Some(35),
    }
}

/// A host in session 1, answered, with `window` granted.
fn host(window: u32) -> SinkTx {
    let mut h = SinkTx::new(config());
    h.open(1, 0);
    assert_eq!(h.opened(&opened_at(1, 0), 0), Ok(0));
    assert_eq!(h.on_sideband(&ack(0, window), 0), SinkSideband::Credit);
    h
}

fn send(h: &mut SinkTx, len: usize, units: u64) -> Option<u32> {
    h.try_new(&vec![0; len], units).unwrap().map(|w| pos_of(&w))
}

fn resent(h: &mut SinkTx) -> Vec<u32> {
    core::iter::from_fn(|| h.next_resend().map(|w| pos_of(&w))).collect()
}

#[test]
fn waits_for_the_answer_and_ignores_stale() {
    let mut h = SinkTx::new(config());
    assert_eq!(h.open(5, 0), cursor(5, 0));
    assert_eq!(send(&mut h, 1, 1), None);
    // from a previous session, before the answer
    assert_eq!(h.on_sideband(&ack(100, 10), 0), SinkSideband::Credit);
    assert_eq!(h.opened(&opened_at(4, 0), 0), Err(SinkOpenError::Stale));
    assert_eq!(
        h.on_sideband(&StreamSideband::FrameSync, 0),
        SinkSideband::Other
    );
    assert_eq!(h.opened(&opened_at(5, 0), 0), Ok(0));
    assert_eq!(send(&mut h, 1, 1), None, "no grant yet");
    h.on_sideband(&ack(0, 10), 0);
    assert_eq!(send(&mut h, 1, 1), Some(0));
}

#[test]
fn never_exceeds_credit() {
    let mut h = host(10);
    assert!(send(&mut h, 4, 1).is_some());
    assert!(send(&mut h, 4, 1).is_some());
    assert_eq!(send(&mut h, 4, 1), None);
    assert_eq!(
        h.try_new(&[0; 11], 1).unwrap_err(),
        SinkTxError::ItemTooBig {
            len: 11,
            buffer: 10
        }
    );
    h.on_sideband(&ack(1, 10), 0);
    assert_eq!(send(&mut h, 4, 1), Some(2));
    assert!(!h.all_acked());
    h.on_sideband(&ack(3, 10), 0);
    assert!(h.all_acked());
}

#[test]
fn units_on_the_wire() {
    let mut h = host(100);
    let w = h.try_new(&[1, 2, 3], 1).unwrap().unwrap();
    assert_eq!((w.units, w.pos.0, w.data), (None, 0, &[1u8, 2, 3][..]));
    let w = h.try_new(&[3, 1, 2, 3], 3).unwrap().unwrap();
    assert_eq!((w.units, w.pos.0), (Some(UVlq64(3)), 1));
    assert_eq!(h.sent(), 4);
    assert_eq!(
        h.try_new(&[1], 0).unwrap_err(),
        SinkTxError::BadUnits { units: 0 }
    );
    let too_many = MAX_WINDOW as u64 + 1;
    assert_eq!(
        h.try_new(&[1], too_many).unwrap_err(),
        SinkTxError::BadUnits { units: too_many }
    );
}

#[test]
fn never_more_than_max_window_units_in_flight() {
    let mut h = host(100);
    let big = MAX_WINDOW as u64 - 10;
    assert_eq!(send(&mut h, 1, big), Some(0));
    assert_eq!(
        send(&mut h, 1, 11),
        None,
        "units in flight would pass MAX_WINDOW"
    );
    assert_eq!(send(&mut h, 1, 10), Some(wire_pos(big)));
    h.on_sideband(&ack(big, 100), 0);
    assert_eq!(send(&mut h, 1, 11), Some(wire_pos(big + 10)));
}

#[test]
fn never_more_than_max_window_bytes_in_flight() {
    let mut h = host(10);
    h.on_sideband(&ack(0, u32::MAX), 0);
    assert_eq!(h.window(), MAX_WINDOW);
}

#[test]
fn go_back_n() {
    let mut h = host(100);
    for _ in 0..4 {
        send(&mut h, 10, 10).unwrap();
    }
    // 10..20 lost, the device saw 20..30
    h.on_sideband(&resend(10, 90), 0);
    assert_eq!(send(&mut h, 10, 10), None, "resends go first");
    assert_eq!(resent(&mut h), [10, 20, 30]);
    assert_eq!(send(&mut h, 10, 10), Some(40));
}

#[test]
fn go_back_n_mixed_sizes() {
    let mut h = host(100);
    // bytes and units differ: the grant is in bytes
    let sent: Vec<u32> = [(3usize, 1u64), (40, 7), (1, 1), (20, 2)]
        .iter()
        .map(|(len, units)| send(&mut h, *len, *units).unwrap())
        .collect();
    assert_eq!(sent, [0, 1, 8, 9]);
    // 64 bytes in flight, 36 left: a 37 byte item waits
    assert_eq!(send(&mut h, 37, 1), None);
    // the 40 byte item was lost: resend from its first unit, which acknowledges the first item
    h.on_sideband(&resend(1, 97), 0);
    assert_eq!(h.acked(), 1);
    assert_eq!(resent(&mut h), [1, 8, 9]);
    h.on_sideband(&ack(11, 100), 0);
    assert!(h.all_acked());
    assert_eq!(send(&mut h, 37, 1), Some(11));
}

#[test]
fn ack_inside_an_item_is_ignored() {
    let mut h = host(100);
    send(&mut h, 10, 10).unwrap();
    send(&mut h, 10, 10).unwrap();
    h.on_sideband(&ack(5, 100), 0);
    assert_eq!(h.acked(), 0);
    h.on_sideband(&ack(10, 100), 0);
    assert_eq!(h.acked(), 10);
}

#[test]
fn ack_does_not_rewind() {
    let mut h = host(100);
    send(&mut h, 10, 1).unwrap();
    send(&mut h, 10, 1).unwrap();
    h.on_sideband(&ack(1, 90), 0);
    assert_eq!(h.next_resend().map(|w| w.pos.0), None);
    h.on_sideband(&ack(0, 100), 0);
    assert_eq!(h.acked(), 1);
}

#[test]
fn query_and_stall() {
    let mut h = SinkTx::new(config());
    assert_eq!(h.due(0), None, "no session");
    h.open(1, 0);
    assert_eq!(h.due(0), None);
    assert_eq!(h.wait_ms(0), 10);
    assert_eq!(
        h.due(10),
        Some(SinkDue::Open(cursor(1, 0))),
        "the open again"
    );
    h.opened(&opened_at(1, 0), 15).unwrap();
    h.on_sideband(&ack(0, 10), 15);
    send(&mut h, 10, 1).unwrap();
    assert_eq!(
        h.due(25),
        Some(SinkDue::Query(StreamSideband::Query {
            sent: UVlq64(1),
            backfill: None
        }))
    );
    assert_eq!(h.due(50), Some(SinkDue::Stalled));
}

#[test]
fn positions_wrap_on_the_wire_host() {
    for start in [M - 25, 3 * M - 1, (1 << 40) - 7] {
        let mut h = SinkTx::new(config());
        h.open_at(2, start, 0);
        assert_eq!(h.opened(&opened_at(2, start), 0), Ok(start));
        h.on_sideband(&ack(start, 100), 0);
        let sent: Vec<u32> = (0..5).map(|_| send(&mut h, 10, 10).unwrap()).collect();
        assert_eq!(sent, [0, 10, 20, 30, 40].map(|d| wire_pos(start + d)));
        h.on_sideband(&resend(start + 30, 70), 0);
        assert_eq!(resent(&mut h), [wire_pos(start + 30), wire_pos(start + 40)]);
        h.on_sideband(&ack(start + 50, 100), 0);
        assert!(h.all_acked());
        assert_eq!((h.acked(), h.sent()), (start + 50, start + 50));
    }
}

// ---- sessions and resume ----

#[test]
fn reconnect_resends_what_the_device_lacks() {
    let mut h = host(100);
    for _ in 0..4 {
        send(&mut h, 10, 10).unwrap();
    }
    h.on_sideband(&ack(10, 90), 0);
    // the link goes down with 10..40 in flight
    h.link_lost();
    assert_eq!(
        h.on_sideband(&ack(40, 100), 0),
        SinkSideband::Credit,
        "stale until the answer"
    );
    assert_eq!(h.acked(), 10);
    assert_eq!(h.resume(5), Some(cursor(1, 10)));
    // the device got 10..20 before the link went down
    assert_eq!(h.opened(&opened_at(1, 20), 5), Ok(20));
    assert_eq!(h.acked(), 20);
    assert_eq!(resent(&mut h), [], "no grant yet");
    h.on_sideband(&ack(20, 100), 5);
    assert_eq!(resent(&mut h), [20, 30]);
    assert_eq!(send(&mut h, 10, 10), Some(40));
}

#[test]
fn device_closed_then_session_lost() {
    let mut h = host(100);
    send(&mut h, 10, 10).unwrap();
    h.on_sideband(&ack(10, 100), 0);
    send(&mut h, 10, 10).unwrap();
    // the device rebooted: a Close for the write it has no session for
    assert_eq!(
        h.on_sideband(&StreamSideband::Close, 1),
        SinkSideband::DeviceClosed
    );
    assert_eq!(send(&mut h, 1, 1), None);
    assert_eq!(h.resume(1), Some(cursor(1, 10)));
    // it starts the host's session at 0
    assert_eq!(
        h.opened(&opened_at(1, 0), 2),
        Err(SinkOpenError::SessionLost {
            device_at: 0,
            acked: 10
        })
    );
    assert_eq!(h.session(), None);
    assert_eq!(h.due(100), None);
}

#[test]
fn restarted_host_continues_where_the_device_is() {
    // the old host saved its cursor at 30; the device got up to 50 before the old host went away
    let mut h = SinkTx::new(config());
    assert_eq!(h.resume_from(cursor(9, 30), 0), cursor(9, 30));
    assert_eq!(h.opened(&opened_at(9, 50), 0), Ok(50));
    assert_eq!((h.acked(), h.sent()), (50, 50));
    h.on_sideband(&ack(50, 100), 0);
    assert_eq!(send(&mut h, 5, 5), Some(50));
    // a device at the cursor itself
    let mut h = SinkTx::new(config());
    h.resume_from(cursor(9, 30), 0);
    assert_eq!(h.opened(&opened_at(9, 30), 0), Ok(30));
}

#[test]
fn device_ahead_with_items_in_flight_is_refused() {
    let mut h = host(100);
    send(&mut h, 10, 10).unwrap();
    h.link_lost();
    h.resume(0);
    assert_eq!(
        h.opened(&opened_at(1, 50), 0),
        Err(SinkOpenError::Ahead {
            device_at: 50,
            sent: 10
        })
    );
}

#[test]
fn closed_by_the_host() {
    let mut h = host(100);
    h.closed();
    assert_eq!(h.cursor(), None);
    assert_eq!(h.resume(0), None);
    assert_eq!(
        h.on_sideband(&StreamSideband::Close, 0),
        SinkSideband::Credit
    );
}

// ---- face to face ----

/// Host and device over a link that loses nothing, `units` per item.
fn transfer(items: &[(Vec<u8>, u64)], capacity: u32) -> Vec<Vec<u8>> {
    let mut d = SinkCredit::new(capacity);
    let mut h = SinkTx::new(config());
    let mut got = Vec::new();
    let c = h.open(42, 0);
    let opened = d.open(c);
    h.opened(&opened, 0).unwrap();
    let mut to_host: Vec<StreamSideband> = d.take_ack().into_iter().collect();
    let mut next = 0;
    let mut now = 0;
    while next < items.len() || !h.all_acked() {
        now += 1;
        assert!(now < 10_000, "stuck");
        for sb in to_host.drain(..) {
            h.on_sideband(&sb, now);
        }
        while let Some((data, units)) = items.get(next) {
            let Some(w) = h.try_new(data, *units).unwrap() else {
                break;
            };
            let units = w.units.map_or(1, |u| u.0);
            assert_eq!(
                d.on_write(w.pos.0, units, w.data.len() as u32),
                SinkVerdict::Accept
            );
            got.push(w.data.to_vec());
            next += 1;
        }
        d.free(d.buffered());
        to_host.extend(d.take_ack());
        if let Some(SinkDue::Query(q)) = h.due(now)
            && let StreamSideband::Query { sent, .. } = q
        {
            to_host.extend(d.on_query(sent.0));
        }
    }
    got
}

#[test]
fn face_to_face() {
    let items: Vec<(Vec<u8>, u64)> = (0..200u32)
        .map(|i| (vec![i as u8; (i % 37) as usize], 1 + (i % 5) as u64))
        .collect();
    let got = transfer(&items, 64);
    assert_eq!(
        got,
        items.iter().map(|(d, _)| d.clone()).collect::<Vec<_>>()
    );
}
