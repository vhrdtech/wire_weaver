//! The kernel against the code it replaced (STD-102): the arithmetic of the old `ops` module (`i128` through the ratio
//! of two prefixes as a `Magnitude`, truncated toward zero, refused where `i128` overflowed), kept here as a reference.
//! Wherever the old code gave an exact answer for an integer, `to_in`, `rescaled_exact`, `==`/`<` and the bounds give
//! the same one, for every kind of prefix (SI, binary, `Pow10`, `Pow2`, `FullScale`); where the old code refused
//! (an `i128` overflow on the way), the new one may answer, never the other way round. Floats are pinned by the
//! fixed cases of `narrow.rs` and `quantities.rs`: their results changed only where the named rounding decides.
#![cfg(feature = "std")]

use core::cmp::Ordering;
use proptest::prelude::*;
use ww_si::exact_units::{FullScaleBits, Pow2Exp, Pow10Exp};
use ww_si::units;
use ww_si::ww_numeric::NumericValue;
use ww_si::{Overflow, Prefix, Quantity, RangeDec, Rounding, Volt};

/// The old `ratio_between` and `apply_truncating`: what a value in units of `from` is multiplied by in units of `to`,
/// as a fraction of `i128`s.
fn old_ratio(from: Prefix, to: Prefix) -> Option<(i128, i128)> {
    let (n, d) = from
        .magnitude()?
        .checked_div(to.magnitude()?)
        .ok()?
        .to_ratio()?;
    Some((i128::try_from(n).ok()?, i128::try_from(d).ok()?))
}

/// The old `rescale_slow` / `scale_i128` for an integer: `x · n / d` truncated toward zero, `None` on an `i128`
/// overflow (the SI path, `checked_scale_pow10`, gives the same truncated value or an overflow).
fn old_scaled(x: i128, from: Prefix, to: Prefix) -> Option<i128> {
    let (n, d) = old_ratio(from, to)?;
    Some(x.checked_mul(n)? / d)
}

/// The old `compare` / `compare_slow` for integers: `x · n` against `y · d`, `None` where `i128` overflowed (the old
/// code fell back to `f64` there, not exact).
fn old_cmp(a: Prefix, x: i128, b: Prefix, y: i128) -> Option<Ordering> {
    if a == b {
        return Some(x.cmp(&y));
    }
    let (n, d) = old_ratio(a, b)?;
    Some(x.checked_mul(n)?.cmp(&y.checked_mul(d)?))
}

fn prefix() -> impl Strategy<Value = Prefix> {
    prop_oneof![
        proptest::sample::select(Prefix::ALL.to_vec()),
        (-20i8..=20).prop_filter_map("an SI exponent", |e| Pow10Exp::new(e).map(Prefix::Pow10)),
        (-40i8..=40).prop_filter_map("a binary exponent", |e| Pow2Exp::new(e).map(Prefix::Pow2)),
        (2u8..=32).prop_map(|n| Prefix::FullScale(FullScaleBits::new(n).unwrap())),
    ]
}

fn value() -> impl Strategy<Value = NumericValue> {
    prop_oneof![
        any::<u8>().prop_map(NumericValue::U8),
        any::<u16>().prop_map(NumericValue::U16),
        any::<u32>().prop_map(NumericValue::U32),
        any::<i32>().prop_map(NumericValue::I32),
        any::<u64>().prop_map(NumericValue::U64),
        any::<i64>().prop_map(NumericValue::I64),
        (-5000i32..5000).prop_map(NumericValue::I32),
    ]
}

type Q = Quantity<units::Volt>;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn conversions_as_before(from in prefix(), to in prefix(), v in value()) {
        let q = Q::from_parts(from, v);
        let x = v.to_i128().unwrap();
        let t = (Rounding::Truncate, Overflow::Checked);
        if let Some(old) = old_scaled(x, from, to) {
            prop_assert_eq!(q.to_in::<i128>(to, t.0, t.1), Some(old));
            prop_assert_eq!(q.to_in::<u32>(to, t.0, t.1), u32::try_from(old).ok());
            prop_assert_eq!(q.to_in::<i32>(to, t.0, t.1), i32::try_from(old).ok());
            prop_assert_eq!(q.to_in::<i64>(to, t.0, t.1), i64::try_from(old).ok());
            // the old to_u32_wrapping_in: the truncated value modulo 2^32
            prop_assert_eq!(q.to_in::<u32>(to, Rounding::Truncate, Overflow::Wrap), Some(old as u32));
            // the old with_prefix: the same value in the same number type, if exact
            let exact = old_scaled(x, from, to).filter(|&y| old_scaled(y, to, from) == Some(x));
            let expected = exact.and_then(|y| v.ty().from_i128(y)).map(|v| Q::from_parts(to, v));
            prop_assert_eq!(q.rescaled_exact(to).map(|q| (q.prefix, q.value)), expected.map(|q| (q.prefix, q.value)));
        }
    }

    #[test]
    fn rounding_brackets_the_truncated_value(from in prefix(), to in prefix(), v in value()) {
        let q = Q::from_parts(from, v);
        let Some(old) = old_scaled(v.to_i128().unwrap(), from, to) else { return Ok(()) };
        let at = |r| q.to_in::<i128>(to, r, Overflow::Checked).unwrap();
        let (floor, ceil) = (at(Rounding::Floor), at(Rounding::Ceil));
        prop_assert!(floor <= old && old <= ceil && ceil - floor <= 1);
        for r in [Rounding::NearestEven, Rounding::NearestAway] {
            let n = at(r);
            prop_assert!(n == floor || n == ceil);
        }
    }

    #[test]
    fn comparisons_as_before(a in prefix(), x in value(), b in prefix(), y in value()) {
        let (p, q) = (Q::from_parts(a, x), Q::from_parts(b, y));
        if let Some(old) = old_cmp(a, x.to_i128().unwrap(), b, y.to_i128().unwrap()) {
            prop_assert_eq!(p.partial_cmp(&q), Some(old));
            prop_assert_eq!(p == q, old == Ordering::Equal);
        }
    }

    #[test]
    fn bounds_as_before(p in prefix(), v in value()) {
        // 0 V ..= 3.3 V: the old cmp_limit against 33 · 10^-1
        let inside = || {
            let x = v.to_i128()?;
            let low = old_cmp(p, x, Prefix::Unit, 0)?;
            let high = old_cmp(p, x, Prefix::Deci, 33)?;
            Some(low != Ordering::Less && high != Ordering::Greater)
        };
        if let Some(expected) = inside() {
            let r = Quantity::<units::Volt, RangeDec<0, 33, -1>>::try_new(p, v);
            prop_assert_eq!(r.is_ok(), expected);
        }
    }
}

/// A value exactly at a bound in another prefix, and one step past it, in every kind of prefix.
#[test]
fn at_the_bound_across_prefixes() {
    type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;
    assert!(Rail::try_new(Prefix::Milli, 3300u16).is_ok());
    assert!(Rail::try_new(Prefix::Milli, 3301u16).is_err());
    assert!(Rail::try_new(Prefix::Micro, 3_300_000u32).is_ok());
    assert!(Rail::try_new(Prefix::Micro, 3_300_001u32).is_err());
    // 3.3 V is not a whole number of 2^-10 V: 3379 · 2^-10 is 3.2998.., 3380 · 2^-10 is 3.3008..
    let pow2 = Prefix::Pow2(Pow2Exp::new(-10).unwrap());
    assert!(Rail::try_new(pow2, 3379u16).is_ok());
    assert!(Rail::try_new(pow2, 3380u16).is_err());
    // a 12-bit full scale of 1 V: 4095 / 4095 is exactly 1 V, inside; the bound at 0
    let fs = Prefix::FullScale(FullScaleBits::new(12).unwrap());
    assert!(Rail::try_new(fs, 4095u16).is_ok());
    assert!(Rail::try_new(fs, -1i32).is_err());
    assert!(Rail::try_new(Prefix::Kibi, 0u8).is_ok());
    assert!(Rail::try_new(Prefix::Kibi, 1u8).is_err());
    // an f32 at the bound: 3.3 as an f32 is 3.2999999523.. V, inside; the next f32 up is outside
    assert!(Rail::try_new(Prefix::Unit, 3.3f32).is_ok());
    assert!(Rail::try_new(Prefix::Unit, f32::from_bits(3.3f32.to_bits() + 1)).is_err());
    assert!(Rail::try_new(Prefix::Milli, 3300.0f32).is_ok());
    assert!(Rail::try_new(Prefix::Milli, 3300.0002f32).is_err());
}

/// Far apart prefixes and the ends of the widest integers: no overflow decides wrongly.
#[test]
fn far_apart() {
    let one = Volt::new(Prefix::Quetta, 1u8);
    let max = Volt::new(Prefix::Quecto, u64::MAX);
    assert!(one > max);
    assert!(Volt::new(Prefix::Quetta, -1i8) < Volt::new(Prefix::Quecto, i64::MIN));
    assert_eq!(
        Volt::new(Prefix::Quetta, 0u8),
        Volt::new(Prefix::Quecto, 0i64)
    );
    let p63 = Prefix::Pow10(Pow10Exp::new(63).unwrap());
    let m63 = Prefix::Pow10(Pow10Exp::new(-63).unwrap());
    assert!(Volt::new(p63, 1u8) > Volt::new(m63, u64::MAX));
    assert_eq!(
        Volt::new(p63, 1u8).to_in::<u128>(m63, Rounding::Truncate, Overflow::Saturate),
        Some(u128::MAX)
    );
    assert_eq!(
        Volt::new(m63, u64::MAX).to_in::<u64>(p63, Rounding::Ceil, Overflow::Checked),
        Some(1)
    );
}
