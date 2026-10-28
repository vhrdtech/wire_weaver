//! Quantities with a binary prefix or a `Pow10`, `Pow2` or `FullScale` scale (STD-59): the same conversions,
//! comparisons and arithmetic as with an SI prefix, exact through the prefixes' magnitudes.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::exact_units::Magnitude;
use ww_si::units;
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Quantity, Ratio, Second, Volt};

type Byte = Quantity<units::Byte>;

fn exact(m: Magnitude) -> Prefix {
    let p = Prefix::from_magnitude(&m).unwrap();
    assert!(!p.is_named(), "{m} is a named prefix");
    p
}

fn duty16() -> Prefix {
    exact(Magnitude::ratio(1, 65535))
}

#[test]
fn prefix_is_four_bytes() {
    // a tag and the scale's parameter inline, or a newer scale's u16 tag (DER-25)
    assert_eq!(size_of::<Prefix>(), 4);
    assert_eq!(size_of::<Option<Prefix>>(), 4);
}

/// DER-25: a scale from a newer version is read as `Unknown(tag)`, the rest of the message intact; nothing converts it.
#[test]
fn unknown_prefix_is_read_past_and_refused() {
    let q = Volt::new(Prefix::Unknown(1000), 5u8);
    let mut buf = [0u8; 16];
    let bytes = (q, 7u8).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(<(Volt, u8)>::from_ww_bytes(bytes).unwrap(), (q, 7));
    assert!(q.to_f64().is_nan());
    assert_eq!(q.with_prefix(Prefix::Unit), None);
    assert_eq!(q.checked_add(&Volt::new(Prefix::Unit, 1u8)), None);
    assert_eq!(std::format!("{q}"), "5·<scale 1000> V");
    assert_eq!(Prefix::Unknown(1000).magnitude(), None);
    // one unknown scale compares by the numbers, another doesn't compare
    assert!(q < Volt::new(Prefix::Unknown(1000), 6u8));
    assert_eq!(q.partial_cmp(&Volt::new(Prefix::Unknown(1001), 5u8)), None);
    assert_eq!(q.partial_cmp(&Volt::new(Prefix::Unit, 5u8)), None);
}

#[test]
fn binary_prefix_converts_exactly() {
    let b = Byte::new(Prefix::Kibi, 3u16);
    assert_eq!(b.to_f64(), 3072.0);
    assert_eq!(b.to_u32_in(Prefix::Unit), Some(3072));
    // 3 KiB = 3.072 kB, truncated
    assert_eq!(b.to_u32_in(Prefix::Kilo), Some(3));
    assert_eq!(
        b.with_prefix(Prefix::Unit),
        Some(Byte::new(Prefix::Unit, 3072u16))
    );
    assert_eq!(b.with_prefix(Prefix::Kilo), None);
    assert_eq!(
        Byte::new(Prefix::Mebi, 1u8).to_u32_in(Prefix::Kibi),
        Some(1024)
    );
    assert_eq!(b, Byte::new(Prefix::Unit, 3072u32));
    assert!(b > Byte::new(Prefix::Kilo, 3u8));
    assert!(b < Byte::new(Prefix::Kilo, 4u8));
    assert_eq!(format!("{b}"), "3 KiB");
    assert_eq!(format!("{b:#}"), "3072 B");
}

#[test]
fn exact_fraction_converts_exactly() {
    let d = Ratio::new(duty16(), 32768u16);
    assert_eq!(d.prefix.pow10(), None);
    assert!((d.to_f64() - 32768.0 / 65535.0).abs() < 1e-15);
    // 65535 · 1/65535 is 1 exactly
    let full = Ratio::new(duty16(), 65535u16);
    assert_eq!(full.to_u32_in(Prefix::Unit), Some(1));
    assert_eq!(full, Ratio::new(Prefix::Unit, 1u8));
    assert_eq!(
        full.with_prefix(Prefix::Unit),
        Some(Ratio::new(Prefix::Unit, 1u16))
    );
    // 32768/65535 = 0.50000763 is 500 ‰, truncated, and not a whole number of them
    assert_eq!(d.to_i32_in(Prefix::Milli), Some(500));
    assert_eq!(d.with_prefix(Prefix::Milli), None);
    assert!(d > Ratio::new(Prefix::Centi, 50u8));
    assert!(d < Ratio::new(Prefix::Centi, 51u8));
    assert_eq!(format!("{}", duty16()), "·1/65535");
}

#[test]
fn exact_power_of_ten_takes_the_fast_values() {
    let p = exact(Magnitude::pow10(-4));
    assert_eq!(p.pow10(), Some(-4));
    assert_eq!(p.si_pow10(), None);
    let v = Volt::new(p, 12345u16);
    assert_eq!(v.to_u32_in(Prefix::Micro), Some(1_234_500));
    // 1 234 500 doesn't fit the U16
    assert_eq!(v.with_prefix(Prefix::Micro), None);
    assert_eq!(v, Volt::new(Prefix::Micro, 1_234_500u32));
}

#[test]
fn float_and_quantize_through_exact_prefix() {
    let d = Ratio::quantize(0.5, duty16(), ww_si::ww_numeric::NumericBaseType::U16);
    assert_eq!(d.value, NumericValue::U16(32768));
    assert!((Ratio::new(duty16(), 0.5f32).to_f32() - 0.5 / 65535.0).abs() < 1e-9);
}

#[test]
fn steps_and_sums_in_binary_prefixes() {
    let a = Byte::new(Prefix::Kibi, 1u32);
    let b = Byte::new(Prefix::Unit, 10u32);
    assert_eq!(a.checked_add(&b), Some(Byte::new(Prefix::Unit, 1034u32)));
    let t: Vec<_> = Second::new(exact(Magnitude::pow2(-15)), 0u32)
        .steps(&Second::milli(1000u16))
        .unwrap()
        .take(2)
        .collect();
    assert_eq!(t[1], Second::new(exact(Magnitude::pow2(-15)), 32768u32));
}

#[test]
fn exact_prefix_on_the_wire() {
    let d = Ratio::new(duty16(), 32768u16);
    let mut buf = [0u8; 256];
    let bytes = d.to_ww_bytes(&mut buf).unwrap().to_vec();
    let back = Ratio::from_ww_bytes(&bytes).unwrap();
    assert!(back.same_repr(&d));
    let k = Byte::new(Prefix::Kibi, 3u16);
    let bytes = k.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(Byte::from_ww_bytes(&bytes).unwrap().same_repr(&k));
}
