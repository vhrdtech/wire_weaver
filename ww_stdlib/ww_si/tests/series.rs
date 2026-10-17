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

#[test]
fn design_example() {
    let items: Vec<IvSample> = [3300u16, 3301, 3301, 3299]
        .into_iter()
        .zip([152_000i32, 152_002, 151_999, 152_001])
        .map(|(v, i)| IvSample {
            v: Volt::millivolts(v),
            i: Ampere::new(Prefix::Micro, i),
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
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
