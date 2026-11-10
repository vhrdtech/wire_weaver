//! Quantities as fields of a `Series` item (SW-60): the design's example bytes from the real types.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use shrink_wrap::series::{Series, SeriesOwned};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Ampere, Prefix, Volt};

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug))]
struct IvSample {
    v: Volt,
    i: Ampere,
}

/// `bytes` are within the bound of a `Series` of `n` items of `T` (SW-62): quantities are bounded.
fn within_bound<T: for<'i> shrink_wrap::series::SeriesColumns<'i>>(n: usize, bytes: &[u8]) {
    let max = Series::<T>::max_bytes(n).expect("quantities have a bound");
    assert!(bytes.len() <= max, "{} bytes, bound {max}", bytes.len());
}

#[test]
fn design_example() {
    let items: Vec<IvSample> = [3300u16, 3301, 3301, 3299]
        .into_iter()
        .zip([152_000i32, 152_002, 151_999, 152_001])
        .map(|(v, i)| IvSample {
            v: Volt::milli(v),
            i: Ampere::new(Prefix::Micro, i),
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    within_bound::<IvSample>(items.len(), &bytes);
    assert_eq!(
        bytes,
        [
            0x42, 0x11, 0x14, 0x33, 0x90, 0x51, 0x91, 0x8a, 0x00, 0x04, 0xa3, 0x80, 0x1c, 0xb0
        ]
    );
    let back = SeriesOwned::<IvSample>::from_ww_bytes_owned(&bytes).unwrap();
    for (a, b) in back.iter().zip(&items) {
        assert_eq!((a.v.prefix, a.v.value), (b.v.prefix, b.v.value));
        assert_eq!((a.i.prefix, a.i.value), (b.i.prefix, b.i.value));
    }
    assert_eq!(back[3].i.value, NumericValue::I32(152_001));
}

/// `Timed<T>` (derived with a type parameter, DER-14) around the sample: a regular clock is a constant delta.
#[test]
fn timed_samples() {
    use ww_si::{Timed, origins::DeviceMicros};
    let items: Vec<Timed<IvSample>> = (0..100u32)
        .map(|k| Timed {
            t: DeviceMicros::from_raw(1_000_000 + k * 1000),
            value: IvSample {
                v: Volt::milli(3300u16 + (k % 2) as u16),
                i: Ampere::new(Prefix::Micro, 152_000i32),
            },
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    let untimed: Vec<IvSample> = items.iter().map(|t| t.value).collect();
    let without = Series::new(&untimed).to_ww_bytes_owned().unwrap();
    within_bound::<Timed<IvSample>>(items.len(), &bytes);
    within_bound::<IvSample>(untimed.len(), &without);
    // the time column: the header, the first value and the step
    assert!(
        bytes.len() <= without.len() + 10,
        "{} vs {}",
        bytes.len(),
        without.len()
    );
    let back = SeriesOwned::<Timed<IvSample>>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back[99].t.raw(), 1_099_000);
    assert_eq!(back[99].value.v.value, NumericValue::U16(3301));
}

/// `Series<Timed<Volt>>`: 32 samples at a 40 µs clock in 21 bytes (22 while `t` was a `Second`: its prefix run).
#[test]
fn timed_volt_bytes() {
    use ww_si::{Timed, origins::DeviceMicros};
    let items: Vec<Timed<Volt>> = (0..32u32)
        .map(|k| {
            Timed::new(
                DeviceMicros::from_raw(1_000_000 + k * 40),
                Volt::milli(3300u16 + (k % 3) as u16),
            )
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    within_bound::<Timed<Volt>>(items.len(), &bytes);
    assert_eq!(
        bytes,
        [
            0x84, 0x60, 0x00, 0xf4, 0x24, 0x0a, 0x80, 0x88, 0xa1, 0x9c, 0x82, 0xae, 0xba, 0x17,
            0x5d, 0x70, 0xae, 0xba, 0xc5, 0x5d, 0x70
        ]
    );
    let back = SeriesOwned::<Timed<Volt>>::from_ww_bytes_owned(&bytes).unwrap();
    for (a, b) in back.iter().zip(&items) {
        assert!(a.t == b.t && a.value.same_repr(&b.value));
    }
}

/// A current that autoranges mid-batch: µA while it is small, mA from the step on, µA again after it, and one item
/// in `I16` among `I32`s. Every policy restarts the value column at each change (a full value, fresh codec state)
/// and reads back the same prefix and number of every item; the store path leaves such a batch to the item walk.
#[test]
fn prefix_changes_mid_batch() {
    use shrink_wrap::series::{
        CodecPolicy, DeltaPacked, NumericCodec, Predictor, Coder, FloatCodec, RawPlain, SeriesPolicy, SeriesStore,
        write_store_series,
    };
    use shrink_wrap::{BufWriter, SerializeShrinkWrap};
    use ww_si::{Timed, origins::DeviceMicros};

    let items: Vec<Timed<Ampere>> = (0..32u32)
        .map(|k| {
            let i = match k {
                0..10 => Ampere::new(Prefix::Micro, 480 + (k as i32 % 3)),
                10..20 => Ampere::new(Prefix::Milli, 1500 + 10 * k as i32),
                20 => Ampere::new(Prefix::Milli, 1700i16),
                _ => Ampere::new(Prefix::Micro, 520 - k as i32),
            };
            Timed::new(DeviceMicros::from_raw(1_000_000 + k * 40), i)
        })
        .collect();

    fn check<P: SeriesPolicy + Copy>(items: &[Timed<Ampere>], policy: P) -> usize {
        let mut buf = [0u8; 1024];
        let bytes = Series::with_policy(items, policy).to_ww_bytes(&mut buf).unwrap();
        within_bound::<Timed<Ampere>>(items.len(), bytes);
        let back = SeriesOwned::<Timed<Ampere>>::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(back.len(), items.len());
        for (a, b) in back.iter().zip(items) {
            assert!(a.t == b.t && a.value.same_repr(&b.value), "{a:?} vs {b:?}");
        }
        // the store keeps every item, but a prefix change leaves the batch to the item walk
        let mut store = Timed::<Ampere>::empty_store::<32>();
        for (k, item) in items.iter().enumerate() {
            Timed::set(&mut store, k, item);
        }
        let mut other = [0u8; 1024];
        let mut wr = BufWriter::new(&mut other);
        assert!(!write_store_series::<Timed<Ampere>, P, 32>(&mut wr, &store, 0..32, &policy).unwrap());
        let walked: Vec<_> = (0..32).map(|k| Timed::<Ampere>::get(&store, k)).collect();
        let mut again = [0u8; 1024];
        assert_eq!(
            Series::with_policy(&walked, policy).to_ww_bytes(&mut again).unwrap(),
            bytes
        );
        bytes.len()
    }
    let smallest = check(&items, CodecPolicy::SMALLEST);
    let delta = check(&items, DeltaPacked);
    let dod = check(
        &items,
        CodecPolicy::fixed(
            NumericCodec::new(Predictor::DeltaOfDelta, Coder::Packed),
            FloatCodec::Xor,
        ),
    );
    let raw = check(&items, RawPlain);
    assert_eq!(back_prefixes(&items), [Prefix::Micro, Prefix::Milli, Prefix::Micro]);
    assert!(smallest <= delta.min(dod) && delta < raw, "{smallest} {delta} {dod} {raw}");

    fn back_prefixes(items: &[Timed<Ampere>]) -> Vec<Prefix> {
        let mut runs: Vec<Prefix> = items.iter().map(|t| t.value.prefix).collect();
        runs.dedup();
        runs
    }
}
