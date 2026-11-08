//! `Integer`: exact comparisons and arithmetic across number types, checked against `i128`, and the way back into a
//! number type (checked, saturating, wrapping). Runs with every number type (`just test`) and with the types up to 32
//! bits only (`--no-default-features --features u32,i32,f32`), where `Magnitude` is `u32` and every overflow decides
//! in `u32`. Scaling by powers of ten is exact_units' kernel's, tested in ww_si (`tests/kernel.rs`).
#![cfg(all(feature = "u32", feature = "i32", feature = "f32"))]

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

#[cfg(feature = "inib32")]
fn inib32_values() -> impl Iterator<Item = NumericValue> {
    I32S.iter()
        .map(|&x| NumericValue::INib32(ww_numeric::INib32(x)))
}
#[cfg(not(feature = "inib32"))]
fn inib32_values() -> impl Iterator<Item = NumericValue> {
    core::iter::empty()
}

fn values() -> impl Iterator<Item = NumericValue> {
    U32S.iter()
        .map(|&x| NumericValue::U32(x))
        .chain(I32S.iter().map(|&x| NumericValue::I32(x)))
        .chain([NumericValue::U8(0), NumericValue::U8(255)])
        .chain(inib32_values())
}

fn reference(v: &NumericValue) -> i128 {
    v.to_i128().unwrap()
}

#[test]
fn compare_matches_i128() {
    for a in values() {
        for b in values() {
            let expected = reference(&a).cmp(&reference(&b));
            let got = a.to_integer().unwrap().cmp(&b.to_integer().unwrap());
            assert_eq!(got, expected, "{a:?} vs {b:?}");
        }
    }
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
fn to_narrow() {
    assert_eq!(Integer::from(i32::MIN).to_i32(), Some(i32::MIN));
    assert_eq!(Integer::from(i32::MAX).to_i32(), Some(i32::MAX));
    assert_eq!(Integer::from(2_147_483_648u32).to_i32(), None);
    assert_eq!((-Integer::from(2_147_483_648u32)).to_i32(), Some(i32::MIN));
    assert_eq!((-Integer::from(2_147_483_649u32)).to_i32(), None);
    assert_eq!(Integer::from(u32::MAX).to_u32(), Some(u32::MAX));
    assert_eq!(Integer::from(-1i8).to_u32(), None);
}

#[test]
fn saturating_and_wrapping_into_number_types() {
    let big = Integer::from(u32::MAX);
    let minus = |m: u32| -Integer::from(m);
    let sat = |t: NumericBaseType, x: Integer| t.from_integer_saturating(x).unwrap();
    let wrap = |t: NumericBaseType, x: Integer| t.from_integer_wrapping(x).unwrap();
    assert_eq!(sat(NumericBaseType::U8, big), NumericValue::U8(255));
    assert_eq!(sat(NumericBaseType::U8, minus(1)), NumericValue::U8(0));
    assert_eq!(
        sat(NumericBaseType::U8, Integer::from(7u8)),
        NumericValue::U8(7)
    );
    assert_eq!(sat(NumericBaseType::I32, big), NumericValue::I32(i32::MAX));
    assert_eq!(
        sat(NumericBaseType::I32, minus(u32::MAX)),
        NumericValue::I32(i32::MIN)
    );
    assert_eq!(
        sat(NumericBaseType::I32, minus(2_147_483_648)),
        NumericValue::I32(i32::MIN)
    );
    assert_eq!(sat(NumericBaseType::U32, minus(5)), NumericValue::U32(0));
    // modulo 2^bits, two's complement below zero
    assert_eq!(
        wrap(NumericBaseType::U8, Integer::from(300u16)),
        NumericValue::U8(44)
    );
    assert_eq!(wrap(NumericBaseType::U8, minus(1)), NumericValue::U8(255));
    assert_eq!(
        wrap(NumericBaseType::U32, minus(1000)),
        NumericValue::U32(0u32.wrapping_sub(1000))
    );
    assert_eq!(wrap(NumericBaseType::I32, big), NumericValue::I32(-1));
    assert_eq!(
        wrap(NumericBaseType::I32, minus(2_147_483_649)),
        NumericValue::I32(i32::MAX)
    );
    // floats have no width to wrap at or range to clamp to here
    assert_eq!(NumericBaseType::F32.from_integer_wrapping(big), None);
    assert_eq!(NumericBaseType::F32.from_integer_saturating(big), None);
    // INib32 is i32 on the inside: the same range, saturating and wrapping as I32
    #[cfg(feature = "inib32")]
    {
        assert_eq!(
            sat(NumericBaseType::INib32, big),
            NumericValue::INib32(ww_numeric::INib32(i32::MAX))
        );
        assert_eq!(
            sat(NumericBaseType::INib32, minus(u32::MAX)),
            NumericValue::INib32(ww_numeric::INib32(i32::MIN))
        );
        assert_eq!(
            wrap(NumericBaseType::INib32, big),
            NumericValue::INib32(ww_numeric::INib32(-1))
        );
    }
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
    #[cfg(feature = "inib32")]
    assert_eq!(
        NumericValue::INib32(ww_numeric::INib32(i32::MAX)).wrapping_add(one),
        Some(NumericValue::INib32(ww_numeric::INib32(i32::MIN)))
    );
}
