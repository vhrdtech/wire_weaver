//! Bounded quantities (STD-59): `Quantity<U, B>` with the range in the unit itself, whatever the prefix, checked when
//! a value is built and when it is read.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use shrink_wrap::series::{Series, SeriesOwned};
use ww_si::exact_units::Magnitude;
use ww_si::ww_numeric::Integer;
use ww_si::{
    Limit, OutOfBounds, Prefix, Quantity, QuantityBounds, RangeDec, RangeU64, Unbounded, Volt,
    units,
};

/// `0 V ..= 3.3 V`
type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;
/// `0 B ..= 2^40 B`
type Disk = Quantity<units::Byte, RangeU64<0, 1_099_511_627_776>>;

#[test]
fn try_new_across_prefixes() {
    assert!(Rail::try_new(Prefix::Milli, 3300u16).is_ok());
    assert_eq!(Rail::try_new(Prefix::Milli, 3301u16), Err(OutOfBounds));
    assert!(Rail::try_new(Prefix::Unit, 3u8).is_ok());
    assert_eq!(Rail::try_new(Prefix::Unit, 4u8), Err(OutOfBounds));
    assert!(Rail::try_new(Prefix::Micro, 0u8).is_ok());
    assert_eq!(Rail::try_new(Prefix::Micro, -1i8), Err(OutOfBounds));
    assert_eq!(Rail::try_new(Prefix::Kilo, 1u8), Err(OutOfBounds));
    // 3 KiV is far out, 3 · 2^-10 V is in
    assert_eq!(Rail::try_new(Prefix::Kibi, 3u8), Err(OutOfBounds));
    let per_kibi = Prefix::from_magnitude(&Magnitude::pow2(-10)).unwrap();
    assert!(Rail::try_new(per_kibi, 3379u16).is_ok()); // 3.2998 V
    assert_eq!(Rail::try_new(per_kibi, 3380u16), Err(OutOfBounds)); // 3.3008 V
}

#[test]
fn exact_prefix_bound() {
    // 3.3 V full scale of a u16: 3.3/65535 V per step
    let step = Prefix::from_magnitude(&Magnitude::ratio(1, 65535)).unwrap();
    assert!(!step.is_named());
    // 3 V is 65535 · 1/65535 · 3
    assert!(Rail::try_new(step, 3u32 * 65535).is_ok());
    assert!(Rail::try_new(step, 216_265u32).is_ok()); // 3.29999 V
    assert_eq!(Rail::try_new(step, 216_266u32), Err(OutOfBounds)); // 3.300005 V
}

#[test]
fn floats_and_nan() {
    assert!(Rail::try_new(Prefix::Unit, 3.3f32).is_ok());
    assert_eq!(Rail::try_new(Prefix::Unit, 3.31f32), Err(OutOfBounds));
    assert!(Rail::try_new(Prefix::Milli, 3299.5f64).is_ok());
    assert_eq!(Rail::try_new(Prefix::Unit, f32::NAN), Err(OutOfBounds));
    assert_eq!(Rail::try_new(Prefix::Unit, -0.001f32), Err(OutOfBounds));
    // unbounded takes NaN
    assert!(Volt::try_new(Prefix::Unit, f32::NAN).is_ok());
}

#[test]
fn bounded_and_back() {
    let v = Volt::milli(3000u16);
    let r: Rail = v.bounded().unwrap();
    assert_eq!(r.unbounded(), v);
    assert_eq!(
        Volt::milli(5000u16).bounded::<RangeDec<0, 33, -1>>(),
        Err(OutOfBounds)
    );
    assert_eq!(r.to_f64(), 3.0);
    assert_eq!(r.to_string(), Volt::milli(3000u16).to_string());
}

#[test]
fn with_prefix_stays_in_bounds() {
    let r = Rail::try_new(Prefix::Milli, 3300u16).unwrap();
    assert_eq!(r.with_prefix(Prefix::Unit), None); // 3.3 is not a u16
    let f = Rail::try_new(Prefix::Unit, 3.3f32).unwrap();
    let mv = f.with_prefix(Prefix::Milli).unwrap();
    assert!(mv.to_f64() <= 3.3 + 1e-6);
}

#[test]
fn decode_refuses_out_of_range() {
    let mut buf = [0u8; 16];
    let bytes = Volt::milli(3300u16).to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(Rail::from_ww_bytes(&bytes).is_ok());
    let bytes = Volt::milli(3301u16).to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(matches!(
        Rail::from_ww_bytes(&bytes),
        Err(shrink_wrap::Error::SubtypeOutOfRange)
    ));
    // the bound is a marker: same bytes as the plain quantity
    let r = Rail::try_new(Prefix::Milli, 3300u16).unwrap();
    let mut buf2 = [0u8; 16];
    assert_eq!(
        r.to_ww_bytes(&mut buf2).unwrap(),
        Volt::milli(3300u16).to_ww_bytes(&mut buf).unwrap()
    );
    assert_eq!(size_of::<Rail>(), size_of::<Volt>());
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug))]
struct RailSample {
    v: Rail,
}

#[test]
fn series_of_bounded() {
    let items: Vec<RailSample> = [3000u16, 3300, 0]
        .into_iter()
        .map(|mv| RailSample {
            v: Rail::try_new(Prefix::Milli, mv).unwrap(),
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    let back = SeriesOwned::<RailSample>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.len(), 3);
    for (a, b) in back.iter().zip(&items) {
        assert_eq!(a.v, b.v);
    }

    #[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug))]
    struct VoltSample {
        v: Volt,
    }
    let out = [VoltSample {
        v: Volt::milli(4000u16),
    }];
    let bytes = Series::new(&out).to_ww_bytes_owned().unwrap();
    assert!(SeriesOwned::<RailSample>::from_ww_bytes_owned(&bytes).is_err());
}

#[test]
fn range_u64() {
    assert!(Disk::try_new(Prefix::Tebi, 1u8).is_ok());
    assert_eq!(Disk::try_new(Prefix::Tebi, 2u8), Err(OutOfBounds));
    assert!(Disk::try_new(Prefix::Gibi, 1024u16).is_ok());
    assert_eq!(Disk::try_new(Prefix::Tera, 2u8), Err(OutOfBounds));
}

#[test]
fn limit_normalized() {
    assert_eq!(<Unbounded as QuantityBounds>::MIN, None);
    assert_eq!(
        <RangeDec<0, 33, -1> as QuantityBounds>::MAX,
        Some(Limit {
            value: Integer::new(false, 33),
            exp: -1
        })
    );
    assert_eq!(Limit::new(1200, 0).exp, 0);
    // trailing zeros move into the exponent only when the number doesn't fit the widest integer enabled
    let max = ww_si::ww_numeric::Magnitude::MAX as u128;
    if max < i128::MAX as u128 / 100 {
        let l = Limit::new(-((max / 10 + 1) as i128 * 100), 2);
        assert_eq!(l.exp, 4);
        assert_eq!(l.value, Integer::new(true, (max / 10 + 1) as _));
    }
}
