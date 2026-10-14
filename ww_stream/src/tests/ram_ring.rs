use super::block_on;
use crate::StreamHead;
use crate::store::{FullPolicy, PushError, RECORD_OVERHEAD, RamRing, RamRingError, StreamStore};

fn head(pos: u64, units: u64) -> StreamHead {
    StreamHead {
        units,
        ..StreamHead::at(pos)
    }
}

fn push<const N: usize>(
    ring: &mut RamRing<N>,
    pos: u64,
    units: u64,
    item: &[u8],
) -> Result<(), PushError<RamRingError>> {
    block_on(ring.push(&head(pos, units), item))
}

fn read<const N: usize>(ring: &mut RamRing<N>, pos: u64) -> Option<(StreamHead, Vec<u8>)> {
    let mut buf = [0u8; 256];
    block_on(ring.read(pos, &mut buf))
        .unwrap()
        .map(|e| (e.head, e.item.to_vec()))
}

/// Bytes one record takes: overhead, head (pos < 512 with units != 1: 3 bytes) and item.
const fn rec(item: usize) -> usize {
    RECORD_OVERHEAD + 3 + item
}

#[test]
fn push_read_release() {
    let mut ring = RamRing::<256>::new();
    assert!(ring.is_empty());
    assert_eq!(read(&mut ring, 0), None);
    for i in 0..5u64 {
        push(&mut ring, 10 + i * 3, 3, &[i as u8; 4]).unwrap();
    }
    let st = ring.state();
    assert_eq!((st.oldest, st.newest, st.next), (Some(10), Some(22), 25));
    assert_eq!(st.used_bytes, 5 * rec(4) as u64);
    assert!(!st.persistent);

    // exact position, a position inside an event and before everything
    assert_eq!(read(&mut ring, 13), Some((head(13, 3), vec![1; 4])));
    assert_eq!(read(&mut ring, 14).unwrap().0.pos, 16);
    assert_eq!(read(&mut ring, 0).unwrap().0.pos, 10);
    assert_eq!(read(&mut ring, 22).unwrap().0.pos, 22);
    assert_eq!(read(&mut ring, 23), None);
    // backwards after a forward read (the hint must not get in the way)
    assert_eq!(read(&mut ring, 11).unwrap().0.pos, 13);

    // release only whole events before the position
    block_on(ring.release(15)).unwrap();
    assert_eq!(ring.state().oldest, Some(13));
    block_on(ring.release(16)).unwrap();
    assert_eq!(ring.state().oldest, Some(16));
    assert_eq!(read(&mut ring, 0).unwrap().0.pos, 16);
    block_on(ring.release(100)).unwrap();
    let st = ring.state();
    assert_eq!(
        (st.oldest, st.newest, st.next, st.used_bytes),
        (None, None, 25, 0)
    );
    assert_eq!(st.dropped_units, 0);
}

#[test]
fn wraps_around() {
    // room for 3 records of 10-byte items
    const N: usize = 3 * rec(10) + 5;
    let mut ring = RamRing::<N>::new();
    let mut pos = 0;
    for i in 0..50u8 {
        push(&mut ring, pos, 2, &[i; 10]).unwrap();
        assert_eq!(
            read(&mut ring, pos),
            Some((head(pos, 2), vec![i; 10])),
            "i {i}"
        );
        if i >= 2 {
            // keep two records
            block_on(ring.release(pos - 2)).unwrap();
            assert_eq!(read(&mut ring, 0).unwrap().0.pos, pos - 2);
        }
        pos += 2;
    }
}

#[test]
fn drop_newest_when_full() {
    const N: usize = 2 * rec(8);
    let mut ring = RamRing::<N>::new();
    push(&mut ring, 0, 2, &[1; 8]).unwrap();
    push(&mut ring, 2, 2, &[2; 8]).unwrap();
    assert_eq!(push(&mut ring, 4, 2, &[3; 8]), Err(PushError::Full));
    let st = ring.state();
    assert_eq!(
        (st.oldest, st.newest, st.next, st.dropped_units),
        (Some(0), Some(2), 4, 0)
    );
    block_on(ring.release(2)).unwrap();
    push(&mut ring, 6, 2, &[4; 8]).unwrap();
    assert_eq!(read(&mut ring, 3).unwrap().0.pos, 6);
}

#[test]
fn drop_oldest_when_full() {
    const N: usize = 2 * rec(8);
    let mut ring = RamRing::<N>::new().with_policy(FullPolicy::DropOldest);
    push(&mut ring, 0, 2, &[1; 8]).unwrap();
    push(&mut ring, 2, 3, &[2; 8]).unwrap();
    push(&mut ring, 5, 2, &[3; 8]).unwrap();
    let st = ring.state();
    assert_eq!(
        (st.oldest, st.newest, st.next, st.dropped_units),
        (Some(2), Some(5), 7, 2)
    );
    // a bigger one pushes out two
    push(&mut ring, 7, 1, &[4; 30]).unwrap();
    let st = ring.state();
    assert_eq!(
        (st.oldest, st.newest, st.dropped_units),
        (Some(7), Some(7), 7)
    );
    assert_eq!(read(&mut ring, 0).unwrap().1, vec![4; 30]);
}

#[test]
fn too_big_and_small_buffer() {
    let mut ring = RamRing::<64>::new();
    assert_eq!(push(&mut ring, 0, 1, &[0; 64]), Err(PushError::TooBig));
    push(&mut ring, 0, 1, &[7; 20]).unwrap();
    let mut buf = [0u8; 10];
    assert_eq!(
        block_on(ring.read(0, &mut buf)),
        Err(RamRingError::BufferTooSmall { item_len: 20 })
    );
    let mut ring = RamRing::<64>::new();
    assert_eq!(
        push(&mut ring, 1 << 57, 1, &[]),
        Err(PushError::Store(RamRingError::Head(
            wire_weaver::shrink_wrap::Error::SubtypeOutOfRange
        )))
    );
}

#[test]
fn big_positions_and_full_heads() {
    let mut ring = RamRing::<512>::new();
    let t = ww_si::Second {
        prefix: ww_si::Prefix::Micro,
        value: ww_si::ww_numeric::NumericValue::U64(u64::MAX),
    };
    let h = StreamHead {
        pos: (1 << 50) + 7,
        units: 1 << 40,
        lost: Some(1 << 33),
        t: Some(t),
        ext: Some(crate::HeadExt {
            clock_epoch: Some(wire_weaver::shrink_wrap::UVlq32(u32::MAX)),
        }),
    };
    block_on(ring.push(&h, &[1, 2, 3])).unwrap();
    assert_eq!(read(&mut ring, 1 << 49), Some((h, vec![1, 2, 3])));
    assert_eq!(ring.state().next, (1 << 50) + 7 + (1 << 40));
}

#[test]
fn reset_starts_a_session() {
    let mut ring = RamRing::<128>::new().with_policy(FullPolicy::DropOldest);
    push(&mut ring, 0, 1, &[1; 40]).unwrap();
    push(&mut ring, 1, 1, &[1; 40]).unwrap();
    push(&mut ring, 2, 1, &[1; 40]).unwrap();
    assert!(ring.state().dropped_units > 0);
    block_on(ring.reset(7)).unwrap();
    let st = ring.state();
    assert_eq!(
        (
            st.session,
            st.oldest,
            st.newest,
            st.next,
            st.used_bytes,
            st.dropped_units
        ),
        (7, None, None, 0, 0, 0)
    );
    assert_eq!(read(&mut ring, 0), None);
    push(&mut ring, 0, 1, &[2; 3]).unwrap();
    assert_eq!(read(&mut ring, 0).unwrap().1, vec![2; 3]);
}
