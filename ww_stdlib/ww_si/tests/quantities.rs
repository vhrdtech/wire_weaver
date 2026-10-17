//! Constructors, conversions and comparisons of quantities, and their bytes on the wire.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::ww_numeric::{NumericValue, UVlq32};
use ww_si::{Ampere, DeciBel, Hertz, Joule, Prefix, Ratio, Second, Timed, Torque, Volt};

#[test]
fn constructors() {
    let v = Volt::millivolts(3300u16);
    assert_eq!(v.prefix, Prefix::Milli);
    assert_eq!(v.value, NumericValue::U16(3300));
    let v = Volt::volts_f32(3.3);
    assert_eq!(v.prefix, Prefix::Unit);
    assert_eq!(v.value, NumericValue::F32(3.3));
    assert_eq!(Hertz::kilohertz(400u32).prefix, Prefix::Kilo);
    assert_eq!(
        Second::microseconds(UVlq32(7)).value,
        NumericValue::UVlq32(UVlq32(7))
    );
    assert_eq!(Second::new(Prefix::Nano, 5u8).value, NumericValue::U8(5));
}

#[test]
fn conversions() {
    assert_eq!(Volt::millivolts(3300u16).to_f64(), 3.3);
    assert_eq!(
        Volt::volts_f32(3.3).to_f64_in(Prefix::Milli).round(),
        3300.0
    );
    assert_eq!(Volt::volts_f32(3.3).to_i128_in(Prefix::Milli), Some(3300));
    assert_eq!(
        Volt::millivolts(3300u16).to_i128_in(Prefix::Micro),
        Some(3_300_000)
    );
    // truncated toward zero when the target prefix is bigger
    assert_eq!(Volt::millivolts(3999u16).to_i128_in(Prefix::Unit), Some(3));
    assert_eq!(Hertz::megahertz(8u8).to_f32(), 8e6);
    assert_eq!(Second::microseconds(1_500_000u32).to_f64_seconds(), 1.5);
    assert_eq!(
        Second::new(Prefix::Quetta, u64::MAX).to_i128_in(Prefix::Quecto),
        None
    );
    assert_eq!(DeciBel::new(-3i8).to_f64(), -3.0);
}

#[test]
fn comparisons_across_prefix_and_storage() {
    assert_eq!(Volt::millivolts(1000u16), Volt::volts(1u8));
    assert_eq!(Volt::millivolts(3300u16), Volt::microvolts(3_300_000u32));
    assert_eq!(Volt::volts_f32(1.5), Volt::millivolts(1500u16));
    assert_ne!(Volt::millivolts(1001u16), Volt::volts(1u8));
    assert!(Volt::millivolts(999u16) < Volt::volts(1u8));
    assert!(Hertz::gigahertz(1u8) > Hertz::hertz(999_999_999u32));
    // prefixes 60 orders of magnitude apart: no overflow, still exact
    assert!(Second::new(Prefix::Quetta, 1u8) > Second::new(Prefix::Quecto, u128::MAX));
    assert!(Second::new(Prefix::Quecto, -1i8) < Second::new(Prefix::Quetta, 0u8));
    assert!(
        Volt::volts_f32(f32::NAN)
            .partial_cmp(&Volt::volts(0u8))
            .is_none()
    );
    assert!(DeciBel::new(-3i8) < DeciBel::new(0u8));
}

/// Prefix and storage are the device's choice, they show on the wire unchanged.
#[test]
fn wire_bytes() {
    let mut buf = [0u8; 16];
    let bytes = Volt::millivolts(3300u16).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x12, 0xE4, 0x0C]);
    let bytes = Second::microseconds(UVlq32(100))
        .to_ww_bytes(&mut buf)
        .unwrap();
    assert_eq!(bytes, [0x28, 0x20, 0x64]);
}

#[test]
fn ratio_and_torque() {
    assert_eq!(Ratio::percent(50u8), Ratio::ratio_f32(0.5));
    assert_eq!(Ratio::ppm(250u16).to_f64(), 0.00025);
    assert_eq!(
        Torque::millinewton_meters(1500i32),
        Torque::newton_meters_f32(1.5)
    );
    let r = ww_si::quantity!(95 % u8);
    assert_eq!(r.prefix, Prefix::Centi);
    let t = ww_si::quantity!(250 mNm i32);
    assert_eq!(t.to_f32(), 0.25);
    // same dimension as `Joule`, but a type of its own: no mixing them up
    let _ = Joule::new(Prefix::Unit, 1u8);
}

/// `Timed` is derived with a type parameter (DER-14): the bytes are what the derive gives the same fields without one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Debug),
    final_structure
)]
struct TimedAmpere {
    t: Second,
    value: Ampere,
}

#[test]
fn timed_wire_bytes() {
    let t = Second::microseconds(1_000_000u32);
    let value = Ampere::milliamperes(-1200i32);
    let mut buf = [0u8; 32];
    let derived = TimedAmpere { t, value }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();
    let mut buf = [0u8; 32];
    let timed = Timed::new(t, value).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(timed, derived.as_slice());
    let back = Timed::<Ampere>::from_ww_bytes(timed).unwrap();
    assert_eq!(back, Timed::new(t, value));
    let owned = Timed::new(t, value).to_ww_bytes_owned().unwrap();
    assert_eq!(owned, derived);
    assert_eq!(
        Timed::<Ampere>::from_ww_bytes_owned(&owned).unwrap(),
        Timed::new(t, value)
    );
}
