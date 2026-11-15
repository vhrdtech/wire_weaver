use super::*;
use crate::chunk::{ColumnSpec, chunk_len, write_chunk};
use crate::times::{EncodedTimes, EventTimes};
use crate::{Block, ChunkHead};
use std::vec::Vec;

/// xorshift64*, fixed seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// An events chunk block with starts, ends, flags and these payloads.
fn block(payloads: &[Vec<u8>]) -> Vec<u8> {
    let n = payloads.len();
    let starts: Vec<i64> = (0..n as i64).map(|i| 1000 + i * 7 + i % 3).collect();
    let ends: Vec<i64> = starts
        .iter()
        .enumerate()
        .map(|(i, s)| s + (i % 5) as i64)
        .collect();
    let flags: Vec<u32> = (0..n as u32).map(|i| i % 4).collect();
    let times = EncodedTimes::encode(&starts, Some(&ends), Some(&flags)).unwrap();
    let p = EncodedPayloads::encode(payloads.iter().map(Vec::as_slice)).unwrap();
    write(
        &times.head.chunk_head(7, 2, 500),
        times.columns().chain(p.columns()),
    )
}

fn write<'a>(head: &ChunkHead, cols: impl Iterator<Item = (ColumnSpec, &'a [u8])>) -> Vec<u8> {
    let cols: Vec<_> = cols.collect();
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = std::vec![0u8; chunk_len(&specs, 0)];
    let len = write_chunk(&mut buf, 1, head, &cols, 0).unwrap();
    buf.truncate(len);
    buf
}

/// Payload workloads: empty, one length, small varied, compressible text, noise, crossing group edges.
fn workloads() -> Vec<(&'static str, Vec<Vec<u8>>)> {
    let mut r = Rng(5);
    let mut noise = |n: usize, max: u64| -> Vec<Vec<u8>> {
        (0..n)
            .map(|_| (0..r.below(max)).map(|_| r.next() as u8).collect())
            .collect()
    };
    let text: Vec<Vec<u8>> = (0..300)
        .map(|i| std::format!("frame {} ok; crc ok; len {}", i % 7, i % 13).into_bytes())
        .collect();
    std::vec![
        ("one empty", std::vec![Vec::new()]),
        ("all empty", std::vec![Vec::new(); 300]),
        (
            "one length",
            (0..257).map(|i| std::vec![i as u8; 12]).collect()
        ),
        ("noise", noise(129, 40)),
        ("noise long", noise(1000, 300)),
        ("text", text),
        (
            "one big",
            std::vec![(0..70_000u32).map(|i| (i * 7 % 251) as u8).collect()]
        ),
    ]
}

/// The iterator gives every item as `get` and `is_suspect` do.
fn iter_matches(p: &Payloads<'_>) {
    let items: Vec<Item<'_>> = p.iter().collect();
    assert_eq!(items.len(), p.len());
    assert_eq!(p.iter().len(), p.len());
    for (i, it) in items.iter().enumerate() {
        assert_eq!(Some(it.bytes), p.get(i), "item {i}");
        assert_eq!(it.suspect, p.is_suspect(i), "item {i}");
    }
}

fn check(p: &Payloads<'_>, want: &[Vec<u8>]) {
    iter_matches(p);
    assert_eq!(p.len(), want.len());
    for (i, w) in want.iter().enumerate() {
        assert_eq!(p.get(i), Some(w.as_slice()), "item {i}");
        assert!(!p.is_suspect(i), "item {i}");
    }
    assert_eq!(p.get(want.len()), None);
    assert_eq!(p.suspect_items().count(), 0);
}

#[test]
fn round_trips_raw_and_lz4() {
    for (name, w) in workloads() {
        let b = block(&w);
        let block = Block::read(&b).unwrap();
        let c = block.chunk().unwrap();
        let p = Payloads::read(&c).unwrap().expect("payload columns");
        check(&p, &w);
        check(&Payloads::salvage(&c).unwrap().unwrap(), &w);
        let codec = c.find(ColumnKind::Payloads, 0).unwrap().codec;
        match name {
            "text" | "one length" | "one big" => assert_eq!(codec, Codec::LZ4, "{name}"),
            _ => assert_eq!(codec, Codec::RAW, "{name}"),
        }
        assert_eq!(EventTimes::read(&c).unwrap().len(), w.len());
    }
}

#[test]
fn one_length_costs_records_only() {
    let w: Vec<Vec<u8>> = (0..256).map(|_| std::vec![1u8; 9]).collect();
    let e = EncodedPayloads::encode(w.iter().map(Vec::as_slice)).unwrap();
    assert_eq!(e.columns()[0].1.len(), 2 * RECORD_LEN);
}

#[test]
fn chunk_without_payloads_and_bad_columns() {
    let times = EncodedTimes::encode(&[1, 2, 3], None, None).unwrap();
    let head = times.head.chunk_head(7, 2, 0);
    let b = write(&head, times.columns());
    let blk = Block::read(&b).unwrap();
    assert!(Payloads::read(&blk.chunk().unwrap()).unwrap().is_none());
    assert!(Payloads::salvage(&blk.chunk().unwrap()).unwrap().is_none());

    let p = EncodedPayloads::encode([&b"ab"[..], b"c", b""]).unwrap();
    let [l, d] = p.columns();
    let read = |cols: Vec<(ColumnSpec, &[u8])>| {
        let b = write(&head, times.columns().chain(cols));
        let blk = Block::read(&b).unwrap();
        Payloads::read(&blk.chunk().unwrap()).map(|p| p.map(|p| p.len()))
    };
    assert_eq!(read(std::vec![l, d]), Ok(Some(3)));
    assert_eq!(
        read(std::vec![l]),
        Err(PayloadsError::Missing(ColumnKind::Payloads))
    );
    assert_eq!(
        read(std::vec![d]),
        Err(PayloadsError::Missing(ColumnKind::PayloadLengths))
    );
    assert_eq!(
        read(std::vec![l, d, d]),
        Err(PayloadsError::Unexpected(ColumnKind::Payloads))
    );
    let raw_lengths = (
        ColumnSpec {
            codec: Codec::RAW,
            ..l.0
        },
        l.1,
    );
    assert_eq!(
        read(std::vec![raw_lengths, d]),
        Err(PayloadsError::UnknownCodec(
            ColumnKind::PayloadLengths,
            Codec::RAW
        ))
    );
    // lengths that add up to more than the payload bytes
    let short = (ColumnSpec { len: 2, ..d.0 }, &d.1[..2]);
    assert!(matches!(
        read(std::vec![l, short]),
        Err(PayloadsError::Corrupt { .. })
    ));
    assert_eq!(
        EncodedPayloads::encode(core::iter::empty()),
        Err(EncodeError::Empty)
    );
}

/// The payload columns of `b`'s chunk: (lengths offset, payloads offset) in the block.
fn column_offsets(b: &[u8]) -> (usize, usize, usize) {
    let blk = Block::read(b).unwrap();
    let c = blk.chunk().unwrap();
    let at = c.payload_offset();
    let l = c.find(ColumnKind::PayloadLengths, 0).unwrap();
    let p = c.find(ColumnKind::Payloads, 0).unwrap();
    (at + l.offset, at + p.offset, p.bytes.len())
}

/// A chunk whose payload columns are rewritten by `edit` (CRCs made good again): read refuses it, salvage marks the
/// groups `bad` and reads every other item.
fn rewritten(w: &[Vec<u8>], edit: impl Fn(&mut [u8]), bad: &[usize]) {
    let b = block(w);
    let blk = Block::read(&b).unwrap();
    let c = blk.chunk().unwrap();
    let times = EncodedTimes::encode(
        &EventTimes::read(&c)
            .unwrap()
            .starts()
            .iter()
            .collect::<Vec<_>>(),
        None,
        None,
    )
    .unwrap();
    let l = c.find(ColumnKind::PayloadLengths, 0).unwrap();
    let p = c.find(ColumnKind::Payloads, 0).unwrap();
    let mut lengths = l.bytes.to_vec();
    edit(&mut lengths);
    let spec = |c: &Column<'_>| ColumnSpec {
        kind: c.kind,
        codec: c.codec,
        param: 0,
        len: c.bytes.len(),
    };
    let b = write(
        &times.head.chunk_head(7, 2, 0),
        times
            .columns()
            .chain([(spec(&l), &lengths[..]), (spec(&p), p.bytes)]),
    );
    let blk = Block::read(&b).unwrap();
    let c = blk.chunk().unwrap();
    assert!(matches!(
        Payloads::read(&c),
        Err(PayloadsError::Corrupt { .. })
    ));
    let s = Payloads::salvage(&c).unwrap().unwrap();
    iter_matches(&s);
    for (i, want) in w.iter().enumerate() {
        let g = i / GROUP;
        assert_eq!(s.is_suspect(i), bad.contains(&g), "item {i}");
        if !bad.contains(&g) {
            assert_eq!(s.get(i), Some(want.as_slice()), "item {i}");
        } else {
            assert_eq!(s.get(i), Some(&[][..]), "item {i}");
        }
    }
}

#[test]
fn salvage_blames_the_damaged_group() {
    let mut r = Rng(9);
    let w: Vec<Vec<u8>> = (0..600)
        .map(|_| (0..r.below(30)).map(|_| r.next() as u8).collect())
        .collect();
    let rec = |g: usize, at: usize| g * RECORD_LEN + at;
    // a base: both links of group 2 break, group 2 alone is blamed
    rewritten(&w, |l| l[rec(2, 0)] ^= 1, &[2]);
    // a minimum, a width: the link after the group breaks
    rewritten(&w, |l| l[rec(1, 4)] ^= 4, &[1]);
    rewritten(&w, |l| l[rec(3, 8)] ^= 1, &[3]);
    // an offset
    rewritten(&w, |l| l[rec(4, 9)] ^= 2, &[4]);
    // the last group's base, and a width over 32
    rewritten(&w, |l| l[rec(4, 1)] ^= 1, &[4]);
    rewritten(&w, |l| l[rec(0, 8)] = 33, &[0]);
    // a packed length
    rewritten(&w, |l| l[5 * RECORD_LEN + 10] ^= 1, &[0]);
}

/// Every flipped bit of a small chunk's payload: salvage never panics, an item not marked suspect reads as written.
#[test]
fn every_flipped_bit_keeps_good_items() {
    let mut r = Rng(3);
    let noise: Vec<Vec<u8>> = (0..140)
        .map(|_| (0..r.below(6)).map(|_| r.next() as u8).collect())
        .collect();
    let text: Vec<Vec<u8>> = (0..140)
        .map(|i| std::vec![b'a' + (i % 3) as u8; 7])
        .collect();
    for w in [noise, text] {
        let b = block(&w);
        let (from, _, _) = column_offsets(&b);
        let mut marked = 0;
        for bit in from * 8..(b.len() - 4) * 8 {
            let mut d = b.clone();
            d[bit / 8] ^= 1 << (bit % 8);
            let blk = Block::salvage(&d).unwrap();
            let c = blk.chunk().unwrap();
            let s = Payloads::salvage(&c).unwrap().unwrap();
            iter_matches(&s);
            iter_matches(&s);
            assert_eq!(s.len(), w.len());
            marked += s.suspect_items().count();
            for (i, want) in w.iter().enumerate() {
                if !s.is_suspect(i) {
                    assert_eq!(s.get(i), Some(want.as_slice()), "bit {bit} item {i}");
                }
            }
        }
        assert!(marked > 0, "flipped bits mark items");
    }
}

/// A flipped bit in a raw payloads column of several segments costs the items in that segment only.
#[test]
fn damaged_segment_costs_its_items() {
    let w: Vec<Vec<u8>> = (0..400u32)
        .map(|i| {
            let mut r = Rng(u64::from(i) + 1);
            (0..1000).map(|_| r.next() as u8).collect()
        })
        .collect();
    let b = block(&w);
    let (_, at, len) = column_offsets(&b);
    assert_eq!(len, 400_000);
    let mut d = b.clone();
    // a byte inside item 200's payload
    d[at + 200_500] ^= 0x10;
    let blk = Block::salvage(&d).unwrap();
    let c = blk.chunk().unwrap();
    assert!(
        Payloads::read(&c).is_ok(),
        "read doesn't check payload bytes"
    );
    let s = Payloads::salvage(&c).unwrap().unwrap();
    iter_matches(&s);
    let suspect: Vec<_> = s.suspect_items().collect();
    assert_eq!(suspect.len(), 1);
    let r = &suspect[0];
    assert!(r.contains(&200) && r.len() <= 67, "{r:?}");
    for (i, want) in w.iter().enumerate() {
        if !r.contains(&i) {
            assert_eq!(s.get(i), Some(want.as_slice()));
        } else if i != 200 {
            assert_eq!(s.get(i), Some(want.as_slice()), "read as stored");
        }
    }
}

#[test]
fn damaged_lz4_marks_every_item() {
    let w: Vec<Vec<u8>> = (0..50)
        .map(|i| std::format!("item {}", i % 4).into_bytes())
        .collect();
    let b = block(&w);
    let (_, at, len) = column_offsets(&b);
    let mut d = b.clone();
    d[at + len / 2] ^= 0x40;
    let blk = Block::salvage(&d).unwrap();
    let s = Payloads::salvage(&blk.chunk().unwrap()).unwrap().unwrap();
    iter_matches(&s);
    let mut runs = s.suspect_items();
    assert_eq!((runs.next(), runs.next()), (Some(0..50), None));
}

/// The same payloads give the same bytes, and they are golden/v2-payloads.hex's (`WW_TAPE_GOLDEN=update` rewrites
/// it): one chunk block per line, in hex.
#[test]
fn payloads_deterministic_and_golden() {
    let blocks: Vec<Vec<u8>> = workloads()
        .into_iter()
        .filter(|(name, _)| *name != "one big" && *name != "noise long")
        .map(|(_, w)| block(&w))
        .collect();
    let again: Vec<Vec<u8>> = workloads()
        .into_iter()
        .filter(|(name, _)| *name != "one big" && *name != "noise long")
        .map(|(_, w)| block(&w))
        .collect();
    assert_eq!(blocks, again, "two encodes of the same payloads");
    let mut hex = String::new();
    for b in &blocks {
        for x in b {
            hex.push_str(&std::format!("{x:02x}"));
        }
        hex.push('\n');
    }
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-payloads.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-payloads.hex: run with WW_TAPE_GOLDEN=update");
    assert_eq!(
        hex, want,
        "the format changed: bytes differ from golden/v2-payloads.hex"
    );
}
