//! `#[derive_shrink_wrap(.., series)]` (SW-60): user structs and enums as items of a `Series`, round trips per
//! feature, and the design's example bytes from types written the way an API crate writes them.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{
    CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor, Series, SeriesColumns, SeriesOwned,
};

#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
enum Prefix {
    Unit,
    Milli,
    Micro,
}

#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
enum NumericValue {
    U8(u8) = 1,
    U16(u16) = 2,
    I32(i32) = 6,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Volt {
    prefix: Prefix,
    value: NumericValue,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Ampere {
    prefix: Prefix,
    value: NumericValue,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct IvSample {
    v: Volt,
    i: Ampere,
}

fn round_trip<T>(items: &[T]) -> Vec<u8>
where
    T: for<'i> SeriesColumns<'i> + Clone + PartialEq + core::fmt::Debug,
{
    let bytes = Series::new(items).to_ww_bytes_owned().unwrap();
    let back = SeriesOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.0, items);
    let mut buf = vec![0u8; bytes.len() + 64];
    assert_eq!(
        Series::new(items).to_ww_bytes(&mut buf).unwrap(),
        &bytes[..],
        "BufWriter and BufWriterOwned"
    );
    bytes
}

/// The design's first example (`series.md`, "Example: 4 items"): the bytes the hand-written impls of the
/// `Series` tests gave, now from the derive.
#[test]
fn iv_sample_design_bytes() {
    let items: Vec<IvSample> = [3300, 3301, 3301, 3299]
        .into_iter()
        .zip([152_000, 152_002, 151_999, 152_001])
        .map(|(v, i)| IvSample {
            v: Volt {
                prefix: Prefix::Milli,
                value: NumericValue::U16(v),
            },
            i: Ampere {
                prefix: Prefix::Micro,
                value: NumericValue::I32(i),
            },
        })
        .collect();
    let bytes = round_trip(&items);
    assert_eq!(bytes, hex!("42 11 14 33 90 51 91 8a 00 04 a3 80 1c b0"));
    let back: Vec<IvSample> = Series::<IvSample>::from_ww_bytes(&bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(back, items);
}

/// Tuple and named variants with several fields, a unit variant, discriminants out of declaration order.
#[derive_shrink_wrap(borrowed, owned, series, ww_repr = u8, no_future_version, derive(Clone, Debug, PartialEq))]
enum Event<'i> {
    Reading(Prefix, i32, bool) = 5,
    Label { text: &'i str, level: u8 } = 1,
    Idle = 0,
    Pair(Option<u16>, Prefix) = 3,
}

#[test]
fn enum_variants() {
    let items = [
        Event::Idle,
        Event::Reading(Prefix::Milli, 100, true),
        Event::Reading(Prefix::Milli, 101, true),
        Event::Label {
            text: "hot",
            level: 3,
        },
        Event::Reading(Prefix::Micro, 100_000, false),
        Event::Pair(Some(7), Prefix::Unit),
        Event::Pair(None, Prefix::Micro),
        Event::Idle,
        Event::Label {
            text: "hot",
            level: 4,
        },
    ];
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    let back: Vec<Event> = Series::<Event>::from_ww_bytes(&bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(back, items);
    let owned = SeriesOwned::<EventOwned>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(
        owned[3],
        EventOwned::Label {
            text: "hot".into(),
            level: 3
        }
    );
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
}

/// A field after an enum field restarts its column where the enum changed, in a struct and in a variant: with the
/// delta + prefix code a restart is a full value, a delta across it a long residual.
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Scaled {
    prefix: Prefix,
    value: i32,
}

#[test]
fn restarts_after_enum_fields() {
    let delta_prefix = CodecPolicy::fixed(
        NumericCodec::new(Predictor::Delta, Coder::Prefix),
        FloatCodec::Plain,
    );
    let items = [
        Scaled {
            prefix: Prefix::Micro,
            value: 480,
        },
        Scaled {
            prefix: Prefix::Micro,
            value: 482,
        },
        Scaled {
            prefix: Prefix::Milli,
            value: 120,
        },
        Scaled {
            prefix: Prefix::Milli,
            value: 121,
        },
    ];
    // n 4, prefix run 4 + 1 + 4 + 4 + 1, header 4, 480 in 32, +2 in 9, 120 in 32 (restart), +1 in 9: 104 bits
    let bytes = Series::with_policy(&items, delta_prefix)
        .to_ww_bytes_owned()
        .unwrap();
    assert_eq!(bytes.len(), 13);
    assert_eq!(
        SeriesOwned::<Scaled>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        items
    );

    // the same inside a tuple variant: `Reading(prefix, value, _)`
    let events = [
        Event::Reading(Prefix::Micro, 480, true),
        Event::Reading(Prefix::Micro, 482, true),
        Event::Reading(Prefix::Milli, 120, true),
        Event::Reading(Prefix::Milli, 121, true),
    ];
    let with = Series::with_policy(&events, delta_prefix)
        .to_ww_bytes_owned()
        .unwrap();
    let back: Vec<Event> = Series::<Event>::from_ww_bytes(&with)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(back, events);
    // n 4, tag run 4 + 1, prefix run 14, value column 4 + 32 + 9 + 32 + 9, bool run 1 + 1: 111 bits
    assert_eq!(with.len(), 14);
}

/// Nested user types: an enum and an evolvable struct inside a struct, through their own impls.
#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, PartialEq))]
struct Inner<'i> {
    id: u16,
    name: &'i str,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, PartialEq))]
struct Outer<'i> {
    event: Event<'i>,
    inner: Inner<'i>,
    maybe: Option<Inner<'i>>,
    t: f32,
}

#[test]
fn nested_user_types() {
    let items: Vec<OuterOwned> = (0..20u16)
        .map(|k| OuterOwned {
            event: if k % 4 == 0 {
                EventOwned::Idle
            } else {
                EventOwned::Reading(Prefix::Milli, k as i32 * 10, k % 3 == 0)
            },
            inner: InnerOwned {
                id: 100 + k,
                name: "inner".into(),
            },
            maybe: (k % 5 != 0).then(|| InnerOwned {
                id: k,
                name: format!("n{}", k / 7),
            }),
            t: k as f32 * 0.5,
        })
        .collect();
    let bytes = round_trip(&items);
    let borrowed: Vec<Outer> = Series::<Outer>::from_ww_bytes(&bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(borrowed[6].maybe.as_ref().unwrap().name, "n0");
    assert_eq!(borrowed[7].maybe.as_ref().unwrap().name, "n1");
}

/// `#[series(opaque)]`: a list of structs and a `Result` as opaque runs, in a nested evolvable struct (its group
/// length is measured by serializing the values into the writer's free space).
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Point {
    x: i16,
    y: i16,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, PartialEq))]
struct Track<'i> {
    id: u8,
    #[series(opaque)]
    points: RefVec<'i, Point>,
    #[series(opaque)]
    status: Result<u8, Prefix>,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, PartialEq))]
struct Tracks<'i> {
    track: Track<'i>,
    n: u32,
}

#[test]
fn opaque_fields() {
    let owned: Vec<TracksOwned> = (0..12u8)
        .map(|k| TracksOwned {
            track: TrackOwned {
                id: k,
                points: (0..k % 4)
                    .map(|p| Point {
                        x: p as i16,
                        y: -(k as i16),
                    })
                    .collect(),
                status: if k < 6 { Ok(k / 3) } else { Err(Prefix::Micro) },
            },
            n: 7,
        })
        .collect();
    let bytes = round_trip(&owned);

    // the borrowed side writes the same bytes without allocating
    let points: Vec<Vec<Point>> = owned.iter().map(|t| t.track.points.clone()).collect();
    let borrowed: Vec<Tracks> = owned
        .iter()
        .zip(&points)
        .map(|(t, p)| Tracks {
            track: Track {
                id: t.track.id,
                points: RefVec::Slice { slice: p },
                status: t.track.status,
            },
            n: t.n,
        })
        .collect();
    let mut buf = [0u8; 512];
    assert_eq!(
        Series::new(&borrowed).to_ww_bytes(&mut buf).unwrap(),
        &bytes[..]
    );
    let back = Series::<Tracks>::from_ww_bytes(&bytes).unwrap();
    for (a, b) in back.iter().zip(&owned) {
        assert_eq!(
            a.track
                .points
                .iter()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            b.track.points
        );
        assert_eq!(a.track.status, b.track.status);
    }

    // too small a buffer is an error, not a panic
    for len in 0..bytes.len() {
        let mut small = vec![0u8; len];
        assert!(Series::new(&borrowed).to_ww_bytes(&mut small).is_err());
    }
}

/// A variant compiled out is `EnumVariantDisabled` on reading; `Unknown` of a newer writer can't be written.
#[derive_shrink_wrap(borrowed, owned, series, ww_repr = u4, derive(Clone, Debug, PartialEq))]
enum Mode {
    Off,
    On(u8),
    #[cfg(any())]
    Turbo(u16),
}

#[test]
fn disabled_and_future_variants() {
    // tag 2 (Turbo), patched into a run of `On`: n = 1, tag run `0001` + end, value column
    let bytes = Series::new(&[Mode::On(9)]).to_ww_bytes_owned().unwrap();
    let mut bad = bytes.clone();
    assert_eq!(bad[0], 0b0001_0001);
    bad[0] = 0b0001_0010;
    assert_eq!(
        Series::<Mode>::from_ww_bytes(&bad).err(),
        Some(ShrinkWrapError::EnumVariantDisabled)
    );
    bad[0] = 0b0001_0101;
    assert_eq!(
        Series::<Mode>::from_ww_bytes(&bad).err(),
        Some(ShrinkWrapError::EnumFutureVersionOrMalformedData)
    );
    assert_eq!(
        Series::new(&[Mode::Off, Mode::Unknown(7)]).to_ww_bytes_owned(),
        Err(ShrinkWrapError::EnumFutureVersionOrMalformedData)
    );
    round_trip(&[Mode::Off, Mode::On(1), Mode::On(1), Mode::Off]);
}

/// A unit struct and a struct with only flags-free plain fields, owned only.
#[derive_shrink_wrap(owned, series, derive(Clone, Debug, PartialEq))]
struct Marker;

#[derive_shrink_wrap(owned, series, derive(Clone, Debug, PartialEq))]
struct LogOwned {
    #[series(opaque)]
    lines: Vec<String>,
    marker: Marker,
}

#[test]
fn owned_only_and_unit() {
    round_trip(&[Marker, Marker]);
    round_trip(&[
        LogOwned {
            lines: vec!["a".into(), "b".into()],
            marker: Marker,
        },
        LogOwned {
            lines: vec![],
            marker: Marker,
        },
    ]);
}

/// A generic struct (DER-14) around a derived one: `Timed<IvSample>`, the time column first.
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Timed<T> {
    t: u32,
    value: T,
}

#[test]
fn generic_timed() {
    let items: Vec<Timed<IvSample>> = (0..50u32)
        .map(|k| Timed {
            t: 1_000_000 + k * 1000,
            value: IvSample {
                v: Volt {
                    prefix: Prefix::Milli,
                    value: NumericValue::U16(3300 + (k % 3) as u16),
                },
                i: Ampere {
                    prefix: Prefix::Micro,
                    value: NumericValue::I32(152_000 - k as i32),
                },
            },
        })
        .collect();
    let bytes = round_trip(&items);
    // a regular clock is a constant delta: n, the header, the first time and the step
    let plain = items.len() * 4;
    let untimed: Vec<IvSample> = items.iter().map(|t| t.value).collect();
    let without = Series::new(&untimed).to_ww_bytes_owned().unwrap();
    assert!(
        bytes.len() < without.len() + 9,
        "{} vs {}",
        bytes.len(),
        without.len()
    );
    assert!(bytes.len() < plain);
}

/// A generic enum (DER-14): its tag run, then the parameter's columns for the `Value` items.
#[derive_shrink_wrap(borrowed, owned, series, ww_repr = u4, derive(Clone, Copy, Debug, PartialEq))]
enum Sample<T> {
    Value(T),
    Gap,
    Pair { a: T, b: u8 },
}

/// Const parameters and a where clause on a series struct.
#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct Window<T, const N: usize>
where
    T: Copy + Default,
{
    at: u32,
    samples: [T; N],
}

#[test]
fn generic_enum_and_const_parameters() {
    let samples: Vec<Timed<Sample<u16>>> = (0..40u32)
        .map(|k| Timed {
            t: k * 10,
            value: match k % 7 {
                0 => Sample::Gap,
                3 => Sample::Pair {
                    a: k as u16,
                    b: k as u8,
                },
                _ => Sample::Value(1000 + k as u16),
            },
        })
        .collect();
    round_trip(&samples);
    let windows: Vec<Window<i16, 3>> = (0..20i16)
        .map(|k| Window {
            at: k as u32,
            samples: [k, -k, k * 2],
        })
        .collect();
    round_trip(&windows);
}
