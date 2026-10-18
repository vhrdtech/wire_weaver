use super::*;
use crate::store::{RamRing, StreamStore};
use crate::tests::{Rng, block_on};
use crate::tx::{Counted, Reliable};
use crate::{Run, StreamCursor};
use wire_weaver::shrink_wrap::series::{Series, SeriesColumns, SeriesOwned};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, UVlq32, UVlq64};
use ww_si::{Ampere, Prefix, Second, Timed};

type Sample = Timed<Ampere>;

/// A sample that doesn't compress much.
fn noisy(k: u32) -> Sample {
    Timed::new(
        Second::micro(k * k * 977),
        Ampere::new(Prefix::Milli, (k as i32 * 7919) % 2001 - 1000),
    )
}

fn sample(k: u32) -> Sample {
    Timed::new(
        Second::micro(k * 40),
        Ampere::new(Prefix::Milli, (k as i32 % 9) - 4),
    )
}

fn decode<T: Clone + for<'i> SeriesColumns<'i>>(bytes: &[u8]) -> Vec<T>
where
    SeriesOwned<T>: DeserializeShrinkWrapOwned,
{
    SeriesOwned::<T>::from_ww_bytes_owned(bytes)
        .expect("decodes")
        .into_inner()
}

type Scr = Scratch<{ scratch_bytes::<Sample>(32) }>;

#[test]
fn seal_full() {
    let mut b = Batcher::<u32, 8, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u32>(8) }>::new();
    for k in 0..7 {
        assert_eq!(b.push(&(k * 3)), Seal::No);
    }
    assert!(!b.has_sealed());
    assert_eq!(b.push(&21), Seal::Sealed(Cause::Full));
    assert_eq!((b.filling(), b.sealed_items()), (0, 8));
    let mut buf = [0u8; 64];
    let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!((enc.items, enc.lost_before, enc.cause), (8, 0, Cause::Full));
    assert_eq!(
        decode::<u32>(&buf[..enc.len]),
        (0..8).map(|k| k * 3).collect::<Vec<_>>()
    );
    assert!(!b.has_sealed());
    assert_eq!(b.encode_into(&mut scratch, &mut buf), Ok(None));
    assert_eq!(b.stats().sealed, [1, 0, 0, 0]);
}

#[test]
fn seal_age_from_first_poll() {
    let mut b = Batcher::<Sample, 32, _>::new(DEFAULT_POLICY);
    let mut scratch = Scr::new();
    b.push(&sample(0));
    b.push(&sample(1));
    // the first poll that sees the batch starts its clock
    assert_eq!(b.poll(5_000, Some(1_000), 1024), Polled::default());
    // more items of the same batch keep its clock
    b.push(&sample(2));
    assert_eq!(b.poll(5_999, Some(1_000), 1024), Polled::default());
    assert_eq!(b.poll(6_000, Some(1_000), 1024).sealed, Some(Cause::Age));
    assert_eq!(b.sealed_items(), 3);
    assert_eq!(b.stats().sealed[Cause::Age as usize], 1);
    // no age limit: never sealed by age
    b.push(&sample(3));
    b.discard_sealed();
    assert_eq!(b.poll(1 << 40, None, 1024), Polled::default());
    let _ = &mut scratch;
}

#[test]
fn seal_age_waits_for_the_spare() {
    let mut b = Batcher::<Sample, 32, _>::new(DEFAULT_POLICY);
    let mut scratch = Scr::new();
    b.push(&sample(0));
    b.flush();
    b.push(&sample(1));
    // the spare is out: the due batch waits
    assert_eq!(b.poll(0, Some(0), 1024).sealed, None);
    let mut buf = [0u8; 512];
    b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!(b.poll(0, Some(0), 1024).sealed, Some(Cause::Age));
    // an empty batch is never sealed
    b.discard_sealed();
    assert_eq!(b.poll(10_000, Some(0), 1024), Polled::default());
}

#[test]
fn seal_budget_worst_case() {
    let budget = 400;
    let mut b = Batcher::<Sample, 32, _>::with_budget(CodecPolicy::SMALLEST, budget);
    let mut scratch = Scr::new();
    let mut k = 0;
    let seal = loop {
        let seal = b.push(&sample(k));
        k += 1;
        if seal != Seal::No {
            break seal;
        }
    };
    assert_eq!(seal, Seal::Sealed(Cause::Budget));
    let n = k as usize;
    assert!(n < 32);
    // as many as the worst case allows
    assert!(Series::<Sample>::max_bytes(n).unwrap() <= budget);
    assert!(Series::<Sample>::max_bytes(n + 1).unwrap() > budget);
    let mut buf = [0u8; 400];
    let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!(enc.items, n);
    assert_eq!(b.stats().split, 0);
}

#[test]
fn seal_budget_from_the_last_batch() {
    // the worst case allows only a few items, the real ones (a regular clock, a slow signal) take far less
    let budget = 200;
    let mut b = Batcher::<Sample, 32, _>::with_budget(CodecPolicy::SMALLEST, budget);
    let mut scratch = Scr::new();
    let worst = (1..=32)
        .take_while(|&n| Series::<Sample>::max_bytes(n).unwrap() <= budget)
        .last()
        .unwrap();
    let mut buf = [0u8; 200];
    let mut k = 0;
    let mut first = true;
    for _ in 0..3 {
        let mut n = 0;
        while b.push(&sample(k)) == Seal::No {
            k += 1;
            n += 1;
        }
        k += 1;
        n += 1;
        if first {
            assert_eq!(n, worst, "the first batch: the worst case");
            first = false;
        } else {
            assert!(n > worst, "{n} items, the worst case allows {worst}");
        }
        let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
        assert_eq!(enc.items, n, "fits whole");
        assert!(enc.len <= budget);
    }
}

#[test]
fn budget_shrinks_seals_at_poll() {
    let mut b = Batcher::<Sample, 32, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    for k in 0..20 {
        b.push(&noisy(k));
    }
    assert_eq!(b.poll(0, None, 4096), Polled::default());
    assert_eq!(b.poll(0, None, 30).sealed, Some(Cause::Budget));
    assert_eq!(b.stats().sealed[Cause::Budget as usize], 1);
    // sealed before the budget shrank: split to fit
    let mut buf = [0u8; 30];
    let mut got = vec![];
    while let Some(enc) = b.encode_into(&mut scratch, &mut buf).unwrap() {
        assert!(enc.len <= 30);
        got.extend(decode::<Sample>(&buf[..enc.len]));
    }
    assert!(b.stats().split >= 1);
    assert_eq!(got, (0..20).map(noisy).collect::<Vec<_>>());
}

#[test]
fn spare_still_out_loses_the_next_batch() {
    let mut b = Batcher::<u32, 4, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u32>(4) }>::new();
    for k in 0..4 {
        b.push(&k);
    }
    for k in 4..7 {
        assert_eq!(b.push(&k), Seal::No);
    }
    assert_eq!(
        b.push(&7),
        Seal::Lost {
            cause: Cause::Full,
            items: 4
        }
    );
    // the store started over
    assert_eq!(b.filling(), 0);
    b.push(&8);
    assert_eq!(
        b.flush(),
        Seal::Lost {
            cause: Cause::Flush,
            items: 1
        }
    );
    let mut buf = [0u8; 32];
    let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!(decode::<u32>(&buf[..enc.len]), vec![0, 1, 2, 3]);
    assert_eq!(enc.lost_before, 0);
    // back: the next batch carries the loss before it
    b.push(&9);
    assert_eq!(b.flush(), Seal::Sealed(Cause::Flush));
    let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!((enc.items, enc.lost_before), (1, 5));
    assert_eq!(b.stats().lost, 5);
}

#[test]
fn loss_without_a_batch_after_it_comes_from_poll() {
    let mut b = Batcher::<u32, 4, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u32>(4) }>::new();
    b.push_many(0..8);
    let mut buf = [0u8; 32];
    assert_eq!(b.poll(0, None, 32).lost, 0, "not before the sealed batch");
    b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!(b.poll(0, None, 32).lost, 4);
    assert_eq!(b.poll(0, None, 32).lost, 0);
}

#[test]
fn split_keeps_every_item_once() {
    let mut b = Batcher::<Sample, 32, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    for k in 0..32 {
        b.push(&noisy(k));
    }
    let all: Vec<Sample> = (0..32).map(noisy).collect();
    let mut buf = [0u8; 24];
    let mut got = vec![];
    let mut events = 0;
    while let Some(enc) = b.encode_into(&mut scratch, &mut buf).unwrap() {
        got.extend(decode::<Sample>(&buf[..enc.len]));
        events += 1;
    }
    assert!(events > 1);
    assert_eq!(got, all);
}

#[test]
fn no_room_keeps_the_batch() {
    let mut b = Batcher::<Sample, 32, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    b.push(&sample(0));
    b.flush();
    assert_eq!(
        b.encode_into(&mut scratch, &mut [0u8; 1]),
        Err(EncodeError::NoRoom)
    );
    assert_eq!(b.sealed_items(), 1);
    assert_eq!(b.discard_sealed(), (1, 0));
    assert_eq!(b.stats().lost, 1);
}

#[test]
fn push_many_seals_and_loses() {
    let mut b = Batcher::<u32, 4, _>::new(CodecPolicy::SMALLEST);
    let (seal, lost) = b.push_many(0..10);
    // 0..4 sealed, 4..8 lost, 8..10 filling
    assert_eq!(
        (seal, lost),
        (
            Seal::Lost {
                cause: Cause::Full,
                items: 4
            },
            4
        )
    );
    assert_eq!((b.sealed_items(), b.filling()), (4, 2));
}

#[test]
fn sealed_len_is_exact() {
    let mut b = Batcher::<Sample, 32, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    for k in 0..32 {
        b.push(&sample(k));
    }
    let len = b.sealed_len(&mut scratch).unwrap().unwrap();
    assert!(len <= b.sealed_bound().unwrap());
    let mut buf = [0u8; 512];
    let enc = b.encode_into(&mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!(enc.len, len);
}

#[test]
fn outbox_drops_oldest_for_room() {
    let mut b = Batcher::<u32, 8, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u32>(8) }>::new();
    let mut outbox = Outbox::<64>::new();
    let mut entries = 0;
    let mut dropped = 0;
    for round in 0..20u32 {
        for k in 0..8 {
            b.push(&(round * 1000 + k * k * 31));
        }
        let done = outbox
            .push_from((round % 3) as u16, &mut b.consumer(), &mut scratch)
            .unwrap();
        assert_eq!(done.lost, 0);
        entries += done.batches;
        dropped += done.dropped;
        assert!(!b.has_sealed(), "fits after dropping");
    }
    assert!(dropped > 0);
    assert_eq!(outbox.len() as u64 * 8 + dropped, 20 * 8);
    assert!(entries >= 20);
    // the newest stay, every entry decodes
    let last = outbox.iter().last().unwrap();
    assert_eq!(last.k, 19 % 3);
    assert_eq!(decode::<u32>(last.bytes)[0], 19_000);
    let first_k1 = outbox.first_of(1).unwrap().bytes.to_vec();
    assert!(outbox.remove_first_of(1));
    assert!(outbox.iter().all(|e| e.bytes != first_k1.as_slice()));
    assert!(!outbox.remove_first_of(7));
    let of_2 = outbox.iter().filter(|e| e.k == 2).count() as u64;
    let before = outbox.len() as u64;
    assert_eq!(outbox.remove_all_of(2), of_2 * 8);
    assert_eq!(outbox.len() as u64, before - of_2);
    assert!(outbox.first_of(2).is_none() && outbox.first_of(0).is_some());
    let left = outbox.len() as u64;
    assert_eq!(outbox.clear(), left * 8);
    assert!(outbox.is_empty());
}

#[test]
fn outbox_item_bigger_than_outbox_is_lost() {
    let mut b = Batcher::<u128, 4, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u128>(4) }>::new();
    let mut outbox = Outbox::<12>::new();
    b.push(&u128::MAX);
    b.flush();
    let done = outbox
        .push_from(0, &mut b.consumer(), &mut scratch)
        .unwrap();
    assert_eq!((done.batches, done.lost), (0, 1));
    assert!(!b.has_sealed());
}

#[test]
fn outbox_split_batch_never_drops_its_own_entries() {
    let mut b = Batcher::<u32, 32, _>::new(CodecPolicy::SMALLEST);
    let mut scratch = Scratch::<{ scratch_bytes::<u32>(32) }>::new();
    let mut outbox = Outbox::<24>::new();
    let mut rng = Rng::new(1);
    for _ in 0..32 {
        b.push(&(rng.next() as u32));
    }
    let done = outbox
        .push_from(0, &mut b.consumer(), &mut scratch)
        .unwrap();
    assert_eq!(done.dropped, 0);
    assert!(
        done.items > 0 && done.items < 32,
        "{done:?} {:?}",
        b.stats()
    );
    assert!(b.has_sealed(), "the rest waits for room");
    let first = outbox.iter().next().unwrap().items;
    // the consumer sends it, the rest goes in
    outbox.pop_oldest();
    let done = outbox
        .push_from(0, &mut b.consumer(), &mut scratch)
        .unwrap();
    assert!(done.items > 0);
    assert!(first > 0);
}

#[test]
fn reliable_batches_into_the_store() {
    let mut s = Batched::<_, Sample, 32, _>::new(
        Reliable::<RamRing<4096>>::new(RamRing::new()),
        CodecPolicy::SMALLEST,
    );
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    block_on(s.stream.open(1)).unwrap();
    for k in 0..32 {
        s.push(&sample(k));
    }
    // the next batch is lost while the first waits
    for k in 32..64 {
        s.push(&sample(k));
    }
    for k in 64..70 {
        s.push(&sample(k));
    }
    let done = block_on(s.poll(0, Some(1_000), &mut scratch, &mut buf)).unwrap();
    assert_eq!(
        done,
        BatchedPoll {
            stored: 1,
            lost: 32
        }
    );
    let done = block_on(s.poll(1_000, Some(1_000), &mut scratch, &mut buf)).unwrap();
    assert_eq!(done, BatchedPoll { stored: 1, lost: 0 });
    let ev = block_on(s.stream.next_event(Run::Live, &mut buf))
        .unwrap()
        .unwrap();
    assert_eq!((ev.head.pos, ev.head.units, ev.head.t), (0, 32, None));
    assert_eq!(
        decode::<Sample>(ev.item),
        (0..32).map(sample).collect::<Vec<_>>()
    );
    let ev = block_on(s.stream.next_event(Run::Live, &mut buf))
        .unwrap()
        .unwrap();
    // the lost batch is the gap before the next one
    assert_eq!(
        (ev.head.pos, ev.head.units, ev.head.lost),
        (64, 6, Some(32))
    );
    assert_eq!(
        decode::<Sample>(ev.item),
        (64..70).map(sample).collect::<Vec<_>>()
    );
}

#[test]
fn reliable_budget_is_the_record_limit() {
    // a ring whose record limit is below `buf`
    let mut s = Batched::<_, Sample, 32, _>::new(
        Reliable::<RamRing<600>>::new(RamRing::new()),
        CodecPolicy::SMALLEST,
    );
    let limit = s.stream.store().max_item_len();
    assert_eq!(limit, 600 - crate::store::RECORD_OVERHEAD - 64);
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    block_on(s.stream.open(1)).unwrap();
    let done = block_on(s.poll(0, None, &mut scratch, &mut buf)).unwrap();
    assert_eq!(done, BatchedPoll::default());
    let mut k = 0;
    while s.push(&sample(k)) == Seal::No {
        k += 1;
    }
    let done = block_on(s.poll(0, None, &mut scratch, &mut buf)).unwrap();
    assert_eq!(done, BatchedPoll { stored: 1, lost: 0 });
}

#[test]
fn reliable_without_session_doesnt_encode() {
    let mut s = Batched::<_, Sample, 32, _>::new(
        Reliable::<RamRing<4096>>::new(RamRing::new()),
        CodecPolicy::SMALLEST,
    );
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    s.push(&sample(0));
    s.batcher.flush();
    let done = block_on(s.poll(0, None, &mut scratch, &mut buf)).unwrap();
    assert_eq!(done, BatchedPoll { stored: 0, lost: 1 });
    assert_eq!(s.batcher.stats().encoded, 0);
}

#[test]
fn counted_batches_with_heads() {
    let mut s = Batched::<_, Sample, 32, _>::new(Counted::new(), CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    // no host: the batch is dropped unencoded, positions move on once there is a session
    s.stream.start_session(3);
    for k in 0..4 {
        s.push(&sample(k));
    }
    assert_eq!(s.poll(0, Some(0), &mut scratch, &mut buf), Ok(None));
    assert_eq!(s.batcher.stats().encoded, 0);
    s.stream
        .resume(StreamCursor {
            session: UVlq32(3),
            pos: UVlq64(0),
        })
        .unwrap();
    for k in 4..10 {
        s.push(&sample(k));
    }
    let (head, bytes) = s
        .poll(10, Some(0), &mut scratch, &mut buf)
        .unwrap()
        .unwrap();
    assert_eq!(
        (head.pos, head.units, head.lost, head.t),
        (4, 6, Some(4), None)
    );
    assert_eq!(
        decode::<Sample>(bytes),
        (4..10).map(sample).collect::<Vec<_>>()
    );
}

#[test]
fn plain_batches_while_open() {
    let mut s = Batched::<_, Sample, 32, _>::new(Plain::new(), CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    // no host: dropped unencoded
    for k in 0..4 {
        s.push(&sample(k));
    }
    assert_eq!(s.poll(0, Some(0), &mut scratch, &mut buf), Ok(None));
    assert_eq!(s.batcher.stats().encoded, 0);
    assert_eq!(s.batcher.stats().discarded, 4);
    s.stream.open();
    for k in 4..10 {
        s.push(&sample(k));
    }
    let (items, bytes) = s
        .poll(10, Some(0), &mut scratch, &mut buf)
        .unwrap()
        .unwrap();
    assert_eq!(items, 6);
    assert_eq!(
        decode::<Sample>(bytes),
        (4..10).map(sample).collect::<Vec<_>>()
    );
    s.stream.close();
    s.push(&sample(10));
    assert_eq!(s.poll(20, Some(0), &mut scratch, &mut buf), Ok(None));
}

#[test]
fn max_age_within_bounds() {
    let mut s = Batched::<_, Sample, 32, _>::new(Plain::new(), CodecPolicy::SMALLEST);
    assert_eq!(s.max_age_us(), DEFAULT_MAX_AGE_US);
    assert_eq!(s.grant_max_age(Some(0)), MIN_MAX_AGE_US);
    assert_eq!(s.grant_max_age(Some(u32::MAX)), MAX_MAX_AGE_US);
    assert_eq!(s.grant_max_age(Some(20_000)), 20_000);
    assert_eq!(s.max_age_us(), 20_000);
    assert_eq!(s.grant_max_age(None), DEFAULT_MAX_AGE_US);
    let mut s = s.with_max_age(0, 5, 3);
    // min at least 1, max at least min, default within them
    assert_eq!(s.max_age_us(), 3);
    assert_eq!(s.grant_max_age(Some(0)), 1);
}

#[test]
fn consumer_apart_from_its_producer() {
    let batcher = Box::leak(Box::new(Batcher::<Sample, 32, _>::new(
        CodecPolicy::SMALLEST,
    )));
    let (mut producer, consumer) = batcher.split();
    let mut s = Batched::with_consumer(Counted::new(), consumer);
    s.stream.start_session(1);
    s.stream
        .resume(StreamCursor {
            session: UVlq32(1),
            pos: UVlq64(0),
        })
        .unwrap();
    for k in 0..5 {
        Push::push(&mut producer, &sample(k));
    }
    let mut scratch = Scr::new();
    let mut buf = [0u8; 512];
    let (head, bytes) = s.poll(0, Some(0), &mut scratch, &mut buf).unwrap().unwrap();
    assert_eq!((head.pos, head.units), (0, 5));
    assert_eq!(
        decode::<Sample>(bytes),
        (0..5).map(sample).collect::<Vec<_>>()
    );
}

#[test]
fn random_runs() {
    let mut rng = Rng::new(7);
    for _ in 0..40 {
        let len = rng.range(0, 2000) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        check::run(&bytes);
    }
}

#[test]
fn default_policy() {
    #[cfg(feature = "std")]
    assert_eq!(DEFAULT_POLICY, CodecPolicy::SMALLEST);
    #[cfg(not(feature = "std"))]
    {
        use wire_weaver::shrink_wrap::series::SeriesPolicy;
        assert_eq!(DEFAULT_POLICY.numeric(), FIXED_DELTA_PACKED.numeric());
        assert_eq!(DEFAULT_POLICY.float(), FIXED_DELTA_PACKED.float());
    }
}

#[test]
fn threaded_hand_off() {
    // shorter under miri, which checks the hand-off for data races
    let count = if cfg!(miri) { 300 } else { 8_000 };
    for seed in 0..if cfg!(miri) { 4 } else { 12 } {
        check::threaded(count, seed);
    }
}
