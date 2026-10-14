use super::block_on;
use crate::sched::{Priority, StreamReady};
use crate::store::{FullPolicy, RECORD_OVERHEAD, RamRing, StreamStore};
use crate::tx::{Pushed, Reliable};
use crate::{HeadExt, Opened, ResumeError, Run, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;

type Dev<const N: usize> = Reliable<RamRing<N>, 4>;

fn push<const N: usize>(dev: &mut Dev<N>, units: u64, byte: u8) -> Pushed {
    block_on(dev.push(&[byte; 8], units, None)).unwrap()
}

fn send<const N: usize>(dev: &mut Dev<N>, run: Run) -> Option<StreamHead> {
    let mut buf = [0u8; 64];
    block_on(dev.next_event(run, &mut buf)).unwrap().map(|e| {
        assert_eq!(e.run, run);
        e.head
    })
}

fn sb<const N: usize>(dev: &mut Dev<N>, sideband: StreamSideband) {
    block_on(dev.on_sideband(&sideband, 0)).unwrap()
}

fn ack(have: u64, backfill: Option<u64>, grant: u32) -> StreamSideband {
    StreamSideband::Ack {
        have: UVlq64(have),
        backfill: backfill.map(UVlq64),
        grant: UVlq32(grant),
    }
}

fn resend(from: u64, grant: u32) -> StreamSideband {
    StreamSideband::ResendFrom {
        from: UVlq64(from),
        grant: UVlq32(grant),
    }
}

fn cursor(session: u32, pos: u64) -> StreamCursor {
    StreamCursor {
        session: UVlq32(session),
        pos: UVlq64(pos),
    }
}

/// Bytes of a record with an 8-byte item and a small position.
const REC: usize = RECORD_OVERHEAD + 3 + 8;

#[test]
fn send_ack_release() {
    let mut dev = Dev::<1024>::new(RamRing::new()).initial_grant(100);
    assert_eq!(push(&mut dev, 2, 0), Pushed::Lost, "no session yet");
    assert_eq!(
        block_on(dev.open(3)).unwrap(),
        Opened {
            session: 3,
            oldest: 0,
            live_from: 0,
            resume_from: None
        }
    );
    for i in 0..4 {
        assert_eq!(push(&mut dev, 2, i), Pushed::Stored);
    }
    assert!(dev.ready().live);
    assert_eq!(send(&mut dev, Run::Live).map(|h| h.pos), Some(0));
    assert_eq!(send(&mut dev, Run::Live).map(|h| h.pos), Some(2));
    assert_eq!(dev.in_flight(), 16);
    sb(&mut dev, ack(2, None, 100));
    assert_eq!(dev.in_flight(), 8);
    assert_eq!(dev.store().state().oldest, Some(2));
    assert_eq!(dev.stats().acked_bytes, 8);
    assert_eq!(send(&mut dev, Run::Live).map(|h| h.pos), Some(4));
    assert_eq!(send(&mut dev, Run::Live).map(|h| h.pos), Some(6));
    assert_eq!(send(&mut dev, Run::Live), None);
    assert!(!dev.ready().live);
    sb(&mut dev, ack(8, None, 100));
    assert!(dev.store().state().oldest.is_none());
    assert_eq!(dev.stats().sent, 8);
    assert_eq!(dev.acked_cursor(), Some(cursor(3, 8)));
}

#[test]
fn grant_limits_bytes_in_flight() {
    let mut dev = Dev::<1024>::new(RamRing::new()).initial_grant(10);
    block_on(dev.open(1)).unwrap();
    for i in 0..5 {
        push(&mut dev, 1, i);
    }
    assert!(send(&mut dev, Run::Live).is_some());
    // 8 of 10 in flight: one more may go, then no credit
    assert!(send(&mut dev, Run::Live).is_some());
    assert!(!dev.ready().live);
    assert_eq!(send(&mut dev, Run::Live), None);
    sb(&mut dev, ack(1, None, 10));
    assert!(dev.ready().live);
    assert!(send(&mut dev, Run::Live).is_some());
    // a bigger grant
    sb(&mut dev, ack(1, None, 1000));
    assert!(send(&mut dev, Run::Live).is_some());
    assert!(send(&mut dev, Run::Live).is_some());
}

#[test]
fn go_back_n() {
    let mut dev = Dev::<1024>::new(RamRing::new()).initial_grant(1000);
    block_on(dev.open(1)).unwrap();
    for i in 0..6 {
        push(&mut dev, 3, i);
    }
    for _ in 0..6 {
        send(&mut dev, Run::Live).unwrap();
    }
    // 6..9 got lost: everything from there again, and 0..6 counts as acknowledged
    sb(&mut dev, resend(6, 1000));
    assert_eq!(dev.store().state().oldest, Some(6));
    let again: Vec<_> = core::iter::from_fn(|| send(&mut dev, Run::Live))
        .map(|h| (h.pos, h.lost))
        .collect();
    assert_eq!(again, [(6, None), (9, None), (12, None), (15, None)]);
    assert_eq!(dev.stats().resent, 12);
    assert_eq!(dev.stats().sent, 18);
    assert_eq!(dev.stats().resend_requests, 1);
    // a resend for what is acknowledged or not sent is ignored
    sb(&mut dev, ack(12, None, 1000));
    sb(&mut dev, resend(3, 1000));
    sb(&mut dev, resend(30, 1000));
    assert_eq!(dev.stats().overruns, 2);
    assert_eq!(send(&mut dev, Run::Live), None);
}

#[test]
fn merged_flight_records() {
    // only 4 in-flight records: 8 events merge pairwise, a resend inside a merged pair goes back to its start
    let mut dev = Dev::<4096>::new(RamRing::new()).initial_grant(1000);
    block_on(dev.open(1)).unwrap();
    for i in 0..8 {
        push(&mut dev, 1, i);
    }
    for _ in 0..8 {
        send(&mut dev, Run::Live).unwrap();
    }
    assert_eq!(dev.in_flight(), 64);
    sb(&mut dev, resend(5, 1000));
    let first = send(&mut dev, Run::Live).unwrap();
    assert!(first.pos <= 5);
    // an ack inside a merged record frees it only when the whole record is acknowledged
    sb(&mut dev, ack(5, None, 1000));
    assert!(dev.in_flight() >= 8);
}

#[test]
fn overrun_ack_is_ignored() {
    let mut dev = Dev::<1024>::new(RamRing::new());
    block_on(dev.open(1)).unwrap();
    push(&mut dev, 1, 0);
    send(&mut dev, Run::Live).unwrap();
    // an Ack of an old session with a high position
    sb(&mut dev, ack(1000, None, 1));
    assert_eq!(dev.stats().overruns, 1);
    assert_eq!(dev.in_flight(), 8);
    assert_eq!(dev.store().state().oldest, Some(0));
}

#[test]
fn query_when_silent() {
    let mut dev = Dev::<1024>::new(RamRing::new()).query_interval_ms(100);
    block_on(dev.open(1)).unwrap();
    assert_eq!(dev.poll_query(0), None, "nothing in flight");
    push(&mut dev, 4, 0);
    send(&mut dev, Run::Live).unwrap();
    assert_eq!(dev.poll_query(1000), None, "starts the clock");
    assert_eq!(dev.poll_query(1050), None);
    let query = Some(StreamSideband::Query {
        sent: UVlq64(4),
        backfill: None,
    });
    assert_eq!(dev.poll_query(1100), query);
    assert_eq!(dev.poll_query(1150), None);
    assert_eq!(dev.poll_query(1200), query);
    block_on(dev.on_sideband(&ack(4, None, 100), 1210)).unwrap();
    assert_eq!(dev.poll_query(5000), None);
}

#[test]
fn store_full_drop_newest() {
    let mut dev = Dev::<{ 3 * REC }>::new(RamRing::new());
    block_on(dev.open(1)).unwrap();
    // no host reads: the store fills, then new events are lost and counted
    for i in 0..5 {
        let expect = if i < 3 { Pushed::Stored } else { Pushed::Lost };
        assert_eq!(push(&mut dev, 2, i), expect);
    }
    assert_eq!(dev.stats().lost_store_full, 4);
    let sent: Vec<_> = core::iter::from_fn(|| send(&mut dev, Run::Live))
        .map(|h| h.pos)
        .collect();
    assert_eq!(sent, [0, 2, 4]);
    sb(&mut dev, ack(6, None, 100));
    push(&mut dev, 2, 9);
    let h = send(&mut dev, Run::Live).unwrap();
    // what the store couldn't hold is the next event's `lost`
    assert_eq!((h.pos, h.lost), (10, Some(4)));
}

#[test]
fn store_full_drop_oldest() {
    let mut dev = Dev::<{ 3 * REC }>::new(RamRing::new().with_policy(FullPolicy::DropOldest));
    block_on(dev.open(1)).unwrap();
    for i in 0..5 {
        assert_eq!(push(&mut dev, 2, i), Pushed::Stored);
    }
    assert_eq!(dev.stats().lost_store_full, 4);
    // the oldest two are gone: the first event sent says so
    let h = send(&mut dev, Run::Live).unwrap();
    assert_eq!((h.pos, h.lost), (4, Some(4)));
}

#[test]
fn producer_lost_and_ext() {
    let mut dev = Dev::<1024>::new(RamRing::new());
    block_on(dev.open(1)).unwrap();
    push(&mut dev, 1, 0);
    dev.lost(10);
    let ext = HeadExt {
        clock_epoch: Some(UVlq32(1)),
    };
    dev.set_ext(ext);
    push(&mut dev, 1, 1);
    send(&mut dev, Run::Live);
    let h = send(&mut dev, Run::Live).unwrap();
    assert_eq!((h.pos, h.lost, h.ext), (11, Some(10), Some(ext)));
    assert_eq!(dev.stats().lost_producer, 10);
}

/// A logger: the device starts the session, the store fills without a host.
fn logger(events: u64) -> Dev<4096> {
    let mut dev = Dev::<4096>::new(RamRing::new()).initial_grant(10_000);
    block_on(dev.start_session(7)).unwrap();
    for i in 0..events {
        push(&mut dev, 10, i as u8);
    }
    assert!(!dev.ready().live);
    dev
}

#[test]
fn resume_inside_store() {
    let mut dev = logger(10);
    let opened = block_on(dev.resume(cursor(7, 30))).unwrap();
    // live from the newest event, backlog from the cursor
    assert_eq!(
        opened,
        Opened {
            session: 7,
            oldest: 30,
            live_from: 90,
            resume_from: Some(30)
        }
    );
    assert_eq!(send(&mut dev, Run::Live).unwrap().pos, 90);
    let backlog: Vec<_> = core::iter::from_fn(|| send(&mut dev, Run::Backfill))
        .map(|h| h.pos)
        .collect();
    assert_eq!(backlog, [30, 40, 50, 60, 70, 80]);
    assert!(dev.is_backfilling());
    // live acknowledged, backlog not: nothing released
    sb(&mut dev, ack(100, Some(30), 10_000));
    assert_eq!(dev.store().state().oldest, Some(30));
    sb(&mut dev, ack(100, Some(60), 10_000));
    assert_eq!(dev.store().state().oldest, Some(60));
    sb(&mut dev, ack(100, Some(90), 10_000));
    assert!(!dev.is_backfilling());
    assert!(dev.store().state().oldest.is_none());
}

#[test]
fn backfill_done_by_ack_without_backfill() {
    let mut dev = logger(3);
    block_on(dev.resume(cursor(7, 0))).unwrap();
    while send(&mut dev, Run::Backfill).is_some() {}
    send(&mut dev, Run::Live).unwrap();
    // the host acknowledged the backlog in an Ack that got lost; later ones carry no backfill
    sb(&mut dev, ack(30, None, 10_000));
    assert!(!dev.is_backfilling());
    assert!(dev.store().state().oldest.is_none());
}

#[test]
fn resume_before_store() {
    let mut dev = Dev::<{ 4 * REC }>::new(RamRing::new().with_policy(FullPolicy::DropOldest));
    block_on(dev.start_session(7)).unwrap();
    for i in 0..10 {
        push(&mut dev, 10, i);
    }
    let opened = block_on(dev.resume(cursor(7, 20))).unwrap();
    assert_eq!(
        opened,
        Opened {
            session: 7,
            oldest: 60,
            live_from: 90,
            resume_from: Some(60)
        }
    );
}

#[test]
fn resume_after_store() {
    // the host has everything: no backlog, live from the next event
    let mut dev = logger(3);
    let opened = block_on(dev.resume(cursor(7, 30))).unwrap();
    assert_eq!(
        opened,
        Opened {
            session: 7,
            oldest: 30,
            live_from: 30,
            resume_from: None
        }
    );
    assert!(
        dev.store().state().oldest.is_none(),
        "released up to the cursor"
    );
    assert!(!dev.ready().live);
    push(&mut dev, 1, 0);
    assert_eq!(send(&mut dev, Run::Live).unwrap().pos, 30);

    // the host has the newest but one: the newest is live, no backlog
    let mut dev = logger(3);
    let opened = block_on(dev.resume(cursor(7, 20))).unwrap();
    assert_eq!((opened.live_from, opened.resume_from), (20, None));

    // a cursor beyond what the device has (another device's?): clamped
    let mut dev = logger(3);
    let opened = block_on(dev.resume(cursor(7, 1000))).unwrap();
    assert_eq!((opened.live_from, opened.resume_from), (30, None));

    // units lost after the newest event: live from the next one, they are the first event's `lost`
    let mut dev = logger(3);
    dev.lost(5);
    let opened = block_on(dev.resume(cursor(7, 30))).unwrap();
    assert_eq!((opened.live_from, opened.resume_from), (35, None));
    push(&mut dev, 1, 0);
    let h = send(&mut dev, Run::Live).unwrap();
    assert_eq!((h.pos, h.lost), (35, None));
}

#[test]
fn resume_other_session_and_none() {
    let mut dev = Dev::<1024>::new(RamRing::new());
    assert_eq!(
        block_on(dev.resume(cursor(1, 0))),
        Err(ResumeError::NoSession)
    );
    let mut dev = logger(3);
    let opened = block_on(dev.resume(cursor(6, 25))).unwrap();
    // the whole current session
    assert_eq!(
        opened,
        Opened {
            session: 7,
            oldest: 0,
            live_from: 20,
            resume_from: Some(0)
        }
    );
}

#[test]
fn detach_keeps_unacknowledged() {
    let mut dev = Dev::<1024>::new(RamRing::new());
    block_on(dev.open(1)).unwrap();
    for i in 0..3 {
        push(&mut dev, 1, i);
    }
    send(&mut dev, Run::Live);
    send(&mut dev, Run::Live);
    sb(&mut dev, ack(1, None, 100));
    dev.detach();
    assert!(!dev.ready().live);
    assert_eq!(send(&mut dev, Run::Live), None);
    sb(&mut dev, ack(3, None, 100));
    assert_eq!(
        dev.store().state().oldest,
        Some(1),
        "nothing acknowledged while detached"
    );
    let opened = block_on(dev.resume(cursor(1, 1))).unwrap();
    assert_eq!((opened.live_from, opened.resume_from), (2, Some(1)));
}

#[test]
fn backlog_dropped_while_backfilling() {
    let mut dev = Dev::<{ 4 * REC }>::new(RamRing::new().with_policy(FullPolicy::DropOldest));
    block_on(dev.start_session(1)).unwrap();
    for i in 0..4 {
        push(&mut dev, 1, i);
    }
    block_on(dev.resume(cursor(1, 0))).unwrap();
    assert_eq!(send(&mut dev, Run::Backfill).unwrap().pos, 0);
    // new data pushes the rest of the backlog out
    for i in 0..3 {
        push(&mut dev, 1, 10 + i);
    }
    assert_eq!(send(&mut dev, Run::Backfill), None);
    assert!(dev.is_backfilling(), "0 not acknowledged yet");
    assert_eq!(dev.poll_query(0), None);
    sb(&mut dev, ack(3, Some(1), 100));
    assert!(!dev.is_backfilling());
    assert_eq!(
        dev.poll_query(1),
        Some(StreamSideband::Query {
            sent: UVlq64(3),
            backfill: None
        })
    );
}

#[test]
fn positions_past_u32_and_2_pow_50() {
    let mut dev = Dev::<1024>::new(RamRing::new());
    block_on(dev.open(1)).unwrap();
    dev.lost((1 << 32) - 1);
    push(&mut dev, 3, 0);
    dev.lost((1 << 50) - (1 << 32));
    push(&mut dev, 1, 1);
    let a = send(&mut dev, Run::Live).unwrap();
    let b = send(&mut dev, Run::Live).unwrap();
    assert_eq!((a.pos, a.lost), ((1 << 32) - 1, Some((1 << 32) - 1)));
    assert_eq!(b.pos, (1 << 50) + 2);
    assert_eq!(b.after_previous(), a.end());
    sb(&mut dev, ack(b.end(), None, 100));
    assert!(dev.store().state().oldest.is_none());
}

#[test]
fn ready_for_scheduler() {
    let mut dev = logger(3).priority(Priority::High);
    assert_eq!(
        dev.ready(),
        StreamReady {
            priority: Priority::High,
            live: false,
            backfill: false
        }
    );
    block_on(dev.resume(cursor(7, 0))).unwrap();
    assert_eq!(
        dev.ready(),
        StreamReady {
            priority: Priority::High,
            live: true,
            backfill: true
        }
    );
    send(&mut dev, Run::Live);
    assert!(!dev.ready().live);
    assert!(dev.ready().backfill);
}

#[test]
fn continue_after_reboot() {
    let mut dev = logger(3);
    let store = core::mem::take(dev.store_mut());
    // a "persistent" store comes back after a reboot
    let mut dev = Dev::<4096>::new(store);
    assert_eq!(dev.session(), None);
    dev.continue_stored_session();
    assert_eq!((dev.session(), dev.next_pos()), (Some(7), 30));
    let opened = block_on(dev.resume(cursor(7, 10))).unwrap();
    assert_eq!((opened.live_from, opened.resume_from), (20, Some(10)));
    // an empty store holds no session
    let mut dev = Dev::<64>::new(RamRing::new());
    dev.continue_stored_session();
    assert_eq!(dev.session(), None);
    let _ = block_on(dev.store_mut().reset(0));
}

#[test]
fn units_lost_before_the_newest_stay_in_the_live_run() {
    let mut dev = logger(2);
    dev.lost(5);
    push(&mut dev, 1, 9);
    let opened = block_on(dev.resume(cursor(7, 0))).unwrap();
    // backlog 0..20, live from 20 where the newest event's `lost` starts
    assert_eq!((opened.live_from, opened.resume_from), (20, Some(0)));
    let h = send(&mut dev, Run::Live).unwrap();
    assert_eq!((h.pos, h.lost), (25, Some(5)));
    let backlog: Vec<_> = core::iter::from_fn(|| send(&mut dev, Run::Backfill))
        .map(|h| h.pos)
        .collect();
    assert_eq!(backlog, [0, 10]);
}
