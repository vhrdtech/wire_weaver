//! ww_tape series tracks on any bytes: a describe record (a track with its kind) and series segment records decode
//! without panic; segments built from the input, then damaged by input bytes written over their records, give a
//! timeline whose kept segments hold its rules, and every tick, scale and chunk check on it answers without panic.
use shrink_wrap::prelude::*;
use ww_tape::chunk::ChunkHead;
use ww_tape::{
    BlockKind, Describe, EndMode, Magnitude, Ratio64, SeriesSegment, SeriesTimeline, TrackKind,
};

/// Ask the timeline everything about samples around `at`.
fn query(t: &SeriesTimeline, at: u64, count: u32) {
    for i in [0, at, at.wrapping_sub(1), at.saturating_add(1), u64::MAX] {
        let _ = (t.tick(i), t.scale(i), t.segment_of(i), t.segment_end(i));
        if let Ok((first, last)) = t.chunk_ticks(i, count) {
            assert!(first <= last, "ticks of a chunk ascend");
            let head = ChunkHead {
                kind: BlockKind::Series,
                track_id: 1,
                track_number: 1,
                first_item: i,
                count,
                first_tick: first,
                last_tick: last,
                latest_end: last,
                time_mode: 0,
                end_mode: EndMode::Absent,
            };
            assert_eq!(t.check_chunk(&head), Ok(()));
        }
    }
}

/// The kept segments hold the timeline's rules.
fn check(t: &SeriesTimeline) {
    let s = t.segments();
    for w in s.windows(2) {
        assert!(w[0].first_index < w[1].first_index, "ascending");
    }
    for seg in s {
        let tick = seg
            .first_tick(t.period())
            .expect("a kept segment's first tick");
        assert_eq!(t.tick(seg.first_index), Some(tick));
    }
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    for (o, x) in b.iter_mut().zip(data.iter().skip(at)) {
        *o = *x;
    }
    u64::from_le_bytes(b)
}

pub fn series(data: &[u8]) {
    // any bytes as a describe record and as a segment
    if let Ok(Describe::Track(t)) = Describe::from_ww_bytes(data) {
        let _ = (t.check_kind(), t.id());
        if let TrackKind::Series(s) = &t.kind {
            let _ = (s.check(), s.first_segment(0));
        }
    }
    let _ = SeriesSegment::from_ww_bytes(data);

    let period = Ratio64::new(u64_at(data, 0) % 100_000 + 1, u64_at(data, 8) % 1000 + 1)
        .expect("parts above 0");
    let records = data.get(16..).unwrap_or(&[]);

    // segments from the input, encoded as records at their first tick
    let mut segs = vec![];
    let mut first = 0u64;
    for (k, c) in records.chunks(4).take(64).enumerate() {
        let step = u64::from(c.first().copied().unwrap_or(1));
        first = first.saturating_add(step);
        let tick =
            i64::from(c.get(1).copied().unwrap_or(0)) << (c.get(2).copied().unwrap_or(0) % 40);
        let scale = Magnitude::pow10(-((c.get(3).copied().unwrap_or(0) % 12) as i8));
        segs.push(if k % 3 == 2 && !segs.is_empty() {
            let prev: &SeriesSegment = segs.last().expect("not empty");
            prev.rescaled(first, scale)
        } else {
            SeriesSegment::start(first, tick, scale)
        });
    }
    let mut encoded: Vec<(i64, Vec<u8>)> = vec![];
    let mut buf = [0u8; 128];
    for s in &segs {
        let tick = s.first_tick(period).unwrap_or(0);
        if let Ok(b) = s.to_ww_bytes(&mut buf) {
            encoded.push((tick, b.to_vec()));
        }
    }
    let (t, _) = SeriesTimeline::from_records(period, encoded.iter().map(|(t, b)| (*t, &b[..])));
    check(&t);
    query(&t, first / 2, records.len() as u32 % 300);

    // the same records damaged: input bytes written over them, record times shifted
    for (k, (tick, b)) in encoded.iter_mut().enumerate() {
        if let Some(x) = records.get(k)
            && !b.is_empty()
        {
            let at = usize::from(*x) % b.len();
            b[at] ^= x | 1;
            *tick = tick.wrapping_add(i64::from(*x & 1));
        }
    }
    let (t, _) = SeriesTimeline::from_records(period, encoded.iter().map(|(t, b)| (*t, &b[..])));
    check(&t);
    query(&t, first / 3, 1);
}
