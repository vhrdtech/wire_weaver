//! Series tracks: the describe record of the track (shape, period, scale) and its segment records (first index,
//! grid index, grid tick, scale, the record's time), in a describe block and a records block.

use super::{Draw, RUN, describe_block, records_block};
use shrink_wrap::prelude::*;
use ww_tape::{
    BlockId, Describe, FullVersion, Magnitude, Ratio64, SampleShape, SeriesDef, SeriesSegment,
    TrackDef, TrackKind, Version, track_key,
};

/// The track number of the hostile series.
pub(crate) const TRACK: u32 = 1;

/// The describe record of series track [TRACK] on clock 1 with `shape`, `period` and `scale`; empty when it doesn't
/// encode.
pub(crate) fn track_item(shape: SampleShape, period: Ratio64, scale: Magnitude) -> Vec<u8> {
    let def = SeriesDef {
        sample: None,
        shape,
        period,
        scale,
        summaries: None,
        media: None,
    };
    let track = Describe::Track(TrackDef {
        number: UNib32(TRACK),
        key: track_key(0x5e, &[1]),
        clock: UNib32(1),
        dimensions: RefVec::Slice { slice: &[] },
        api: FullVersion::new("hostile_api", Version::new(0, 1, 0)),
        bundle: RefVec::Slice { slice: &[] },
        path: RefVec::Slice {
            slice: &[UNib32(1)],
        },
        item: None,
        label: "hostile",
        sticky: true,
        kind: TrackKind::Series(def),
    });
    let mut buf = [0u8; 256];
    track
        .to_ww_bytes(&mut buf)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// A power of ten from the input, the extremes a magnitude holds (10^-63, 10^63) included.
fn magnitude(d: &mut Draw<'_>) -> Magnitude {
    let e = match d.u8() % 4 {
        0 => -63,
        1 => 63,
        _ => -((d.u8() % 19) as i8),
    };
    Magnitude::pow10(e)
}

/// A period whose parts take hostile values; 1 when they make no ratio.
fn period(d: &mut Draw<'_>) -> Ratio64 {
    let n = d.value(64, &[1 << 53, u64::from(u32::MAX)], &[1000]);
    let m = d.value(64, &[1 << 53, u64::from(u32::MAX)], &[3]);
    Ratio64::new(n, m)
        .or_else(|| Ratio64::new(1 + n % 100_000, 1 + m % 1000))
        .expect("parts above 0")
}

/// A describe block of a series track and a records block of its segments, whose indexes, grid ticks, scales,
/// record times and period take hostile values.
pub fn series(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let period = period(&mut d);
    let scale = magnitude(&mut d);
    let channels = UNib32(1 + d.below(3) as u32);
    let shape = SampleShape {
        element: ww_tape::Element::I16,
        channels,
    };
    let item = track_item(shape, period, scale);
    let mut out = describe_block(BlockId::new(RUN, 0), &[&item]);
    // segments, valid ones first, then hostile fields
    let mut segs: Vec<SeriesSegment> = vec![];
    let mut first = u64::from(d.u16());
    let mut tick = i64::from(d.u32() as i32);
    for k in 0..1 + d.below(5) {
        let s = if k % 3 == 2 && !segs.is_empty() {
            segs[segs.len() - 1].rescaled(first, magnitude(&mut d))
        } else {
            SeriesSegment::start(first, tick, magnitude(&mut d))
        };
        segs.push(s);
        first = first.saturating_add(1 + u64::from(d.u16()));
        tick = tick.saturating_add(i64::from(d.u32()));
    }
    let caps = &[u64::MAX / 2, i64::MAX as u64, 1 << 53];
    for _ in 0..1 + d.below(3) {
        let i = d.below(segs.len());
        let own = [segs[i].first_index, segs[i].grid_index];
        let s = &mut segs[i];
        match d.u8() % 4 {
            0 => s.first_index = d.value(64, caps, &own),
            1 => s.grid_index = d.value(64, caps, &own),
            2 => s.grid_tick = d.value(64, caps, &[s.grid_tick as u64]) as i64,
            _ => s.scale = magnitude(&mut d),
        }
    }
    let encoded: Vec<(i64, Vec<u8>)> = segs
        .iter()
        .map(|s| {
            let mut buf = [0u8; 128];
            let bytes = s
                .to_ww_bytes(&mut buf)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            let at = s.first_tick(period).unwrap_or(s.grid_tick);
            (at, bytes)
        })
        .collect();
    let mut records: Vec<(u32, i64, &[u8])> = encoded
        .iter()
        .map(|(t, b)| (TRACK, *t, b.as_slice()))
        .collect();
    // a record time off its segment's first tick
    if d.u8() % 4 == 3 {
        let i = d.below(records.len());
        records[i].1 = d.value(64, &[], &[records[i].1 as u64]) as i64;
    }
    out.extend(records_block(BlockId::new(RUN, 0), &records));
    out
}
