//! Comparisons and integer conversions of quantities with number types up to 32 bits: exact across prefixes, signs and
//! `u32` / `i32`, with every overflow deciding in `u32`. Runs with every number type (`just test`) and with the types
//! up to 32 bits only (`--no-default-features --features time,electrical,u32,i32,f32`), where no 64- or 128-bit code
//! is involved.
#![cfg(all(
    feature = "time",
    feature = "electrical",
    feature = "u32",
    feature = "i32",
    feature = "f32"
))]

use core::cmp::Ordering;
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Second, Volt};

#[test]
fn same_prefix_and_variant() {
    assert_eq!(Volt::milli(3300u32), Volt::milli(3300u32));
    assert!(Volt::milli(3300u32) < Volt::milli(3301u32));
    assert!(Volt::milli(-5i32) < Volt::milli(-4i32));
    assert!(Volt::milli(i32::MIN) < Volt::milli(i32::MAX));
    assert!(Volt::milli(u32::MAX) > Volt::milli(u32::MAX - 1));
}

#[test]
fn mixed_prefixes() {
    assert_eq!(Volt::milli(3000u32), Volt::base(3u8));
    assert_eq!(Volt::base(3u8), Volt::milli(3000u32));
    assert!(Volt::milli(3001u32) > Volt::base(3u8));
    assert!(Volt::milli(2999u32) < Volt::base(3u8));
    assert_eq!(Second::micro(5_000_000u32), Second::base(5i32));
    assert_eq!(Volt::micro(1_000_000_000u32), Volt::kilo(1u8));
    // the coarser value scaled in u32 overflows: it is the bigger one
    assert!(Volt::kilo(5_000u32) > Volt::micro(u32::MAX));
    assert!(Volt::micro(u32::MAX) < Volt::kilo(5_000u32));
    // 429_496_729 * 10 fits u32, 429_496_730 * 10 doesn't
    assert!(Volt::new(Prefix::Deci, 429_496_729u32) < Volt::new(Prefix::Centi, u32::MAX));
    assert!(Volt::new(Prefix::Deci, 429_496_730u32) > Volt::new(Prefix::Centi, u32::MAX));
    // 60 orders of magnitude apart
    assert!(Volt::new(Prefix::Quetta, 1u8) > Volt::new(Prefix::Quecto, u32::MAX));
    assert!(Volt::new(Prefix::Quecto, u32::MAX) < Volt::new(Prefix::Quetta, 1u8));
}

#[test]
fn signs() {
    assert_eq!(Volt::milli(-3000i32), Volt::base(-3i32));
    assert!(Volt::milli(-3001i32) < Volt::base(-3i32));
    assert!(Volt::base(-3i32) > Volt::milli(-3001i32));
    // a negative value against a huge positive one, and against zero at any prefix
    assert!(Volt::kilo(-1i32) < Volt::micro(1u8));
    assert!(Volt::micro(-1i32) < Volt::kilo(0u8));
    assert_eq!(Volt::kilo(0i32), Volt::micro(0u32));
    assert!(Volt::new(Prefix::Quetta, -1i32) < Volt::new(Prefix::Quecto, -(i32::MAX)));
    assert!(Volt::new(Prefix::Quecto, i32::MIN) > Volt::new(Prefix::Quetta, -1i32));
}

#[test]
fn u32_against_i32() {
    assert_eq!(Volt::milli(5u32), Volt::milli(5i32));
    assert!(Volt::milli(u32::MAX) > Volt::milli(i32::MAX));
    assert!(Volt::milli(0u32) > Volt::milli(-1i32));
    assert!(Volt::milli(u32::MAX) > Volt::base(i32::MAX / 1000));
    assert_eq!(Volt::milli(2_147_483_000u32), Volt::base(2_147_483i32));
    assert!(Volt::base(-1i32) < Volt::milli(u32::MAX));
    assert_eq!(
        Volt::milli(7u32).partial_cmp(&Volt::milli(-7i32)),
        Some(Ordering::Greater)
    );
}

#[test]
fn floats_still_compare() {
    assert_eq!(Volt::from_f32(1.5), Volt::milli(1500u32));
    assert!(Volt::from_f32(3.3) < Volt::milli(3301i32));
    assert_eq!(Volt::from_f32(f32::NAN).partial_cmp(&Volt::base(1u8)), None);
}

#[test]
fn to_i32_and_u32() {
    assert_eq!(Second::milli(5u32).to_i32_in(Prefix::Micro), Some(5000));
    assert_eq!(Second::milli(5u32).to_u32_in(Prefix::Micro), Some(5000));
    // truncated toward zero going to a bigger prefix
    assert_eq!(Volt::milli(3999u32).to_u32_in(Prefix::Unit), Some(3));
    assert_eq!(Volt::milli(-3999i32).to_i32_in(Prefix::Unit), Some(-3));
    assert_eq!(Volt::milli(-1i32).to_u32_in(Prefix::Milli), None);
    // the edges
    assert_eq!(
        Volt::milli(u32::MAX).to_u32_in(Prefix::Milli),
        Some(u32::MAX)
    );
    assert_eq!(Volt::milli(u32::MAX).to_i32_in(Prefix::Milli), None);
    assert_eq!(
        Volt::milli(i32::MIN).to_i32_in(Prefix::Milli),
        Some(i32::MIN)
    );
    assert_eq!(
        Volt::base(429_496_729u32).to_u32_in(Prefix::Deci),
        Some(4_294_967_290)
    );
    assert_eq!(Volt::base(429_496_730u32).to_u32_in(Prefix::Deci), None);
    assert_eq!(
        Volt::base(214_748_364i32).to_i32_in(Prefix::Deci),
        Some(2_147_483_640)
    );
    assert_eq!(Volt::base(214_748_365i32).to_i32_in(Prefix::Deci), None);
    assert_eq!(
        Volt::base(-214_748_364i32).to_i32_in(Prefix::Deci),
        Some(-2_147_483_640)
    );
    assert_eq!(Volt::base(1u8).to_u32_in(Prefix::Quecto), None);
    assert_eq!(
        Volt::new(Prefix::Quecto, u32::MAX).to_u32_in(Prefix::Quetta),
        Some(0)
    );
    // floats, rounded
    assert_eq!(Volt::from_f32(3.3).to_i32_in(Prefix::Milli), Some(3300));
    assert_eq!(Volt::from_f32(-0.0026).to_i32_in(Prefix::Milli), Some(-3));
    assert_eq!(Volt::from_f32(-0.4).to_u32_in(Prefix::Unit), Some(0));
    assert_eq!(Volt::from_f32(-0.6).to_u32_in(Prefix::Unit), None);
    assert_eq!(
        Volt::from_f32(-2_147_483_648.0).to_i32_in(Prefix::Unit),
        Some(i32::MIN)
    );
    assert_eq!(
        Volt::from_f32(2_147_483_648.0).to_i32_in(Prefix::Unit),
        None
    );
    assert_eq!(
        Volt::from_f32(4_294_967_040.0).to_u32_in(Prefix::Unit),
        Some(4_294_967_040)
    );
    assert_eq!(
        Volt::from_f32(4_294_967_296.0).to_u32_in(Prefix::Unit),
        None
    );
    assert_eq!(Volt::from_f32(f32::NAN).to_i32_in(Prefix::Unit), None);
}

#[test]
fn to_u32_wrapping() {
    assert_eq!(
        Second::micro(u32::MAX).to_u32_wrapping_in(Prefix::Micro),
        Some(u32::MAX)
    );
    // 5_000 s in µs is past 2^32: wraps like the device's µs counter
    assert_eq!(
        Second::base(5_000u32).to_u32_wrapping_in(Prefix::Micro),
        Some(5_000_000_000u64 as u32)
    );
    assert_eq!(
        Second::micro(-1i32).to_u32_wrapping_in(Prefix::Micro),
        Some(u32::MAX)
    );
    assert_eq!(
        Second::milli(-1i32).to_u32_wrapping_in(Prefix::Micro),
        Some(0u32.wrapping_sub(1000))
    );
    assert_eq!(
        Second::micro(1_999u32).to_u32_wrapping_in(Prefix::Milli),
        Some(1)
    );
    assert_eq!(
        Second::from_f32(-1.0).to_u32_wrapping_in(Prefix::Unit),
        Some(u32::MAX)
    );
    assert_eq!(
        Second::from_f32(1e10).to_u32_wrapping_in(Prefix::Unit),
        None
    );
}

#[test]
fn with_prefix_in_own_type() {
    let v = Volt::milli(3000u32).with_prefix(Prefix::Unit).unwrap();
    assert_eq!(v.value, NumericValue::U32(3));
    assert_eq!(Volt::milli(3300u32).with_prefix(Prefix::Unit), None);
    let v = Volt::base(-3i32).with_prefix(Prefix::Milli).unwrap();
    assert_eq!(v.value, NumericValue::I32(-3000));
    assert_eq!(Volt::base(5u32).with_prefix(Prefix::Nano), None);
    assert_eq!(Volt::base(-3i32).with_prefix(Prefix::Nano), None);
}

#[test]
fn exact_arithmetic() {
    assert_eq!(
        Second::milli(5u32).checked_add(&Second::micro(40u32)),
        Some(Second::micro(5040u32))
    );
    assert_eq!(
        Second::micro(u32::MAX).checked_add(&Second::micro(1u8)),
        None
    );
    assert_eq!(
        Volt::micro(1i32).checked_sub(&Volt::micro(2u8)),
        Some(Volt::micro(-1i32))
    );
    assert_eq!(Volt::micro(i32::MIN).checked_sub(&Volt::micro(1u8)), None);
    assert_eq!(
        Volt::milli(-5i32).checked_mul_int(-3),
        Some(Volt::milli(15i32))
    );
    assert_eq!(Volt::milli(u32::MAX).checked_mul_int(2), None);
    assert_eq!(Volt::milli(65_536u32).checked_mul_int(65_536u32), None);
    assert_eq!(
        Volt::milli(65_535u32).checked_mul_int(65_537u32),
        Some(Volt::milli(u32::MAX))
    );
    let t: Vec<_> = Second::micro(u32::MAX - 39)
        .steps(&Second::micro(40u8))
        .unwrap()
        .take(3)
        .collect();
    assert_eq!(
        t,
        [
            Second::micro(u32::MAX - 39),
            Second::micro(0u32),
            Second::micro(40u32)
        ]
    );
    assert_eq!(
        Second::micro(0u32).wrapping_sub(&Second::milli(1u8)),
        Some(Second::micro(u32::MAX - 999))
    );
}
