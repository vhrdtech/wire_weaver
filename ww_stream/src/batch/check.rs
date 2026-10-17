//! A [Batcher] driven by random operations against a model: every encoded batch fits the room it was given and
//! decodes to the items pushed, in order; every item pushed is encoded exactly once or counted as lost exactly once;
//! [Outbox] entries always decode. Shared by the unit tests and the `batch-seal` fuzz target.

use super::{
    BatchStats, Batcher, Cause, EncodeError, Encoded, Outbox, Scratch, Seal, scratch_bytes,
};
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::series::{
    CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor, SeriesOwned,
};

/// A sequence number (to check order and loss), a signal and two discrete leaves.
pub type Item = (u32, i32, bool, Option<u16>);

const N: usize = 16;
const OUTBOX: usize = 192;

/// Bytes as a source of choices: runs out into zeros.
pub struct Picks<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Picks<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Picks { bytes, pos: 0 }
    }

    pub fn byte(&mut self) -> u8 {
        let b = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    pub fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

fn policy(pick: u8) -> CodecPolicy {
    match pick % 4 {
        0 => CodecPolicy::SMALLEST,
        1 => super::FIXED_DELTA_PACKED,
        2 => CodecPolicy::fixed(
            NumericCodec::new(Predictor::DeltaOfDelta, Coder::Prefix),
            FloatCodec::Plain,
        ),
        _ => CodecPolicy::fixed(
            NumericCodec::new(Predictor::Raw, Coder::Plain),
            FloatCodec::Xor,
        ),
    }
}

/// What the batcher was given and what came out of it.
struct Model {
    pushed: Vec<Item>,
    /// Next sequence number the receiver expects, and the items reported lost before it
    expect: u32,
    gap: u64,
    received: u64,
    /// Lost as the producer saw it ([Seal::Lost]) and as reported to the consumer (`lost_before`, polls, discards,
    /// outbox)
    seal_lost: u64,
    consumer_lost: u64,
    /// The gap isn't known exactly: an outbox push reports the losses of all its batches together
    inexact: bool,
}

impl Model {
    fn new() -> Self {
        Model {
            pushed: Vec::new(),
            expect: 0,
            gap: 0,
            received: 0,
            seal_lost: 0,
            consumer_lost: 0,
            inexact: false,
        }
    }

    fn push(&mut self, seal: Seal) {
        if let Seal::Lost { items, .. } = seal {
            self.seal_lost += items as u64;
        }
    }

    /// Items lost with nothing between them and the next batch.
    fn lost(&mut self, items: u64) {
        self.gap += items;
        self.consumer_lost += items;
    }

    /// Items of one encoded batch, decoded from `bytes`: right after what came before, apart from what was
    /// reported lost in between.
    fn receive(&mut self, items: usize, bytes: &[u8], exact_gap: bool) {
        let got = SeriesOwned::<Item>::from_ww_bytes_owned(bytes).expect("encoded batch decodes");
        assert_eq!(got.len(), items, "item count");
        let first = got[0].0;
        if exact_gap && !self.inexact {
            assert_eq!(
                first as u64,
                self.expect as u64 + self.gap,
                "gap before a batch"
            );
        } else {
            assert!(first as u64 >= self.expect as u64, "batch out of order");
        }
        self.gap = 0;
        self.inexact = false;
        for (i, item) in got.iter().enumerate() {
            assert_eq!(item.0, first + i as u32, "items in order");
            assert_eq!(item, &self.pushed[item.0 as usize], "item as pushed");
        }
        self.expect = first + items as u32;
        self.received += items as u64;
    }

    fn encoded(&mut self, enc: Encoded, bytes: &[u8]) {
        self.lost(enc.lost_before);
        self.receive(enc.items, bytes, true);
    }

    /// Every item once, every loss reported to both sides.
    fn finish(&self, stats: BatchStats) {
        assert_eq!(
            self.consumer_lost, stats.lost,
            "lost as reported to the consumer"
        );
        assert_eq!(
            self.seal_lost + stats.discarded,
            stats.lost,
            "lost as the producer saw it"
        );
        assert_eq!(
            self.received + stats.lost,
            self.pushed.len() as u64,
            "every item once"
        );
        assert_eq!(stats.encoded_items, self.received);
    }
}

/// Items with a sequence number from `start`: a ramp, ripple, bit flips or a constant, switching leaves.
fn items(
    start: u32,
    k: usize,
    shape: u8,
    value: &mut i32,
    flag: &mut bool,
    opt: &mut Option<u16>,
) -> Vec<Item> {
    (0..k)
        .map(|i| {
            *value = match shape % 4 {
                0 => value.wrapping_add(1),
                1 => value.wrapping_add((i as i32 % 7) - 3),
                2 => *value ^ ((shape as i32) << (i % 24)),
                _ => *value,
            };
            if shape & 0x10 != 0 && i % 5 == 0 {
                *flag = !*flag;
            }
            if shape & 0x20 != 0 && i % 3 == 0 {
                *opt = if opt.is_some() {
                    None
                } else {
                    Some(i as u16 * 300)
                };
            }
            (start + i as u32, *value, *flag, *opt)
        })
        .collect()
}

/// One run: operations picked from `bytes`, through the batcher's own methods or its two halves.
pub fn run(bytes: &[u8]) {
    let mut p = Picks::new(bytes);
    let policy = policy(p.byte());
    let mut budget = 1 + p.byte() as usize * 2;
    let mut b = Batcher::<Item, N>::with_budget(policy, budget);
    let mut scratch = Scratch::<{ scratch_bytes::<Item>(N) }>::new();
    let mut outbox = Outbox::<OUTBOX>::new();
    let mut m = Model::new();
    let mut buf = [0u8; 1024];
    let mut now = 0u64;
    let (mut value, mut flag, mut opt) = (0i32, false, None);
    let mut steps = 0;
    let (mut producer, mut consumer) = b.split();
    while !p.done() && steps < 4000 {
        steps += 1;
        match p.byte() % 11 {
            0..=3 => {
                let k = 1 + p.byte() as usize % 24;
                let shape = p.byte();
                let items = items(
                    m.pushed.len() as u32,
                    k,
                    shape,
                    &mut value,
                    &mut flag,
                    &mut opt,
                );
                m.pushed.extend_from_slice(&items);
                if shape & 0x40 != 0 {
                    let (seal, lost) = producer.push_many(items.iter().copied());
                    if let Seal::Lost { .. } = seal {
                        assert!(lost > 0);
                    }
                    m.seal_lost += lost as u64;
                } else {
                    for item in &items {
                        let seal = producer.push(item);
                        m.push(seal);
                    }
                }
            }
            4 => {
                now += p.byte() as u64 * 10;
                let age = p.byte();
                let max_age = (age != 0).then_some(age as u64 * 20);
                if p.byte().is_multiple_of(8) {
                    budget = 1 + p.byte() as usize * 2;
                }
                let filling = consumer.filling();
                let spare_out = consumer.has_sealed();
                let polled = consumer.poll(now, max_age, budget);
                m.lost(polled.lost);
                if filling > 0 && max_age == Some(0) && !spare_out {
                    assert_eq!(
                        polled.sealed,
                        Some(Cause::Age),
                        "max age 0 seals at every poll"
                    );
                }
                if polled.sealed.is_none() {
                    assert_eq!(consumer.filling(), filling);
                }
            }
            5 | 6 => {
                let room = if p.byte().is_multiple_of(4) {
                    p.byte() as usize % (budget + 1)
                } else {
                    budget.min(buf.len())
                };
                let sealed = consumer.sealed_items();
                match consumer.encode_into(&mut scratch, &mut buf[..room]) {
                    Ok(Some(enc)) => {
                        assert!(enc.len <= room, "fits the room");
                        assert!(enc.items <= sealed);
                        m.encoded(enc, &buf[..enc.len]);
                    }
                    Ok(None) => assert_eq!(sealed, 0),
                    Err(EncodeError::NoRoom) => assert_eq!(consumer.sealed_items(), sealed),
                    Err(EncodeError::Write(e)) => panic!("write: {e:?}"),
                }
            }
            7 => {
                let k = p.byte() as u16 % 3;
                let done = outbox
                    .push_from(k, &mut consumer, &mut scratch)
                    .expect("outbox");
                let new: Vec<_> = outbox
                    .iter()
                    .skip(outbox.len() - done.batches)
                    .map(|e| (e.k, e.items as usize, e.bytes.to_vec()))
                    .collect();
                assert_eq!(new.iter().map(|e| e.1).sum::<usize>(), done.items);
                m.consumer_lost += done.lost;
                for (ek, items, bytes) in new {
                    assert_eq!(ek, k);
                    m.receive(items, &bytes, false);
                }
                if done.lost > 0 {
                    m.inexact = true;
                }
                assert!(outbox.used_bytes() <= OUTBOX);
            }
            8 => {
                // the consumer takes some entries out, every entry decodes
                for e in outbox.iter() {
                    let got = SeriesOwned::<Item>::from_ww_bytes_owned(e.bytes).expect("entry");
                    assert_eq!(got.len(), e.items as usize);
                }
                match p.byte() % 3 {
                    0 => {
                        outbox.pop_oldest();
                    }
                    1 => {
                        outbox.remove_first_of(p.byte() as u16 % 3);
                    }
                    _ => {}
                }
            }
            9 => {
                if p.byte().is_multiple_of(2) {
                    m.push(producer.flush());
                } else {
                    consumer.flush();
                }
            }
            _ => {
                let (items, lost_before) = consumer.discard_sealed();
                m.lost(lost_before + items as u64);
            }
        }
    }
    // drain: everything left goes out
    loop {
        m.push(producer.flush());
        let polled = consumer.poll(now, None, budget);
        m.lost(polled.lost);
        match consumer.encode_into(&mut scratch, &mut buf) {
            Ok(Some(enc)) => m.encoded(enc, &buf[..enc.len]),
            Ok(None) if consumer.filling() == 0 && polled.lost == 0 => break,
            Ok(None) => {}
            Err(e) => panic!("drain: {e:?}"),
        }
    }
    m.finish(consumer.stats());
}

/// A producer on another thread and the consumer on this one, through the two halves: the producer pushes `count`
/// items (and flushes now and then), the consumer polls with a small age and encodes into rooms of every size, with
/// no lock between them. Every batch decodes to the items pushed, in order, right after the previous one apart from
/// what was reported lost before it; at the end every item was encoded or lost once. `seed` varies the signal, the
/// budget and the codec policy.
pub fn threaded(count: u32, seed: u8) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let policy = policy(seed);
    // small budgets (budget seals, splits) and room for whole batches, a producer from flat out to paced
    let budget = if seed.is_multiple_of(2) {
        24 + seed as usize * 3
    } else {
        400
    };
    let spins = [0, 10, 40, 400][seed as usize / 2 % 4] / if cfg!(miri) { 10 } else { 1 };
    let max_age = [20, 300, 3000][seed as usize % 3];
    let (mut value, mut flag, mut opt) = (0i32, false, None);
    let pushed = items(0, count as usize, seed, &mut value, &mut flag, &mut opt);
    let mut b = Batcher::<Item, N>::with_budget(policy, budget);
    let mut scratch = Scratch::<{ scratch_bytes::<Item>(N) }>::new();
    let mut m = Model::new();
    m.pushed = pushed.clone();
    let mut buf = [0u8; 512];
    let finished = AtomicBool::new(false);
    let (mut producer, mut consumer) = b.split();
    let seal_lost = std::thread::scope(|s| {
        let producer = s.spawn(|| {
            let mut lost = 0u64;
            for (i, item) in pushed.iter().enumerate() {
                if let Seal::Lost { items, .. } = producer.push(item) {
                    lost += items as u64;
                }
                if i % 53 == 52
                    && let Seal::Lost { items, .. } = producer.flush()
                {
                    lost += items as u64;
                }
                for _ in 0..spins {
                    core::hint::spin_loop();
                }
                if i % 7 == 0 {
                    std::thread::yield_now();
                }
            }
            if let Seal::Lost { items, .. } = producer.flush() {
                lost += items as u64;
            }
            finished.store(true, Ordering::Release);
            lost
        });
        let mut now = 0;
        let mut room = 0usize;
        loop {
            let done = finished.load(Ordering::Acquire);
            now += 7;
            let polled = consumer.poll(now, Some(max_age), budget);
            m.lost(polled.lost);
            let mut any = polled.lost > 0;
            loop {
                room = (room + 13) % (budget + 1);
                match consumer.encode_into(&mut scratch, &mut buf[..room.max(budget / 2)]) {
                    Ok(Some(enc)) => {
                        m.encoded(enc, &buf[..enc.len]);
                        any = true;
                    }
                    Ok(None) => break,
                    Err(EncodeError::NoRoom) => {}
                    Err(e) => panic!("{e:?}"),
                }
            }
            if done && !any && consumer.filling() == 0 && !consumer.has_sealed() {
                break;
            }
        }
        producer.join().expect("producer")
    });
    m.seal_lost = seal_lost;
    m.finish(consumer.stats());
}
