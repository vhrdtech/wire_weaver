use crate::rx::{AckMode, Cause, Delivery, Gap, Receiver, Verdict};
use crate::{Opened, Run, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};
use ww_client_server::StreamSideband;

fn head(pos: u64, units: u64, lost: u64) -> StreamHead {
    StreamHead {
        units,
        lost: (lost > 0).then_some(lost),
        ..StreamHead::at(pos)
    }
}

fn fresh(session: u32) -> Opened {
    Opened {
        session,
        oldest: 0,
        live_from: 0,
        resume_from: None,
    }
}

fn gaps(v: Verdict) -> Vec<Gap> {
    match v {
        Verdict::Accept { gaps, .. } => gaps.iter().copied().collect(),
        other => panic!("{other:?}"),
    }
}

fn gap(run: Run, at: u64, units: u64, cause: Cause) -> Gap {
    Gap {
        run,
        at,
        units,
        cause,
    }
}

#[test]
fn counted_in_order_device_and_link_gaps() {
    let mut rx = Receiver::new(Delivery::Counted);
    assert_eq!(rx.on_item(&head(0, 1, 0)), Verdict::NotOpen);
    rx.opened(None, &fresh(1));
    assert!(gaps(rx.on_item(&head(0, 10, 0))).is_empty());
    // the device dropped 5
    assert_eq!(
        gaps(rx.on_item(&head(15, 10, 5))),
        [gap(Run::Live, 10, 5, Cause::Device)]
    );
    // an event of 10 units at 25 was lost on the link, then the device dropped 3 more
    assert_eq!(
        gaps(rx.on_item(&head(38, 2, 3))),
        [
            gap(Run::Live, 25, 10, Cause::Link),
            gap(Run::Live, 35, 3, Cause::Device)
        ]
    );
    // the lost one arrives late, then a duplicate
    assert_eq!(rx.on_item(&head(25, 10, 0)), Verdict::Late);
    assert_eq!(rx.on_item(&head(15, 10, 5)), Verdict::Duplicate);
    let st = rx.stats();
    assert_eq!(
        (
            st.received,
            st.lost_device,
            st.lost_link,
            st.late,
            st.duplicates
        ),
        (22, 8, 10, 1, 1)
    );
    assert_eq!(rx.cursor().unwrap().pos, UVlq64(40));
    // counted streams never send anything
    assert_eq!(rx.poll(0), None);
}

#[test]
fn reliable_gap_asks_once_and_again_after_timeout() {
    let mut rx = Receiver::new(Delivery::Reliable).window(1000);
    rx.opened(None, &fresh(1));
    // first an Ack with the grant
    assert_eq!(
        rx.poll(0),
        Some(StreamSideband::Ack {
            have: UVlq64(0),
            backfill: None,
            grant: UVlq32(1000)
        })
    );
    assert_eq!(rx.poll(1), None);
    assert!(gaps(rx.on_item(&head(0, 4, 0))).is_empty());
    // 4..8 lost: dropped, one resend for both events after it
    assert_eq!(rx.on_item(&head(8, 4, 0)), Verdict::OutOfOrder);
    assert_eq!(rx.on_item(&head(12, 4, 0)), Verdict::OutOfOrder);
    let resend = Some(StreamSideband::Resend {
        from: UVlq64(4),
        grant: UVlq32(1000),
    });
    assert_eq!(rx.poll(10), resend);
    assert_eq!(rx.poll(20), None);
    assert_eq!(rx.on_item(&head(16, 4, 0)), Verdict::OutOfOrder);
    assert_eq!(rx.poll(30), None);
    // nothing came: asked again
    assert_eq!(rx.poll(600), resend);
    // the resend arrives, in order
    assert!(gaps(rx.on_item(&head(4, 4, 0))).is_empty());
    assert!(gaps(rx.on_item(&head(8, 4, 0))).is_empty());
    assert_eq!(rx.poll(2000), None, "nothing delivered, nothing to ack");
    rx.delivered(&head(0, 4, 0), 10);
    rx.delivered(&head(4, 4, 0), 10);
    assert_eq!(
        rx.poll(2001),
        Some(StreamSideband::Ack {
            have: UVlq64(8),
            backfill: None,
            grant: UVlq32(1000)
        })
    );
    // within the ack interval: wait
    rx.delivered(&head(8, 4, 0), 10);
    assert_eq!(rx.poll(2050), None);
    assert!(matches!(
        rx.poll(2101),
        Some(StreamSideband::Ack {
            have: UVlq64(12),
            ..
        })
    ));
    assert_eq!(rx.stats().resend_requests, 2);
    assert_eq!(rx.stats().out_of_order, 3);
}

#[test]
fn half_window_acks_at_once() {
    let mut rx = Receiver::new(Delivery::Reliable).window(100);
    rx.opened(None, &fresh(1));
    rx.poll(0);
    rx.on_item(&head(0, 1, 0));
    rx.delivered(&head(0, 1, 0), 30);
    assert_eq!(rx.poll(1), None);
    rx.on_item(&head(1, 1, 0));
    rx.delivered(&head(1, 1, 0), 30);
    assert!(matches!(
        rx.poll(2),
        Some(StreamSideband::Ack {
            have: UVlq64(2),
            ..
        })
    ));
}

#[test]
fn explicit_acks() {
    let mut rx = Receiver::new(Delivery::Reliable).ack_mode(AckMode::Explicit);
    rx.opened(None, &fresh(1));
    rx.poll(0);
    rx.on_item(&head(0, 5, 0));
    rx.on_item(&head(5, 5, 0));
    rx.delivered(&head(0, 5, 0), 5);
    rx.delivered(&head(5, 5, 0), 5);
    assert_eq!(rx.poll(1000), None, "received, not confirmed");
    assert_eq!(rx.cursor().unwrap().pos, UVlq64(0));
    rx.ack(5);
    assert!(matches!(
        rx.poll(1001),
        Some(StreamSideband::Ack {
            have: UVlq64(5),
            ..
        })
    ));
    // can't confirm what didn't arrive
    rx.ack(100);
    assert!(matches!(
        rx.poll(2000),
        Some(StreamSideband::Ack {
            have: UVlq64(10),
            ..
        })
    ));
    assert_eq!(rx.cursor().unwrap().pos, UVlq64(10));
}

#[test]
fn resume_gaps_by_cause() {
    let cursor = StreamCursor {
        session: UVlq32(4),
        pos: UVlq64(100),
    };
    // cursor older than the store: the store didn't keep 100..150
    let mut rx = Receiver::new(Delivery::Reliable);
    let (g, ended) = rx.opened(
        Some(cursor),
        &Opened {
            session: 4,
            oldest: 150,
            live_from: 300,
            resume_from: Some(150),
        },
    );
    assert_eq!(
        g.iter().copied().collect::<Vec<_>>(),
        [gap(Run::Backfill, 100, 50, Cause::Store)]
    );
    assert_eq!(ended, None);
    assert!(rx.is_backfilling());

    // inside the store: nothing missing
    let mut rx = Receiver::new(Delivery::Reliable);
    let (g, _) = rx.opened(
        Some(cursor),
        &Opened {
            session: 4,
            oldest: 50,
            live_from: 300,
            resume_from: Some(100),
        },
    );
    assert!(g.is_empty());

    // nothing kept: all of it gone
    let (g, _) = rx.opened(
        Some(cursor),
        &Opened {
            session: 4,
            oldest: 300,
            live_from: 300,
            resume_from: None,
        },
    );
    assert_eq!(
        g.iter().copied().collect::<Vec<_>>(),
        [gap(Run::Live, 100, 200, Cause::Store)]
    );

    // counted: what the host missed is the device's
    let mut rx = Receiver::new(Delivery::Counted);
    let (g, _) = rx.opened(
        Some(cursor),
        &Opened {
            session: 4,
            oldest: 120,
            live_from: 120,
            resume_from: None,
        },
    );
    assert_eq!(
        g.iter().copied().collect::<Vec<_>>(),
        [gap(Run::Live, 100, 20, Cause::Device)]
    );

    // another session: the old one ended, the new one is taken from its start
    let mut rx = Receiver::new(Delivery::Reliable);
    let (g, ended) = rx.opened(
        Some(cursor),
        &Opened {
            session: 5,
            oldest: 0,
            live_from: 30,
            resume_from: Some(0),
        },
    );
    assert!(g.is_empty());
    assert_eq!(ended, Some(4));
    assert_eq!(rx.session(), Some(5));
}

#[test]
fn two_runs() {
    let mut rx = Receiver::new(Delivery::Reliable);
    rx.opened(
        Some(StreamCursor {
            session: UVlq32(1),
            pos: UVlq64(10),
        }),
        &Opened {
            session: 1,
            oldest: 0,
            live_from: 20,
            resume_from: Some(10),
        },
    );
    assert!(matches!(
        rx.poll(0),
        Some(StreamSideband::Ack {
            have: UVlq64(20),
            backfill: Some(UVlq64(10)),
            ..
        })
    ));
    // live first, then the backlog interleaved
    assert!(matches!(
        rx.on_item(&head(20, 5, 0)),
        Verdict::Accept { run: Run::Live, .. }
    ));
    assert!(matches!(
        rx.on_item(&head(10, 5, 0)),
        Verdict::Accept {
            run: Run::Backfill,
            backfill_done: false,
            ..
        }
    ));
    assert!(matches!(
        rx.on_item(&head(25, 5, 0)),
        Verdict::Accept { run: Run::Live, .. }
    ));
    // a gap in the backfill run asks a resend there
    assert_eq!(rx.on_item(&head(16, 4, 0)), Verdict::OutOfOrder);
    assert!(matches!(
        rx.poll(1),
        Some(StreamSideband::Resend {
            from: UVlq64(15),
            ..
        })
    ));
    rx.on_item(&head(15, 1, 0));
    assert!(matches!(
        rx.on_item(&head(16, 4, 0)),
        Verdict::Accept {
            run: Run::Backfill,
            backfill_done: true,
            ..
        }
    ));
    assert!(!rx.is_backfilling());
    for h in [
        head(20, 5, 0),
        head(10, 5, 0),
        head(25, 5, 0),
        head(15, 1, 0),
        head(16, 4, 0),
    ] {
        rx.delivered(&h, 1);
    }
    // the Ack that says the backlog is complete, then none with a backfill
    assert!(matches!(
        rx.poll(1000),
        Some(StreamSideband::Ack {
            have: UVlq64(30),
            backfill: Some(UVlq64(20)),
            ..
        })
    ));
    rx.on_item(&head(30, 1, 0));
    rx.delivered(&head(30, 1, 0), 1);
    assert!(matches!(
        rx.poll(2000),
        Some(StreamSideband::Ack {
            have: UVlq64(31),
            backfill: None,
            ..
        })
    ));
    // a late backfill duplicate is just a duplicate
    assert_eq!(rx.on_item(&head(10, 5, 0)), Verdict::Duplicate);
}

#[test]
fn query_finds_tail_loss_and_dropped_backlog() {
    let mut rx = Receiver::new(Delivery::Reliable);
    rx.opened(
        Some(StreamCursor {
            session: UVlq32(1),
            pos: UVlq64(0),
        }),
        &Opened {
            session: 1,
            oldest: 0,
            live_from: 50,
            resume_from: Some(0),
        },
    );
    rx.poll(0);
    rx.on_item(&head(50, 10, 0));
    rx.on_item(&head(0, 10, 0));
    // the device sent up to 70: the last event didn't arrive
    assert!(rx.on_query(70, Some(10)).is_empty());
    assert!(matches!(
        rx.poll(1),
        Some(StreamSideband::Resend {
            from: UVlq64(60),
            ..
        })
    ));
    rx.on_item(&head(60, 10, 0));
    // the device dropped the rest of the backlog
    let g = rx.on_query(70, None);
    assert_eq!(
        g.iter().copied().collect::<Vec<_>>(),
        [gap(Run::Backfill, 10, 40, Cause::Store)]
    );
    assert!(!rx.is_backfilling());
    assert_eq!(rx.stats().lost_store, 40);
}
