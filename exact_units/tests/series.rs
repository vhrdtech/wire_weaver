//! `Fixed` as a field of a `Series` item (SW-60): the columns of plain numbers.
#![cfg(all(feature = "std", feature = "quantities"))]

use shrink_wrap::prelude::*;
use shrink_wrap::series::{Series, SeriesOwned};

/// `bytes` are within the bound of a `Series` of `n` items of `T` (SW-62).
fn within_bound<T: for<'i> shrink_wrap::series::SeriesColumns<'i>>(n: usize, bytes: &[u8]) {
    let max = Series::<T>::max_bytes(n).expect("fixed numbers have a bound");
    assert!(bytes.len() <= max, "{} bytes, bound {max}", bytes.len());
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct PlainSample {
    t_us: u32,
    v_mv: i16,
    duty_pct: u8,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct FixedSample {
    t: exact_units::Micros<u32>,
    v: exact_units::Fixed<exact_units::units::Volt, exact_units::prefix::Milli, i16>,
    duty: exact_units::Fixed<exact_units::units::Ratio, exact_units::prefix::Centi, u8>,
}

/// A `Series` of `Fixed` fields has the bytes of the same series of plain numbers, and reads back the same values.
#[test]
fn fixed_fields_are_plain_columns() {
    use exact_units::Fixed;
    let plain: Vec<PlainSample> = (0..64u32)
        .map(|k| PlainSample {
            t_us: 1_000_000 + k * 40,
            v_mv: 3300 + (k % 3) as i16 - 1,
            duty_pct: (k % 101) as u8,
        })
        .collect();
    let fixed: Vec<FixedSample> = plain
        .iter()
        .map(|p| FixedSample {
            t: Fixed::new(p.t_us),
            v: Fixed::new(p.v_mv),
            duty: Fixed::new(p.duty_pct),
        })
        .collect();
    let plain_bytes = Series::new(&plain).to_ww_bytes_owned().unwrap();
    let fixed_bytes = Series::new(&fixed).to_ww_bytes_owned().unwrap();
    assert_eq!(fixed_bytes, plain_bytes);
    within_bound::<FixedSample>(fixed.len(), &fixed_bytes);
    let back = SeriesOwned::<FixedSample>::from_ww_bytes_owned(&fixed_bytes).unwrap();
    assert_eq!(back.iter().copied().collect::<Vec<_>>(), fixed);
}
