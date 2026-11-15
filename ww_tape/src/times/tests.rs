use super::*;
use crate::Block;
use crate::chunk::{ColumnSpec, chunk_len, write_chunk};
use crate::tests::id;
use std::vec::Vec;

/// xorshift64*, fixed seeds.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A chunk block of these times, with a payload column after them.
pub(crate) fn block(starts: &[i64], ends: Option<&[i64]>, flags: Option<&[u32]>) -> Vec<u8> {
    let enc = EncodedTimes::encode(starts, ends, flags).expect("valid times");
    let payload = [0xa5u8; 5];
    let mut cols: Vec<(ColumnSpec, &[u8])> = enc.columns().collect();
    cols.push((
        ColumnSpec {
            kind: ColumnKind::Payloads,
            codec: Codec::RAW,
            param: 0,
            len: payload.len(),
        },
        &payload,
    ));
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = std::vec![0u8; chunk_len(&specs, 0)];
    let head = enc.head.chunk_head(9, 1, 1000);
    let len = write_chunk(&mut buf, id(1), &head, &cols, 0).expect("a valid chunk");
    buf.truncate(len);
    buf
}

/// Every read of `t` against the plain values.
fn check(t: &EventTimes<'_>, starts: &[i64], ends: &[i64], flags: &[u32], r: &mut Rng) {
    let n = starts.len();
    assert_eq!(t.len(), n);
    let s = t.starts();
    for i in 0..n {
        assert_eq!(s.get(i), starts[i], "start {i}");
        assert_eq!(t.end(i), ends[i], "end {i}");
        assert_eq!(t.flag(i), flags[i], "flag {i}");
    }
    assert!(s.iter().eq(starts.iter().copied()), "forward");
    assert!(s.iter().rev().eq(starts.iter().rev().copied()), "backward");
    let (a, b) = (n / 3, n - n / 5);
    assert!(s.range(a..b).eq(starts[a..b].iter().copied()), "range");
    let mut both = s.iter();
    let mut k = 0;
    while let (Some(x), Some(y)) = (both.next(), both.next_back()) {
        assert_eq!((x, y), (starts[k], starts[n - 1 - k]), "both ends {k}");
        k += 1;
    }
    let mut buf = [0i64; GROUP];
    let mut fbuf = [0u32; GROUP];
    for g in 0..s.group_count() {
        let m = s.decode_group(g, &mut buf);
        assert_eq!(&buf[..m], &starts[g * GROUP..g * GROUP + m], "group {g}");
        assert_eq!(s.group_first(g), starts[g * GROUP]);
        let m = t.decode_group_ends(g, &mut buf);
        assert_eq!(
            &buf[..m],
            &ends[g * GROUP..g * GROUP + m],
            "ends of group {g}"
        );
        let or = flags[g * GROUP..g * GROUP + m].iter().fold(0, |a, f| a | f);
        assert_eq!(t.group_flags(g), or, "flags OR of group {g}");
        if let Some(f) = t.flags() {
            f.decode_group(g, &mut fbuf);
            assert_eq!(&fbuf[..m], &flags[g * GROUP..g * GROUP + m]);
        }
        if let Ends::Durations(d) = t.ends() {
            let longest = (g * GROUP..g * GROUP + m)
                .map(|i| ends[i].wrapping_sub(starts[i]) as u64)
                .max()
                .unwrap();
            assert!(d.group_bound(g) >= longest, "duration bound of group {g}");
        }
    }
    let probe = |q: i64| {
        let want = starts.partition_point(|x| *x < q);
        assert_eq!(s.lower_bound(q), want, "lower bound {q}");
        let want = starts.partition_point(|x| *x <= q).checked_sub(1);
        assert_eq!(s.last_at_or_before(q), want, "last at or before {q}");
    };
    for &x in starts.iter().step_by(1 + n / 500) {
        for q in [x.saturating_sub(1), x, x.saturating_add(1)] {
            probe(q);
        }
    }
    let (lo, hi) = (starts[0], starts[n - 1]);
    for _ in 0..500 {
        let span = (hi as i128 - lo as i128 + 3) as u128;
        let q = lo as i128 - 1 + (u128::from(r.next()) % span) as i128;
        probe(q.clamp(i64::MIN as i128, i64::MAX as i128) as i64);
    }
    for q in [i64::MIN, i64::MAX] {
        probe(q);
    }
    assert_eq!(t.suspect_items().count(), 0);
}

fn round_trip(starts: &[i64], ends: Option<&[i64]>, flags: Option<&[u32]>) -> (TimeMode, EndMode) {
    let b = block(starts, ends, flags);
    let block = Block::read(&b).expect("a good block");
    let chunk = block.chunk().expect("a chunk");
    let t = EventTimes::read(&chunk).expect("times read");
    let zeros = std::vec![0u32; starts.len()];
    let mut r = Rng(starts.len() as u64 + 7);
    check(
        &t,
        starts,
        ends.unwrap_or(starts),
        flags.unwrap_or(&zeros),
        &mut r,
    );
    (t.starts().mode(), chunk.head().end_mode)
}

/// Start tick sets: name and ticks.
pub(crate) fn workloads() -> Vec<(&'static str, Vec<i64>)> {
    let mut r = Rng(11);
    let mut w = Vec::new();
    let bursty = {
        let mut t = 1_700_000_000_000_000_000i64;
        (0..4096)
            .map(|_| {
                t += if r.below(32) == 0 {
                    1_000_000 + r.below(2_000_000_000) as i64
                } else {
                    1000 + r.below(19_000) as i64
                };
                t
            })
            .collect()
    };
    w.push(("bursty", bursty));
    w.push((
        "jitter",
        (0..4096i64)
            .map(|i| 5_000_000_000 + i * 1_000_000 + r.below(600) as i64 - 300)
            .collect(),
    ));
    w.push(("exact", (0..4096i64).map(|i| -7 + i * 1_000_000).collect()));
    w.push((
        "exact with gaps",
        (0..4000i64)
            .map(|i| i * 1000 + if i >= 1500 { 5000 } else { 0 })
            .collect(),
    ));
    w.push(("one", std::vec![42]));
    w.push(("two", std::vec![-5, 1000]));
    w.push(("same tick", std::vec![77; 300]));
    w.push(("duplicates", (0..1000i64).map(|i| i / 7 * 13).collect()));
    w.push((
        "wide",
        (0..700i64)
            .map(|i| i * (1 << 40) + (r.next() >> 30) as i64)
            .collect(),
    ));
    let mut extremes: Vec<i64> = (0..300)
        .map(|_| r.next() as i64)
        .chain([i64::MIN, i64::MAX, 0])
        .collect();
    extremes.sort();
    w.push(("extremes", extremes));
    w.push((
        "frames on 1 ns",
        (0..3000i64)
            .map(|i| i * 1_000_000_000 * 1001 / 60_000)
            .collect(),
    ));
    w.push((
        "sparse",
        (0..129i64)
            .map(|i| i * i * 1_000_003 + (i % 3) * 7)
            .collect(),
    ));
    // gaps of 2^63 ticks and more: periods that don't fit an i64
    w.push(("extreme gaps", std::vec![i64::MIN, 0, 1]));
    w.push(("extreme pair", std::vec![i64::MIN, i64::MAX]));
    w.push(("extreme even", std::vec![i64::MIN, -1, i64::MAX - 1]));
    w
}

#[test]
fn starts_read_back_every_way() {
    for (name, s) in workloads() {
        let (mode, end) = round_trip(&s, None, None);
        assert_eq!(end, EndMode::Absent, "{name}");
        let implicit = matches!(name, "exact" | "one" | "two" | "same tick" | "extreme even");
        assert_eq!(mode == TimeMode::Implicit, implicit, "{name}");
    }
}

#[test]
fn end_modes_and_flags_read_back() {
    let mut r = Rng(5);
    for (name, s) in workloads() {
        let n = s.len();
        let flags: Vec<u32> = (0..n)
            .map(|_| {
                if r.below(10) == 0 {
                    r.next() as u32 >> r.below(32)
                } else {
                    0
                }
            })
            .collect();
        // instants given as ends
        assert_eq!(
            round_trip(&s, Some(&s), Some(&flags)).1,
            EndMode::Absent,
            "{name}"
        );
        // back to back
        let mut next: Vec<i64> = s[1..].to_vec();
        next.push(s[n - 1].saturating_add(3));
        assert_eq!(
            round_trip(&s, Some(&next), None).1,
            EndMode::NextStart,
            "{name}"
        );
        // spans of their own
        let spans: Vec<i64> = s
            .iter()
            .map(|x| x.saturating_add(r.below(1_000_000) as i64))
            .collect();
        let want = if n == 1 {
            EndMode::NextStart
        } else {
            EndMode::Durations
        };
        assert_eq!(round_trip(&s, Some(&spans), Some(&flags)).1, want, "{name}");
    }
}

#[test]
fn zero_flags_take_no_column() {
    let s: Vec<i64> = (0..10).map(|i| i * i).collect();
    let enc = EncodedTimes::encode(&s, None, Some(&[0; 10])).unwrap();
    assert_eq!(enc.columns().count(), 1);
}

#[test]
fn encode_refuses_bad_times() {
    assert_eq!(
        EncodedTimes::encode(&[], None, None),
        Err(EncodeError::Empty)
    );
    assert_eq!(
        EncodedTimes::encode(&[1, 3, 2], None, None),
        Err(EncodeError::Unsorted { at: 2 })
    );
    assert_eq!(
        EncodedTimes::encode(&[1, 3], Some(&[2, 2]), None),
        Err(EncodeError::EndBeforeStart { at: 1 })
    );
    assert_eq!(
        EncodedTimes::encode(&[1, 3], Some(&[2]), None),
        Err(EncodeError::Length)
    );
    assert_eq!(
        EncodedTimes::encode(&[1, 3], None, Some(&[0])),
        Err(EncodeError::Length)
    );
}

#[test]
fn the_slope_narrows_periodic_groups() {
    let s: Vec<i64> = (0..4096i64)
        .map(|i| i * 1_000_000 + (i * 7919) % 601 - 300)
        .collect();
    let enc = EncodedTimes::encode(&s, None, None).unwrap();
    let (_, bytes) = enc.columns().next().unwrap();
    // 25 B per group plus 10 bits per item
    assert_eq!(bytes.len(), 32 * 25 + 4096 * 10 / 8);
}

/// The time columns' byte ranges in a block: payload start and the end of the last time column.
fn time_bytes(b: &[u8]) -> core::ops::Range<usize> {
    let block = Block::read(b).unwrap();
    let c = block.chunk().unwrap();
    let p = c.payload_offset();
    let end = c
        .columns()
        .filter(|c| c.kind != ColumnKind::Payloads)
        .map(|c| c.offset + c.bytes.len())
        .max()
        .unwrap();
    p..p + end
}

/// Read everything there is to read, panicking on nothing.
fn read_all(t: &EventTimes<'_>) {
    let s = t.starts();
    let mut buf = [0i64; GROUP];
    for i in 0..t.len() {
        let _ = (t.start(i), t.end(i), t.flag(i), t.is_suspect(i));
    }
    for g in 0..s.group_count() {
        t.decode_group_ends(g, &mut buf);
        let _ = t.group_flags(g);
        if let Ends::Durations(d) = t.ends() {
            let _ = d.group_bound(g);
        }
    }
    let _ = s.iter().rev().count();
    for q in [i64::MIN, -1, 0, 1 << 40, i64::MAX] {
        let _ = (s.lower_bound(q), s.last_at_or_before(q));
    }
    let _ = t.suspect_items().count();
}

#[test]
fn every_flipped_byte_reads_without_panic() {
    let mut r = Rng(3);
    // every fourth byte, a different quarter per workload; the fuzz target tape-times goes further
    for (k, (_, mut s)) in workloads()
        .into_iter()
        .filter(|w| w.1.len() >= 129)
        .enumerate()
    {
        s.truncate(300);
        let spans: Vec<i64> = s
            .iter()
            .map(|x| x.saturating_add(r.below(1000) as i64))
            .collect();
        let flags: Vec<u32> = (0..s.len()).map(|i| (i % 5 == 0) as u32).collect();
        let good = block(&s, Some(&spans), Some(&flags));
        for at in time_bytes(&good).skip(k % 4).step_by(4) {
            let mut b = good.clone();
            b[at] ^= 1 << (r.below(8) as u32);
            let block = Block::salvage(&b).expect("a payload flip leaves the head good");
            let chunk = block.chunk().unwrap();
            if let Ok(t) = EventTimes::read(&chunk) {
                read_all(&t);
            }
            if let Ok(t) = EventTimes::salvage(&chunk) {
                read_all(&t);
            }
        }
    }
}

#[test]
fn salvage_marks_only_the_bad_group() {
    let s: Vec<i64> = workloads().swap_remove(0).1;
    let good = block(&s, None, None);
    let p = time_bytes(&good).start;
    // group 5's width: byte 20 of its record
    let at = p + 5 * 25 + 20;
    for width in [65u8, 0, 63] {
        let mut b = good.clone();
        b[at] = width;
        let block = Block::salvage(&b).unwrap();
        assert!(!block.is_intact());
        let chunk = block.chunk().unwrap();
        assert!(EventTimes::read(&chunk).is_err(), "width {width}");
        let t = EventTimes::salvage(&chunk).unwrap();
        let bad: Vec<_> = t.suspect_items().collect();
        assert_eq!(bad, std::vec![(5 * GROUP..6 * GROUP)], "width {width}");
        for i in (0..s.len()).filter(|i| i / GROUP != 5) {
            assert_eq!(t.start(i), s[i], "item {i}");
        }
        for i in 5 * GROUP..6 * GROUP {
            assert_eq!(t.start(i), s[5 * GROUP], "stand-in {i}");
        }
        assert_eq!(
            t.starts().lower_bound(s[1000]),
            s.partition_point(|x| *x < s[1000])
        );
    }
    // a flipped residual bit inside a group damages one item and no group: bit 0 of group 0's item 64
    let mut b = good.clone();
    let w = usize::from(good[p + 20]);
    b[p + 32 * 25 + 8 * w] ^= 1;
    let block = Block::salvage(&b).unwrap();
    let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
    assert_eq!(t.suspect_items().count(), 0);
    let wrong: Vec<usize> = (0..s.len()).filter(|&i| t.start(i) != s[i]).collect();
    assert_eq!(wrong, std::vec![64]);
}

#[test]
fn salvage_checks_what_a_group_record_says() {
    let s: Vec<i64> = workloads().swap_remove(0).1;
    let good = block(&s, None, None);
    let p = time_bytes(&good).start;
    let rec = |g: usize, at: usize| p + g * 25 + at;
    let w = usize::from(good[rec(5, 20)]);
    let res5 = p
        + 32 * 25
        + usize::from_le_bytes([
            good[rec(5, 21)],
            good[rec(5, 22)],
            good[rec(5, 23)],
            0,
            0,
            0,
            0,
            0,
        ]);
    // (byte, bit, the groups that go suspect): a low bit of the residual minimum or of item 0's residual breaks
    // item 0 == first tick; a first tick or slope that breaks the order with a neighbour marks both, either may be
    // the damaged one
    let cases = [
        (rec(5, 16), 0x01, 5..6),
        (res5, 0x01, 5..6),
        (rec(5, 7), 0x01, 5..6),
        (rec(5, 3), 0x80, 4..6),
        (rec(5, 14), 0x01, 5..7),
    ];
    assert!(w > 0, "group 5 has residuals");
    for (at, bit, groups) in cases {
        let mut b = good.clone();
        b[at] ^= bit;
        let block = Block::salvage(&b).unwrap();
        let chunk = block.chunk().unwrap();
        assert!(EventTimes::read(&chunk).is_err(), "byte {at}");
        let t = EventTimes::salvage(&chunk).unwrap();
        let bad: Vec<_> = t.suspect_items().collect();
        let want: Vec<_> = groups.map(|g| g * GROUP..(g + 1) * GROUP).collect();
        assert_eq!(bad, want, "byte {at}");
        for i in (0..s.len()).filter(|i| !t.is_suspect(*i)) {
            assert_eq!(t.start(i), s[i], "byte {at} item {i}");
        }
        let starts = t.starts();
        assert!(starts.iter().all(|x| (s[0]..=s[s.len() - 1]).contains(&x)));
    }
    // a residual pushed past the chunk's last tick reads clamped: the top bit of group 31's item 64
    let mut b = good.clone();
    let w31 = usize::from(good[rec(31, 20)]);
    let res31 = usize::from_le_bytes([
        good[rec(31, 21)],
        good[rec(31, 22)],
        good[rec(31, 23)],
        0,
        0,
        0,
        0,
        0,
    ]);
    let bit = 64 * w31 + w31 - 1;
    b[p + 32 * 25 + res31 + bit / 8] ^= 1 << (bit % 8);
    let block = Block::salvage(&b).unwrap();
    let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
    let i = 31 * GROUP + 64;
    assert_ne!(t.start(i), s[i]);
    assert!(t.start(i) <= s[s.len() - 1]);
}

/// The block offset of the first byte of the `kind` column.
fn column_start(b: &[u8], kind: ColumnKind) -> usize {
    let block = Block::read(b).unwrap();
    let c = block.chunk().unwrap();
    c.payload_offset() + c.columns().find(|c| c.kind == kind).unwrap().offset
}

/// Per packed column: kind, record length, where the width (a u8, or a u32 OR for flags) and the offset sit.
const RECORDS: [(ColumnKind, usize, usize, usize); 3] = [
    (ColumnKind::Starts, 25, 20, 21),
    (ColumnKind::Ends, 13, 8, 9),
    (ColumnKind::Flags, 8, 0, 4),
];

#[test]
fn salvage_bounds_every_group_by_the_column() {
    // offsets that agree with each other but point far past the column
    let s: Vec<i64> = (0..129i64).map(|i| i * i).collect();
    let spans: Vec<i64> = s
        .iter()
        .enumerate()
        .map(|(i, x)| x + (i % 7) as i64)
        .collect();
    let flags: Vec<u32> = (0..129).map(|i| (i % 3) as u32).collect();
    let good = block(&s, Some(&spans), Some(&flags));
    for (kind, len, width, offset) in RECORDS {
        let mut b = good.clone();
        let c = column_start(&b, kind);
        if kind == ColumnKind::Flags {
            b[c + width..c + width + 4].copy_from_slice(&1u32.to_le_bytes());
        } else {
            b[c + width] = 1;
        }
        b[c + offset..c + offset + 4].copy_from_slice(&0x0100_0000u32.to_le_bytes());
        b[c + len + offset..c + len + offset + 4].copy_from_slice(&0x0100_0010u32.to_le_bytes());
        let block = Block::salvage(&b).unwrap();
        let chunk = block.chunk().unwrap();
        assert!(EventTimes::read(&chunk).is_err(), "{kind:?}");
        let t = EventTimes::salvage(&chunk).unwrap();
        read_all(&t);
        assert!(t.is_suspect_group(0), "{kind:?}");
    }
}

#[test]
fn a_damaged_record_marks_its_group_only() {
    let s: Vec<i64> = workloads().swap_remove(0).1;
    let mut r = Rng(5);
    let spans: Vec<i64> = s.iter().map(|x| x + r.below(5000) as i64).collect();
    let flags: Vec<u32> = (0..s.len()).map(|i| (i % 3) as u32).collect();
    let good = block(&s, Some(&spans), Some(&flags));
    for (kind, len, width, offset) in RECORDS {
        // a flipped offset breaks the links on both sides of group 5; a flipped width only the one after it
        for (at, bit) in [(offset, 0x04), (offset + 1, 0x01), (width, 0x04)] {
            let mut b = good.clone();
            let c = column_start(&b, kind);
            b[c + 5 * len + at] ^= bit;
            let block = Block::salvage(&b).unwrap();
            let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
            let bad: Vec<_> = t.suspect_items().collect();
            assert_eq!(bad, std::vec![(5 * GROUP..6 * GROUP)], "{kind:?} byte {at}");
            for i in (0..s.len()).filter(|i| i / GROUP != 5) {
                let got = (t.start(i), t.end(i), t.flag(i));
                assert_eq!(
                    got,
                    (s[i], spans[i], flags[i]),
                    "{kind:?} byte {at} item {i}"
                );
            }
        }
    }
}

#[test]
fn salvage_keeps_durations_and_flags_in_bounds() {
    let s: Vec<i64> = (0..300i64).map(|i| i * 1000 + (i * i) % 17).collect();
    let spans: Vec<i64> = s
        .iter()
        .enumerate()
        .map(|(i, x)| x + (i % 100) as i64)
        .collect();
    let flags: Vec<u32> = (0..300).map(|i| (i % 4 == 0) as u32 * 2).collect();
    let good = block(&s, Some(&spans), Some(&flags));
    let latest = *spans.iter().max().unwrap();
    // bit 18 of group 2's duration minimum: in range, but its items end past the latest end; bit 0 of item 64's
    // flags in group 1 (OR 2, width 2)
    let mut b = good.clone();
    let e = column_start(&good, ColumnKind::Ends);
    b[e + 2 * 13 + 2] ^= 0x04;
    let f = column_start(&good, ColumnKind::Flags);
    let f_off = usize::from_le_bytes([good[f + 8 + 4], good[f + 8 + 5], 0, 0, 0, 0, 0, 0]);
    b[f + 3 * 8 + f_off + 64 * 2 / 8] ^= 1;
    let block = Block::salvage(&b).unwrap();
    let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
    assert_eq!(t.suspect_items().count(), 0);
    let i = GROUP + 64;
    assert_eq!(
        t.flag(i),
        flags[i] & 2,
        "a bit outside the group's OR is dropped"
    );
    let mut out = [0u32; GROUP];
    t.flags().unwrap().decode_group(1, &mut out);
    assert!(out.iter().all(|f| f & !2 == 0));
    assert_eq!(t.end(299), latest, "clamped to the latest end");
    let mut ends = [0i64; GROUP];
    let n = t.decode_group_ends(2, &mut ends);
    for (j, e) in ends[..n].iter().enumerate() {
        assert_eq!(*e, t.end(2 * GROUP + j));
        assert!(*e <= latest);
    }
}

#[test]
fn an_intact_column_is_read_strictly_in_salvage() {
    let s: Vec<i64> = workloads().swap_remove(0).1;
    let flags: Vec<u32> = (0..s.len()).map(|i| (i % 3) as u32).collect();
    let good = block(&s, None, Some(&flags));
    let mut b = good.clone();
    // damage the payload column after the time columns: the times read as they are
    let end = time_bytes(&good).end;
    b[end + 1] ^= 0xff;
    let block = Block::salvage(&b).unwrap();
    let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
    assert_eq!(t.suspect_items().count(), 0);
    assert_eq!(t.start(4000), s[4000]);
}

#[test]
fn a_salvaged_chunk_has_no_suspect_past_its_items() {
    let s: Vec<i64> = (0..300i64).map(|i| i * i).collect();
    let spans: Vec<i64> = s.iter().map(|x| x + 3).collect();
    let flags: Vec<u32> = (0..300).map(|i| (i % 3) as u32).collect();
    let good = block(&s, Some(&spans), Some(&flags));
    let mut b = good.clone();
    // the residual minimum of group 1: the starts column is read by salvage
    b[column_start(&good, ColumnKind::Starts) + 25 + 16] ^= 0x01;
    let block = Block::salvage(&b).unwrap();
    let t = EventTimes::salvage(&block.chunk().unwrap()).unwrap();
    assert_eq!(t.suspect_items().collect::<Vec<_>>(), [GROUP..2 * GROUP]);
    for i in [s.len(), 3 * GROUP, usize::MAX] {
        assert!(!t.is_suspect(i), "item {i}");
    }
    for g in [3, 1000, usize::MAX] {
        assert!(!t.is_suspect_group(g), "group {g}");
        assert!(!t.starts().is_suspect_group(g), "group {g}");
        assert!(!t.flags().unwrap().is_suspect_group(g), "group {g}");
        let Ends::Durations(d) = t.ends() else {
            panic!("durations")
        };
        assert!(!d.is_suspect_group(g), "group {g}");
    }
}

#[test]
fn read_refuses_wrong_shapes() {
    let s = [1i64, 5, 6];
    let enc = EncodedTimes::encode(&s, None, None).unwrap();
    let cols: Vec<(ColumnSpec, &[u8])> = enc.columns().collect();
    let write = |head: crate::ChunkHead, cols: &[(ColumnSpec, &[u8])]| {
        let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
        let mut buf = std::vec![0u8; chunk_len(&specs, 0)];
        let len = write_chunk(&mut buf, id(1), &head, cols, 0).unwrap();
        buf.truncate(len);
        buf
    };
    let head = enc.head.chunk_head(1, 1, 0);
    let read = |b: &[u8]| {
        let block = Block::read(b).unwrap();
        EventTimes::read(&block.chunk().unwrap()).map(|_| ())
    };
    assert_eq!(read(&write(head, &cols)), Ok(()));
    let mut h = head;
    h.time_mode = 7;
    assert_eq!(read(&write(h, &cols)), Err(TimesError::UnknownMode(7)));
    h = head;
    h.end_mode = EndMode::Durations;
    h.latest_end = 9;
    assert_eq!(
        read(&write(h, &cols)),
        Err(TimesError::Missing(ColumnKind::Ends))
    );
    let mut raw = cols.clone();
    raw[0].0.codec = Codec::RAW;
    assert_eq!(
        read(&write(head, &raw)),
        Err(TimesError::UnknownCodec(ColumnKind::Starts, Codec::RAW))
    );
    h = head;
    h.last_tick = 7;
    h.latest_end = 7;
    assert!(matches!(
        read(&write(h, &cols)),
        Err(TimesError::Corrupt { .. })
    ));
}

/// The golden chunks: each workload cut to 300 items, with spans and flags on some.
fn golden_blocks() -> Vec<Vec<u8>> {
    let mut r = Rng(17);
    let mut out = Vec::new();
    for (k, (_, mut s)) in workloads().into_iter().enumerate() {
        s.truncate(300);
        let n = s.len();
        out.push(block(&s, None, None));
        if k % 3 == 0 {
            let spans: Vec<i64> = s
                .iter()
                .map(|x| x.saturating_add(r.below(5000) as i64))
                .collect();
            let flags: Vec<u32> = (0..n)
                .map(|i| if i % 9 == 0 { 1 << (i % 5) } else { 0 })
                .collect();
            out.push(block(&s, Some(&spans), Some(&flags)));
        }
        if k % 4 == 1 {
            let mut next = s[1..].to_vec();
            next.push(s[n - 1].saturating_add(1));
            out.push(block(&s, Some(&next), None));
        }
    }
    out
}

/// The same times give the same bytes, and they are golden/v2-times.hex's (`WW_TAPE_GOLDEN=update` rewrites it):
/// one chunk block per line, in hex.
#[test]
fn times_deterministic_and_golden() {
    let blocks = golden_blocks();
    assert_eq!(blocks, golden_blocks(), "two encodes of the same times");
    let mut hex = String::new();
    for b in &blocks {
        for x in b {
            hex.push_str(&std::format!("{x:02x}"));
        }
        hex.push('\n');
    }
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-times.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-times.hex: run with WW_TAPE_GOLDEN=update");
    assert_eq!(
        hex, want,
        "the format changed: bytes differ from golden/v2-times.hex"
    );
    // and the golden blocks read back to the workloads' starts
    let starts: Vec<Vec<i64>> = workloads()
        .into_iter()
        .map(|(_, mut s)| {
            s.truncate(300);
            s
        })
        .collect();
    for line in want.lines() {
        let b: Vec<u8> = (0..line.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
            .collect();
        let block = Block::read(&b).unwrap();
        let t = EventTimes::read(&block.chunk().unwrap()).unwrap();
        let got: Vec<i64> = t.starts().iter().collect();
        assert!(starts.contains(&got));
    }
}
