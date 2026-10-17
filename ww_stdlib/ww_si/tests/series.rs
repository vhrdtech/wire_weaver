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
    use ww_si::{Second, Timed};
    let items: Vec<Timed<IvSample>> = (0..100u32)
        .map(|k| Timed {
            t: Second::micro(1_000_000 + k * 1000),
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
    // the time column: its prefix and storage runs, the header, the first value and the step
    assert!(
        bytes.len() <= without.len() + 10,
        "{} vs {}",
        bytes.len(),
        without.len()
    );
    let back = SeriesOwned::<Timed<IvSample>>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back[99].t.value, NumericValue::U32(1_099_000));
    assert_eq!(back[99].value.v.value, NumericValue::U16(3301));
}

/// `Series<Timed<Volt>>`: 32 samples at a 40 µs clock, the same 22 bytes as ww_si 0.1.
#[test]
fn timed_volt_bytes() {
    use ww_si::{Second, Timed};
    let items: Vec<Timed<Volt>> = (0..32u32)
        .map(|k| {
            Timed::new(
                Second::micro(1_000_000 + k * 40),
                Volt::milli(3300u16 + (k % 3) as u16),
            )
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    within_bound::<Timed<Volt>>(items.len(), &bytes);
    assert_eq!(
        bytes,
        [
            0x84, 0x21, 0x98, 0x00, 0x3d, 0x09, 0x02, 0xa0, 0x22, 0x28, 0x67, 0x20, 0xab, 0xae,
            0x85, 0xd7, 0x5c, 0x2b, 0xae, 0xb1, 0x57, 0x5c
        ]
    );
    let back = SeriesOwned::<Timed<Volt>>::from_ww_bytes_owned(&bytes).unwrap();
    for (a, b) in back.iter().zip(&items) {
        assert!(a.t.same_repr(&b.t) && a.value.same_repr(&b.value));
    }
}
