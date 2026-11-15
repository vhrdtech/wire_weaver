use exact_units::prefix::{Micro, Milli, One};
use exact_units::units::{Ampere, Volt};
use exact_units::{Complex, Fixed};
use shrink_wrap::prelude::*;

use super::*;
use crate::summary::{SeriesStats, SeriesSummarySpec};
use crate::{
    BlockKind, Describe, DescribeError, DescribeSet, Dimension, EndMode, Ends, EventsDef,
    FullVersion, TrackDef, TrackKind, Version,
};

/// 48 kHz on a 1 ns clock.
fn period() -> Ratio64 {
    Ratio64::new(62_500, 3).unwrap()
}

fn milli() -> Magnitude {
    Magnitude::pow10(-3)
}

fn micro() -> Magnitude {
    Magnitude::pow10(-6)
}

fn sample_type() -> TypeRef<'static> {
    TypeRef {
        bundle: RefVec::Slice { slice: &[7, 7] },
        krate: FullVersion::new("motor_api", Version::new(0, 3, 0)),
        path: "PhaseCurrent",
    }
}

fn def() -> SeriesDef<'static> {
    SeriesDef {
        summaries: Some(SeriesSummarySpec {
            base_shift: 6,
            stats: SeriesStats::all(Element::I32),
            clip: None,
        }),
        media: Some(MediaStream {
            media: RefVec::Slice { slice: &[1, 2, 3] },
            stream: UNib32(1),
        }),
        ..SeriesDef::of::<Fixed<Ampere, Milli, i32>>(Some(sample_type()), 3, period())
    }
}

fn encode<T: SerializeShrinkWrap>(v: &T) -> std::vec::Vec<u8> {
    let mut buf = [0u8; 512];
    v.to_ww_bytes(&mut buf).unwrap().to_vec()
}

#[test]
fn def_takes_element_and_scale_from_the_type() {
    let d = def();
    assert_eq!(d.shape, SampleShape::new(Element::I32, 3));
    assert_eq!(d.scale, milli());
    assert_eq!(d.check(), Ok(()));
    let bytes = encode(&d);
    assert_eq!(SeriesDef::from_ww_bytes(&bytes).unwrap(), d);

    assert_eq!(<Fixed<Volt, Micro, i16>>::ELEMENT, Element::I16);
    assert_eq!(<Fixed<Volt, Micro, i16>>::SCALE, micro());
    assert_eq!(<Complex<Volt, One, i16>>::ELEMENT, Element::ComplexI16);
    assert_eq!(<Complex<Volt, One, f32>>::ELEMENT, Element::ComplexF32);
    assert_eq!(<f32 as SeriesValue>::SCALE, Magnitude::ONE);
    assert_eq!(<bool as SeriesValue>::ELEMENT, Element::Bool);

    let mut out = [0i16; 2];
    assert_eq!(
        Complex::<Volt, One, i16>::new(3, -4).write_parts(&mut out),
        2
    );
    assert_eq!(out, [3, -4]);
    assert_eq!(
        Complex::<Volt, One, i16>::new(5, 6).write_parts(&mut out[..1]),
        1
    );
    assert_eq!(out, [5, -4]);
    let mut one = [0i32; 1];
    assert_eq!(
        Fixed::<Ampere, Milli, i32>::new(-7).write_parts(&mut one),
        1
    );
    assert_eq!(one, [-7]);
}

#[test]
fn bad_defs_are_refused_on_decode() {
    let bad = [
        SeriesDef {
            shape: SampleShape::new(Element::I32, 0),
            ..def()
        },
        SeriesDef {
            shape: SampleShape::new(Element::I32, SERIES_CHANNELS_MAX + 1),
            ..def()
        },
        SeriesDef {
            shape: SampleShape::new(Element::Unknown(40), 1),
            summaries: None,
            ..def()
        },
        SeriesDef {
            summaries: Some(SeriesSummarySpec {
                base_shift: 2,
                stats: SeriesStats::all(Element::I32),
                clip: None,
            }),
            ..def()
        },
        SeriesDef {
            summaries: Some(SeriesSummarySpec {
                base_shift: 6,
                stats: SeriesStats::default(),
                clip: None,
            }),
            ..def()
        },
        SeriesDef {
            summaries: Some(SeriesSummarySpec {
                base_shift: 6,
                stats: SeriesStats::all(Element::ComplexI16),
                clip: None,
            }),
            ..def()
        },
    ];
    for d in bad {
        assert!(d.check().is_err(), "{d:?}");
        let bytes = encode(&d);
        assert!(SeriesDef::from_ww_bytes(&bytes).is_err(), "{d:?}");
    }
}

fn track(kind: TrackKind<'static>, sticky: bool) -> TrackDef<'static> {
    TrackDef {
        number: UNib32(2),
        key: 0x5e71e5,
        clock: UNib32(1),
        dimensions: RefVec::Slice {
            slice: &[Dimension {
                name: "phase",
                value: "a",
            }],
        },
        api: FullVersion::new("motor_api", Version::new(0, 3, 0)),
        bundle: RefVec::Slice { slice: &[7, 7] },
        path: RefVec::Slice {
            slice: &[UNib32(4), UNib32(1)],
        },
        item: None,
        label: "phase[a].current",
        sticky,
        kind,
    }
}

#[test]
fn track_kind_round_trips_and_is_checked() {
    for t in [
        track(TrackKind::Series(def()), true),
        track(
            TrackKind::Events(EventsDef {
                ends: Ends::Spans,
                flags: true,
            }),
            false,
        ),
    ] {
        let d = Describe::Track(t.clone());
        let bytes = encode(&d);
        assert_eq!(Describe::from_ww_bytes(&bytes).unwrap(), d);
        assert_eq!(t.check_kind(), Ok(()));
    }
    let t = track(TrackKind::Series(def()), true);
    assert_eq!(t.series(), Some(&def()));
    assert!(Ends::Instants.allows(EndMode::Absent));
    assert!(!Ends::Instants.allows(EndMode::Durations));
    assert!(Ends::Spans.allows(EndMode::NextStart));

    let not_sticky = track(TrackKind::Series(def()), false);
    let with_item = TrackDef {
        item: Some(sample_type()),
        ..t.clone()
    };
    for t in [not_sticky, with_item] {
        let mut set = DescribeSet::new();
        assert!(matches!(
            set.insert(&Describe::Track(t)),
            Err(DescribeError::Kind { track: 2, .. })
        ));
    }
}

#[test]
fn rescaled_segment_keeps_the_grid() {
    let p = period();
    let a = SeriesSegment::start(0, 1_000, micro());
    let b = a.rescaled(301, milli());
    let mut t = SeriesTimeline::new(p);
    t.push(a).unwrap();
    t.push(b).unwrap();
    // 301 · 62500/3 = 6270833.3..: the same tick on both segments
    assert_eq!(a.tick(p, 301), Some(1_000 + 6_270_833));
    assert_eq!(t.tick(301), a.tick(p, 301));
    assert_eq!(t.tick(302), Some(1_000 + 6_291_666));
    assert_eq!(t.scale(300), Some(micro()));
    assert_eq!(t.scale(301), Some(milli()));
    assert_eq!(t.segment_end(5), Some(301));
    assert_eq!(t.segment_end(301), None);
    assert_eq!(t.chunk_ticks(0, 301), Ok((1_000, a.tick(p, 300).unwrap())));
    assert_eq!(
        t.chunk_ticks(0, 302),
        Err(SeriesError::ChunkSpan { first_item: 0 })
    );
    // a repeat is taken once
    t.push(a).unwrap();
    t.push(b).unwrap();
    assert_eq!(t.segments(), &[a, b]);
}

#[test]
fn timeline_refuses_what_breaks_its_rules() {
    let p = period();
    let mut t = SeriesTimeline::new(p);
    assert_eq!(t.tick(0), None);
    assert_eq!(
        t.chunk_ticks(0, 1),
        Err(SeriesError::NoSegment { index: 0 })
    );
    let a = SeriesSegment::start(10, 0, micro());
    t.push(a).unwrap();
    assert_eq!(t.tick(9), None);
    assert_eq!(
        t.push(SeriesSegment::start(10, 5, micro())),
        Err(SeriesError::Conflict { first_index: 10 })
    );
    assert_eq!(
        t.push(SeriesSegment::start(5, -100, micro())),
        Err(SeriesError::NotAscending {
            first_index: 5,
            previous: 10
        })
    );
    // sample 19 is at 9 · 62500/3 = 187500: a new grid must start after it
    assert_eq!(
        t.push(SeriesSegment::start(20, 187_500, micro())),
        Err(SeriesError::TimeNotIncreasing { first_index: 20 })
    );
    t.push(SeriesSegment::start(20, 187_501, micro())).unwrap();
    let grid_after = SeriesSegment {
        first_index: 30,
        grid_index: 31,
        grid_tick: i64::MAX,
        scale: micro(),
    };
    assert_eq!(
        t.push(grid_after),
        Err(SeriesError::GridAfterFirst {
            first_index: 30,
            grid_index: 31
        })
    );
    let overflow = SeriesSegment {
        first_index: u64::MAX,
        grid_index: 0,
        grid_tick: 0,
        scale: micro(),
    };
    assert_eq!(
        t.push(overflow),
        Err(SeriesError::TickOverflow { index: u64::MAX })
    );
    assert_eq!(
        t.chunk_ticks(20, 0),
        Err(SeriesError::ChunkSpan { first_item: 20 })
    );
    assert_eq!(
        t.chunk_ticks(u64::MAX, 2),
        Err(SeriesError::ChunkSpan {
            first_item: u64::MAX
        })
    );
}

#[test]
fn records_are_checked_against_their_time() {
    let p = period();
    let a = SeriesSegment::start(0, 1_000, micro());
    let b = a.rescaled(300, milli());
    let c = SeriesSegment::start(500, 99_000_000, micro());
    let (ab, bb, cb) = (encode(&a), encode(&b), encode(&c));
    let b_tick = b.first_tick(p).unwrap();
    let records: [(i64, &[u8]); 6] = [
        (99_000_000, &cb),
        (b_tick, &bb),
        (1_000, &ab),
        (b_tick, &bb),
        // wrong time, and not a segment
        (1_001, &ab),
        (0, &[0xff, 0xff, 0xff]),
    ];
    let (t, errors) = SeriesTimeline::from_records(p, records);
    assert_eq!(t.segments(), &[a, b, c]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.contains(&SeriesError::RecordTime { first_index: 0 }));
    let mut t = SeriesTimeline::new(p);
    assert_eq!(
        t.push_record(1_001, &ab),
        Err(SeriesError::RecordTime { first_index: 0 })
    );
    t.push_record(1_000, &ab).unwrap();
}

#[test]
fn chunk_heads_are_checked_against_the_grid() {
    let p = period();
    let mut t = SeriesTimeline::new(p);
    t.push(SeriesSegment::start(0, 1_000, micro())).unwrap();
    let (first, last) = t.chunk_ticks(0, 100).unwrap();
    let mut head = crate::ChunkHead {
        kind: BlockKind::Series,
        track_id: 1,
        track_number: 2,
        first_item: 0,
        count: 100,
        first_tick: first,
        last_tick: last,
        latest_end: last,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    assert_eq!(t.check_chunk(&head), Ok(()));
    head.last_tick += 1;
    assert_eq!(
        t.check_chunk(&head),
        Err(SeriesError::ChunkTicks { first_item: 0 })
    );
    head.kind = BlockKind::Events;
    assert!(t.check_chunk(&head).is_err());
}
