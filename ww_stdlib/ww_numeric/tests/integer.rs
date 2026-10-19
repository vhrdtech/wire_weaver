//! `Integer`: exact comparisons and powers of ten across number types, checked against `i128`. Runs with every number
//! type (`just test`) and with the types up to 32 bits only (`--no-default-features --features u32,i32,f32`), where
//! `Magnitude` is `u32` and every overflow decides in `u32`.
#![cfg(all(feature = "u32", feature = "i32", feature = "f32"))]

use core::cmp::Ordering;
use ww_numeric::{Integer, NumericBaseType, NumericValue};

/// Values around every edge the `u32` path has: 0, powers of ten, `u32::MAX / 10`, `i32` and `u32` limits.
const U32S: [u32; 14] = [
    0,
    1,
    9,
    10,
    99,
    100,
    999_999_999,
    1_000_000_000,
    429_496_729,
    429_496_730,
    2_147_483_647,
    2_147_483_648,
    4_294_967_294,
    u32::MAX,
];

const I32S: [i32; 10] = [
    0,
    -1,
    -9,
    -10,
    -1_000_000_000,
    -429_496_729,
    -429_496_730,
    -2_147_483_647,
    i32::MIN,
    i32::MAX,
];

const EXPS: [i8; 9] = [-12, -9, -6, -3, 0, 1, 3, 9, 10];

fn values() -> impl Iterator<Item = NumericValue> {
    U32S.iter()
        .map(|&x| NumericValue::U32(x))
        .chain(I32S.iter().map(|&x| NumericValue::I32(x)))
        .chain([NumericValue::U8(0), NumericValue::U8(255)])
}

fn reference(v: &NumericValue) -> i128 {
    v.to_i128().unwrap()
}

/// `x * 10^exp` in units of `10^-12`, exact for the values and powers above.
fn at_pico(x: i128, exp: i8) -> i128 {
    x * 10i128.pow((exp as i32 + 12) as u32)
}

#[test]
fn compare_matches_i128() {
    for a in values() {
        for b in values() {
            for &ae in &EXPS {
                for &be in &EXPS {
                    let expected = at_pico(reference(&a), ae).cmp(&at_pico(reference(&b), be));
                    let got = a
                        .to_integer()
                        .unwrap()
                        .cmp_pow10(ae, b.to_integer().unwrap(), be);
                    assert_eq!(got, expected, "{a:?}e{ae} vs {b:?}e{be}");
                }
            }
        }
    }
}

#[test]
fn compare_far_apart() {
    let one = Integer::from(1u8);
    let max = Integer::from(u32::MAX);
    // 1e60 against u32::MAX: the overflow decides
    assert_eq!(one.cmp_pow10(30, max, -30), Ordering::Greater);
    assert_eq!(max.cmp_pow10(-30, one, 30), Ordering::Less);
    assert_eq!((-one).cmp_pow10(30, -max, -30), Ordering::Less);
    assert_eq!((-max).cmp_pow10(-30, -one, 30), Ordering::Greater);
    // the extreme powers of an i8
    assert_eq!(one.cmp_pow10(i8::MAX, max, i8::MIN), Ordering::Greater);
    assert_eq!(
        one.cmp_pow10(i8::MIN, Integer::ZERO, i8::MAX),
        Ordering::Greater
    );
    // zero at any power
    assert_eq!(
        Integer::ZERO.cmp_pow10(30, Integer::ZERO, -30),
        Ordering::Equal
    );
    assert_eq!(Integer::ZERO.cmp_pow10(30, -one, -30), Ordering::Greater);
    assert_eq!(Integer::ZERO.cmp_pow10(-30, one, 30), Ordering::Less);
}

#[test]
fn signs_and_zero() {
    assert_eq!(Integer::new(true, 0), Integer::ZERO);
    assert!(!(-Integer::ZERO).is_negative());
    assert_eq!(Integer::from(-5i32).magnitude(), 5);
    assert!(Integer::from(-5i32) < Integer::from(0u8));
    assert!(Integer::from(-5i32) < Integer::from(-4i8));
    assert!(Integer::from(u32::MAX) > Integer::from(i32::MAX));
    assert!(Integer::from(i32::MIN) < Integer::from(i32::MIN + 1));
}

#[test]
fn scale() {
    let x = Integer::from(3999u16);
    assert_eq!(x.checked_scale_pow10(-3), Some(Integer::from(3u8)));
    assert_eq!(x.exact_scale_pow10(-3), None);
    assert_eq!(
        Integer::from(3000u16).exact_scale_pow10(-3),
        Some(Integer::from(3u8))
    );
    // truncated toward zero below zero too
    assert_eq!(
        Integer::from(-3999i32).checked_scale_pow10(-3),
        Some(Integer::from(-3i8))
    );
    assert_eq!(
        Integer::from(u32::MAX).checked_scale_pow10(-100),
        Some(Integer::ZERO)
    );
    assert_eq!(Integer::from(u32::MAX).exact_scale_pow10(-100), None);
    assert_eq!(Integer::ZERO.exact_scale_pow10(-100), Some(Integer::ZERO));
    assert_eq!(Integer::ZERO.checked_scale_pow10(100), Some(Integer::ZERO));
    assert_eq!(
        Integer::from(1u8).checked_scale_pow10(9),
        Some(Integer::from(1_000_000_000u32))
    );
    // 4_294_967_290 fits u32, 42_949_672_960 needs more than 32 bits
    let x = Integer::from(429_496_729u32);
    assert_eq!(
        x.checked_scale_pow10(1),
        Some(Integer::from(4_294_967_290u32))
    );
    let y = Integer::from(429_496_730u32).checked_scale_pow10(1);
    assert_eq!(y.and_then(Integer::to_u32), None);
}

#[test]
fn to_narrow() {
    assert_eq!(Integer::from(i32::MIN).to_i32(), Some(i32::MIN));
    assert_eq!(Integer::from(i32::MAX).to_i32(), Some(i32::MAX));
    assert_eq!(Integer::from(2_147_483_648u32).to_i32(), None);
    assert_eq!((-Integer::from(2_147_483_648u32)).to_i32(), Some(i32::MIN));
    assert_eq!((-Integer::from(2_147_483_649u32)).to_i32(), None);
    assert_eq!(Integer::from(u32::MAX).to_u32(), Some(u32::MAX));
    assert_eq!(Integer::from(-1i8).to_u32(), None);
    assert_eq!(Integer::from(-1i8).to_u32_wrapping_pow10(0), u32::MAX);
    assert_eq!(
        Integer::from(-1i8).to_u32_wrapping_pow10(3),
        0u32.wrapping_sub(1000)
    );
    assert_eq!(
        Integer::from(u32::MAX).to_u32_wrapping_pow10(1),
        u32::MAX.wrapping_mul(10)
    );
    assert_eq!(
        Integer::from(u32::MAX).to_u32_wrapping_pow10(-1),
        u32::MAX / 10
    );
}

#[test]
fn arithmetic() {
    let a = Integer::from(5u8);
    let b = Integer::from(-7i8);
    assert_eq!(a.checked_add(b), Some(Integer::from(-2i8)));
    assert_eq!(b.checked_add(a), Some(Integer::from(-2i8)));
    assert_eq!(a.checked_sub(b), Some(Integer::from(12u8)));
    assert_eq!(b.checked_sub(b), Some(Integer::ZERO));
    assert_eq!(a.checked_mul(b), Some(Integer::from(-35i8)));
    assert_eq!(b.checked_mul(b), Some(Integer::from(49u8)));
    // magnitudes up to u32::MAX always add without overflow in a wider build; in a u32 one they overflow, and the
    // sum fits no enabled type either
    let max = Integer::from(u32::MAX);
    assert_eq!(
        max.checked_add(Integer::from(1u8))
            .and_then(Integer::to_u32),
        None
    );
    assert_eq!(max.checked_add(-max), Some(Integer::ZERO));
}

#[test]
fn round_trip_through_number_types() {
    for v in values() {
        let x = v.to_integer().unwrap();
        assert_eq!(v.ty().from_integer(x), Some(v));
    }
    let big = Integer::from(u32::MAX);
    assert_eq!(NumericBaseType::U8.from_integer(big), None);
    assert_eq!(NumericBaseType::I32.from_integer(big), None);
    assert_eq!(NumericBaseType::U32.from_integer(Integer::from(-1i8)), None);
    assert_eq!(
        NumericBaseType::F32.from_integer(Integer::from(-3i8)),
        Some(NumericValue::F32(-3.0))
    );
    assert_eq!(NumericValue::F32(1.0).to_integer(), None);
}

#[test]
fn wrapping_add() {
    let one = Integer::from(1u8);
    assert_eq!(
        NumericValue::U32(u32::MAX).wrapping_add(Integer::from(2u8)),
        Some(NumericValue::U32(1))
    );
    assert_eq!(
        NumericValue::U32(0).wrapping_add(-one),
        Some(NumericValue::U32(u32::MAX))
    );
    assert_eq!(
        NumericValue::U8(250).wrapping_add(Integer::from(10u8)),
        Some(NumericValue::U8(4))
    );
    // the step bigger than the type: modulo 256
    assert_eq!(
        NumericValue::U8(0).wrapping_add(Integer::from(300u16)),
        Some(NumericValue::U8(44))
    );
    assert_eq!(
        NumericValue::I32(i32::MAX).wrapping_add(one),
        Some(NumericValue::I32(i32::MIN))
    );
    assert_eq!(
        NumericValue::I32(i32::MIN).wrapping_add(-one),
        Some(NumericValue::I32(i32::MAX))
    );
    assert_eq!(
        NumericValue::I32(-5).wrapping_add(Integer::from(u32::MAX)),
        Some(NumericValue::I32(-6))
    );
    assert_eq!(NumericValue::F32(1.0).wrapping_add(one), None);
}
