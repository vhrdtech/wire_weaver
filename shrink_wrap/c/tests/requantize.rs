//! The C requantize (`sw_requantize.c`, CLT-54) against `exact_units::Requantizer`: the same result, bit for bit,
//! for random factors (exact ratios up to 128 bits, fixed points of factors that aren't fractions of integers),
//! storages, policies and values, and for the edge values of every storage.

use exact_units::{Irrational, Magnitude, Overflow, Repr, Requantizer, Rounding, StorageKind};
use proptest::prelude::*;
use shrink_wrap_c::requantize::{CRequantizer, c_struct_sizes};

#[test]
fn struct_sizes_match() {
    for (c, rust) in c_struct_sizes() {
        assert_eq!(c, rust);
    }
}

fn rounding() -> impl Strategy<Value = Rounding> {
    prop_oneof![
        Just(Rounding::Truncate),
        Just(Rounding::Floor),
        Just(Rounding::Ceil),
        Just(Rounding::NearestEven),
        Just(Rounding::NearestAway),
    ]
}

fn overflow() -> impl Strategy<Value = Overflow> {
    prop_oneof![
        Just(Overflow::Checked),
        Just(Overflow::Saturate),
        Just(Overflow::Wrap)
    ]
}

fn storage() -> impl Strategy<Value = StorageKind> {
    prop_oneof![
        4 => (1..=64u32, any::<bool>()).prop_map(|(bits, signed)| StorageKind::Int { bits, signed }),
        1 => Just(StorageKind::F32),
        1 => Just(StorageKind::F64),
    ]
}

/// Ratios of small numbers, powers of 10 and 2 (up to 2^±60 and 10^±38: num and den past 64 and near 128 bits),
/// and factors with π, ln 2, ln 10 or a half exponent (fixed points).
fn factor() -> impl Strategy<Value = Magnitude> {
    let ratio = (1..2000u64, 1..2000u64)
        .prop_filter_map("a ratio", |(n, d)| Magnitude::checked_ratio(n, d).ok());
    let rest = (-38..=38i8, -60..=60i8, -2..=2i8);
    (ratio, rest, 0..4u8, -3..=3i8).prop_filter_map(
        "a magnitude",
        |(r, (p10, p2, pi), irr, half)| {
            let m = r
                .checked_mul(Magnitude::checked_pow10(p10)?)
                .ok()?
                .checked_mul(Magnitude::pow2(p2))
                .ok()?;
            let (pi, ln2, ln10) = match irr {
                0 => (0, 0, 0),
                1 => (pi * 2, 0, 0),
                2 => (0, half, 0),
                _ => (0, 0, half),
            };
            let irr = Irrational { pi, ln2, ln10 };
            let (pow10, pow2) = m.pow10_pow2_x2();
            Magnitude::from_parts_x2(pow10, pow2, irr, m.primes()).ok()
        },
    )
}

fn value() -> impl Strategy<Value = Repr> {
    prop_oneof![
        3 => any::<i64>().prop_map(|i| Repr::Int(i as i128)),
        2 => (-100_000..100_000i64).prop_map(|i| Repr::Int(i as i128)),
        2 => any::<u64>().prop_map(|i| Repr::Int(i as i128)),
        2 => any::<f64>().prop_map(Repr::Float),
        2 => (-1e6..1e6f64).prop_map(Repr::Float),
        1 => (-2000..2000i32).prop_map(|i| Repr::Float(i as f64 / 4.0)),
    ]
}

fn same(a: Option<Repr>, b: Option<Repr>) -> bool {
    match (a, b) {
        // NaN is NaN, by its bits
        (Some(Repr::Float(x)), Some(Repr::Float(y))) => x.to_bits() == y.to_bits(),
        (a, b) => a == b,
    }
}

fn check(
    f: &Magnitude,
    to: StorageKind,
    r: Rounding,
    o: Overflow,
    x: Repr,
) -> Result<(), TestCaseError> {
    let want = Requantizer::new(f, to, r, o).apply(x);
    let got = CRequantizer::new(f, to, r, o).apply(x);
    prop_assert!(
        same(got, want),
        "{x:?} * {f} into {to:?}, {r:?}, {o:?}: C {got:?}, Rust {want:?}"
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn c_requantizes_as_rust(f in factor(), to in storage(), r in rounding(), o in overflow(), x in value()) {
        check(&f, to, r, o, x)?;
    }
}

#[test]
fn edge_values() {
    let factors = [
        Magnitude::ratio(1, 1),
        Magnitude::ratio(1, 2),
        Magnitude::ratio(3, 2),
        Magnitude::ratio(7, 3),
        Magnitude::pow10(-4),
        Magnitude::pow10(3),
        Magnitude::pow10(38),
        Magnitude::pow10(-38),
        Magnitude::pow2(-63),
        Magnitude::pow2(63),
        Magnitude::pow2(-60).mul(Magnitude::pow10(-38)),
        Magnitude::pow2(60).mul(Magnitude::pow10(38)),
        Magnitude::new(1, 180000, 0, 0, 1),
        Magnitude::new(1, 1, 0, 0, 1),
    ];
    let mut values = vec![
        0i128, 1, -1, 2, -2, 3, -3, 5, -5, 127, -128, 255, 32767, -32768,
    ];
    for b in [8, 16, 31, 32] {
        values.extend([(1i128 << b) - 1, 1 << b, -(1i128 << b), -(1i128 << b) - 1]);
    }
    values.extend([
        i64::MAX as i128,
        1 << 63,
        u64::MAX as i128,
        i64::MIN as i128,
        i64::MIN as i128 + 1,
    ]);
    let floats = [
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.5,
        2.5,
        -2.5,
        1e300,
        -1e300,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        9.2e18,
        -9.3e18,
        1.8e19,
    ];
    let rs = [
        Rounding::Truncate,
        Rounding::Floor,
        Rounding::Ceil,
        Rounding::NearestEven,
        Rounding::NearestAway,
    ];
    let os = [Overflow::Checked, Overflow::Saturate, Overflow::Wrap];
    let mut storages = vec![StorageKind::F32, StorageKind::F64];
    for bits in [1, 4, 8, 16, 32, 63, 64] {
        storages.extend([
            StorageKind::Int {
                bits,
                signed: false,
            },
            StorageKind::Int { bits, signed: true },
        ]);
    }
    let xs: Vec<Repr> = values
        .iter()
        .map(|&i| Repr::Int(i))
        .chain(floats.iter().map(|&f| Repr::Float(f)))
        .collect();
    for f in &factors {
        for &to in &storages {
            for r in rs {
                for o in os {
                    let (rust, c) = (
                        Requantizer::new(f, to, r, o),
                        CRequantizer::new(f, to, r, o),
                    );
                    for &x in &xs {
                        let (want, got) = (rust.apply(x), c.apply(x));
                        assert!(
                            same(got, want),
                            "{x:?} * {f} into {to:?}, {r:?}, {o:?}: C {got:?}, Rust {want:?}"
                        );
                    }
                }
            }
        }
    }
}
