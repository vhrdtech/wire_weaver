//! A [PersistentRamRing] across simulated soft resets: the area's image is carried into a new store, as the RAM is
//! after `sys_reset`, or replaced by garbage, as after a power-on.

use super::{Rng, block_on};
use crate::store::ram_ring::fault;
use crate::store::{
    FullPolicy, PERSISTENT_RECORD_OVERHEAD, PersistentRamRing, PushError, RamArea, RamRecovery,
    StoreState, StreamStore,
};
use crate::tx::{Pushed, Reliable};
use crate::{Run, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};

const N: usize = 512;

fn head(pos: u64, units: u64) -> StreamHead {
    StreamHead {
        units,
        ..StreamHead::at(pos)
    }
}

/// What a store holds: its state and every event.
fn contents<const M: usize>(
    store: &mut PersistentRamRing<'_, M>,
) -> (StoreState, Vec<(StreamHead, Vec<u8>)>) {
    let mut events = Vec::new();
    let mut pos = 0;
    let mut buf = [0u8; 256];
    while let Some(e) = block_on(store.read(pos, &mut buf)).unwrap() {
        pos = e.head.end().max(e.head.pos + 1);
        events.push((e.head, e.item.to_vec()));
    }
    (store.state(), events)
}

/// The RAM after a soft reset: a new area holding the old one's bytes.
fn reset_copy<const M: usize>(area: &RamArea<M>) -> Box<RamArea<M>> {
    let mut next = Box::new(RamArea::<M>::new());
    next.load_image(area.image());
    next
}

fn epoch_of(head: &StreamHead) -> Option<u32> {
    head.ext.and_then(|e| e.clock_epoch).map(|e| e.0)
}

#[test]
fn survives_a_soft_reset() {
    let mut area = Box::new(RamArea::<N>::new());
    let mut store = PersistentRamRing::attach(&mut area);
    assert_eq!(
        store.recovery(),
        RamRecovery {
            formatted: true,
            truncated_bytes: 0,
            clock_epoch: 0
        }
    );
    assert!(store.is_empty());
    block_on(store.reset(5)).unwrap();
    for i in 0..6u64 {
        block_on(store.push(&head(i * 4, 4), &[i as u8; 10])).unwrap();
    }
    block_on(store.release(8)).unwrap();
    let before = contents(&mut store);
    assert!(before.0.persistent);
    assert_eq!(
        (before.0.session, before.0.oldest, before.0.newest),
        (5, Some(8), Some(20))
    );
    // a new ring: no epoch marked
    assert!(before.1.iter().all(|(h, _)| h.ext.is_none()));

    let mut area = reset_copy(&area);
    let mut store = PersistentRamRing::attach(&mut area);
    assert_eq!(
        store.recovery(),
        RamRecovery {
            formatted: false,
            truncated_bytes: 0,
            clock_epoch: 1
        }
    );
    assert_eq!(
        contents(&mut store),
        before,
        "bit-identical after the reset"
    );

    // the first event of the new boot carries its epoch, the next ones don't
    block_on(store.push(&head(24, 4), b"after")).unwrap();
    block_on(store.push(&head(28, 4), b"after")).unwrap();
    let (state, events) = contents(&mut store);
    assert_eq!(state.next, 32);
    let epochs: Vec<_> = events.iter().map(|(h, _)| epoch_of(h)).collect();
    assert_eq!(epochs, [None, None, None, None, Some(1), None]);

    // every boot is a new epoch, also one that pushed nothing
    let mut area = reset_copy(&area);
    assert_eq!(PersistentRamRing::attach(&mut area).clock_epoch(), 2);
    let mut area = reset_copy(&area);
    let mut store = PersistentRamRing::attach(&mut area);
    assert_eq!(store.clock_epoch(), 3);
    // a sender's own epoch is kept
    let mut h = head(32, 1);
    h.ext = Some(crate::HeadExt {
        clock_epoch: Some(UVlq32(99)),
    });
    block_on(store.push(&h, b"x")).unwrap();
    let (_, events) = contents(&mut store);
    assert_eq!(epoch_of(&events.last().unwrap().0), Some(99));
}

#[test]
fn drop_oldest_wraps_and_survives() {
    let mut area = Box::new(RamArea::<N>::new());
    let mut store = PersistentRamRing::attach(&mut area).with_policy(FullPolicy::DropOldest);
    block_on(store.reset(1)).unwrap();
    for i in 0..100u64 {
        block_on(store.push(&head(i * 2, 2), &[i as u8; 17])).unwrap();
    }
    let before = contents(&mut store);
    assert!(before.0.dropped_units > 0);
    assert_eq!(before.0.next, 200);
    assert_eq!(before.1.last().unwrap().1, vec![99; 17]);
    let mut area = reset_copy(&area);
    let mut store = PersistentRamRing::attach(&mut area);
    assert_eq!(contents(&mut store), before);
}

#[test]
fn record_overhead() {
    let mut area = Box::new(RamArea::<N>::new());
    let mut store = PersistentRamRing::attach(&mut area);
    block_on(store.push(&head(10, 2), &[0; 10])).unwrap();
    // head with pos < 512 and units != 1: 3 bytes
    assert_eq!(
        store.state().used_bytes,
        (PERSISTENT_RECORD_OVERHEAD + 3 + 10) as u64
    );
    assert_eq!(
        block_on(store.push(&head(12, 1), &[0; N])),
        Err(PushError::TooBig)
    );
}

#[test]
fn garbage_after_power_on_is_never_data() {
    let mut rng = Rng::new(1);
    let mut image = vec![0u8; RamArea::<N>::SIZE];
    for _ in 0..2000 {
        image.iter_mut().for_each(|b| *b = rng.next() as u8);
        let mut area = Box::new(RamArea::<N>::new());
        area.load_image(&image);
        let mut store = PersistentRamRing::attach(&mut area);
        assert!(store.recovery().formatted);
        assert_eq!(store.clock_epoch(), 0);
        let (state, events) = contents(&mut store);
        assert_eq!((state.session, state.next, state.used_bytes), (0, 0, 0));
        assert!(events.is_empty());
    }
    // all ones and all zeros (RAM that held a pattern)
    for fill in [0x00, 0xff, 0x55] {
        let mut area = Box::new(RamArea::<N>::new());
        area.load_image(&vec![fill; RamArea::<N>::SIZE]);
        assert!(PersistentRamRing::attach(&mut area).recovery().formatted);
    }
}

/// A valid image with one bit flipped anywhere in the header slots: either the other slot is taken (same events,
/// one step older at most), or a new ring starts; never anything else.
#[test]
fn flipped_bit_in_a_slot() {
    let mut area = Box::new(RamArea::<N>::new());
    let mut store = PersistentRamRing::attach(&mut area);
    block_on(store.reset(3)).unwrap();
    for i in 0..5u64 {
        block_on(store.push(&head(i, 1), &[i as u8; 8])).unwrap();
    }
    let after = contents(&mut store);
    let mut before = None;
    for bit in 0..2 * crate::store::SLOT_LEN * 8 {
        let mut area = reset_copy(&area);
        let mut image = area.image().to_vec();
        image[bit / 8] ^= 1 << (bit % 8);
        area.load_image(&image);
        let mut store = PersistentRamRing::attach(&mut area);
        assert!(!store.recovery().formatted, "the other slot is good");
        let got = contents(&mut store);
        if got != after {
            // the slot written last was hit: the one before it, without the last push
            assert_eq!(got.1, after.1[..4]);
            before.get_or_insert(got);
        }
    }
    assert!(before.is_some());
}

#[test]
fn records_written_over_are_cut() {
    let mut area = Box::new(RamArea::<N>::new());
    let mut store = PersistentRamRing::attach(&mut area);
    block_on(store.reset(3)).unwrap();
    for i in 0..5u64 {
        block_on(store.push(&head(i * 2, 2), &[i as u8; 8])).unwrap();
    }
    let rec = PERSISTENT_RECORD_OVERHEAD + 3 + 8;
    let mut area = reset_copy(&area);
    let mut image = area.image().to_vec();
    // something ran wild into the third record's item
    image[2 * crate::store::SLOT_LEN + 2 * rec + rec - 2] ^= 0x10;
    area.load_image(&image);
    let mut store = PersistentRamRing::attach(&mut area);
    assert_eq!(store.recovery().truncated_bytes, 3 * rec as u32);
    let (state, events) = contents(&mut store);
    assert_eq!(events.len(), 2);
    assert_eq!((state.newest, state.next), (Some(2), 10), "positions go on");
    block_on(store.push(&head(10, 1), b"next")).unwrap();
    assert_eq!(store.state().newest, Some(10));
}

/// Fault in an operation after every number of bytes written, reset, attach: the store holds what it held before
/// the operation or after it, nothing in between, and never garbage. A push that `drops` the oldest events may also
/// stop after dropping them.
fn fault_at_every_byte(
    drops: bool,
    prepare: impl Fn(&mut PersistentRamRing<'_, N>),
    op: impl Fn(&mut PersistentRamRing<'_, N>),
) {
    let mut base = Box::new(RamArea::<N>::new());
    prepare(&mut PersistentRamRing::attach(&mut base).with_policy(FullPolicy::DropOldest));

    // the store as it boots from the base image, with the operation done or not
    let mut area = reset_copy(&base);
    let mut store = PersistentRamRing::attach(&mut area).with_policy(FullPolicy::DropOldest);
    let mut before = contents(&mut store);
    op(&mut store);
    let mut after = contents(&mut store);
    let mut area = reset_copy(&area);
    let (state, events) = contents(&mut PersistentRamRing::attach(&mut area));
    // only the epoch of a later boot differs
    assert_eq!(events, after.1);
    after.0 = state;
    let mut area = reset_copy(&base);
    PersistentRamRing::attach(&mut area);
    let mut area = reset_copy(&area);
    before.0 = PersistentRamRing::attach(&mut area).state();

    // a push that drops the oldest events to make room may stop after dropping them, before the new one is in
    let dropped = drops.then(|| after.1[..after.1.len() - 1].to_vec());
    // 0: before, 1: oldest dropped, 2: after; a later fault never goes back to an earlier phase
    let mut seen = [false; 3];
    let mut phase = 0;
    for bytes in 0.. {
        let mut area = reset_copy(&base);
        let mut store = PersistentRamRing::attach(&mut area).with_policy(FullPolicy::DropOldest);
        fault::after_bytes(Some(bytes));
        op(&mut store);
        fault::after_bytes(None);
        let mut area = reset_copy(&area);
        let mut store = PersistentRamRing::attach(&mut area);
        assert!(!store.recovery().formatted, "fault after {bytes} bytes");
        assert_eq!(
            store.recovery().truncated_bytes,
            0,
            "fault after {bytes} bytes"
        );
        let got = contents(&mut store);
        let now = if got == before {
            0
        } else if got == after {
            2
        } else if dropped.as_ref() == Some(&got.1) && got.0.next == before.0.next {
            1
        } else {
            panic!("fault after {bytes} bytes: {got:?}");
        };
        assert!(
            now >= phase,
            "fault after {bytes} bytes: back to an earlier state"
        );
        phase = now;
        seen[now] = true;
        if now == 2 && bytes > 2 * (PERSISTENT_RECORD_OVERHEAD + 64 + 32 + crate::store::SLOT_LEN) {
            break;
        }
    }
    assert!(seen[0] && seen[2]);
    assert_eq!(seen[1], dropped.is_some());
}

fn filled(store: &mut PersistentRamRing<'_, N>) {
    block_on(store.reset(9)).unwrap();
    for i in 0..40u64 {
        block_on(store.push(&head(i * 3, 3), &[i as u8; 21])).unwrap();
    }
    assert!(store.state().dropped_units > 0, "the ring has wrapped");
}

#[test]
fn fault_during_push_dropping_oldest() {
    fault_at_every_byte(true, filled, |s| {
        // two records' worth: drops the oldest, wraps around the end
        let _ = block_on(s.push(&head(120, 3), &[0xaa; 32]));
    });
}

#[test]
fn fault_during_push_with_room() {
    fault_at_every_byte(
        false,
        |s| {
            block_on(s.reset(2)).unwrap();
            block_on(s.push(&head(0, 1), b"first")).unwrap();
        },
        |s| {
            let _ = block_on(s.push(&head(1, 1), &[0x5a; 30]));
        },
    );
}

#[test]
fn fault_during_release_and_reset() {
    fault_at_every_byte(false, filled, |s| block_on(s.release(100)).unwrap());
    fault_at_every_byte(false, filled, |s| block_on(s.reset(10)).unwrap());
}

/// A reliable stream on the ring goes on with its session and positions after a reset, and the host gets the
/// events from before it.
#[test]
fn reliable_continues_after_reset() {
    let mut area = Box::new(RamArea::<1024>::new());
    let mut dev = Reliable::<_, 4>::new(PersistentRamRing::attach(&mut area)).initial_grant(10_000);
    block_on(dev.start_session(7)).unwrap();
    for i in 0..5u8 {
        assert_eq!(
            block_on(dev.push(&[i; 8], 10, None)).unwrap(),
            Pushed::Stored
        );
    }

    let mut area = reset_copy(&area);
    let mut dev = Reliable::<_, 4>::new(PersistentRamRing::attach(&mut area)).initial_grant(10_000);
    dev.continue_stored_session();
    assert_eq!((dev.session(), dev.next_pos()), (Some(7), 50));
    assert_eq!(
        block_on(dev.push(&[9; 8], 10, None)).unwrap(),
        Pushed::Stored
    );
    let cursor = StreamCursor {
        session: UVlq32(7),
        pos: UVlq64(0),
    };
    let opened = block_on(dev.resume(cursor)).unwrap();
    assert_eq!(opened.resume_from, Some(0));
    let mut buf = [0u8; 64];
    let mut sent = Vec::new();
    for run in [Run::Live, Run::Backfill] {
        while let Some(e) = block_on(dev.next_event(run, &mut buf)).unwrap() {
            sent.push((e.head.pos, epoch_of(&e.head)));
        }
    }
    sent.sort();
    assert_eq!(
        sent,
        [
            (0, None),
            (10, None),
            (20, None),
            (30, None),
            (40, None),
            (50, Some(1))
        ]
    );
}
