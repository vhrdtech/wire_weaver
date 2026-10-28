//! Rounding conversions and overflow policies (STD-63): every mode against a reference written independently of the
//! crate's own (Euclidean division), the typed conversion against the run-time one, factors with π.

use exact_units::prefix::{Centi, Deci, Kilo, Micro, Milli, One, Pow2, Rational};
use exact_units::units::{Radian, Ratio, Second, Volt};
use exact_units::{
    Bam16, Fixed, FixedScale, Magnitude, Overflow, Q15, Range, Repr, Requantizer, Rounding,
    Storage, StorageKind, requantize,
};

const MODES: [Rounding; 5] = [
    Rounding::Truncate,
    Rounding::Floor,
    Rounding::Ceil,
    Rounding::NearestEven,
    Rounding::NearestAway,
];

/// `p / d` rounded by `r`, from the Euclidean floor.
fn reference(p: i128, d: i128, r: Rounding) -> i128 {
    let f = p.div_euclid(d);
    let rem = p - f * d; // 0 <= rem < d
    if rem == 0 {
        return f;
    }
    let c = f + 1;
    match r {
        Rounding::Floor => f,
        Rounding::Ceil => c,
        Rounding::Truncate => {
            if p < 0 {
                c
            } else {
                f
            }
        }
        Rounding::NearestEven | Rounding::NearestAway => match (2 * rem).cmp(&d) {
            core::cmp::Ordering::Less => f,
            core::cmp::Ordering::Greater => c,
            core::cmp::Ordering::Equal => match r {
                Rounding::NearestAway => {
                    if p < 0 {
                        f
                    } else {
                        c
                    }
                }
                _ => {
                    if f % 2 == 0 {
                        f
                    } else {
                        c
                    }
                }
            },
        },
    }
}

/// `x` into `min..=max` by `o`, wrapping modulo the span.
fn settle(x: i128, min: i128, max: i128, o: Overflow) -> Option<i128> {
    match o {
        Overflow::Checked => (min..=max).contains(&x).then_some(x),
        Overflow::Saturate => Some(x.clamp(min, max)),
        Overflow::Wrap => Some((x - min).rem_euclid(max - min + 1) + min),
    }
}

/// Every i16 from scale `P` to `P2` in `T2`, every mode and policy: the typed conversion, the reference and the
/// run-time one agree.
fn exhaustive<P: FixedScale, P2: FixedScale, T2: Storage + Into<i128>>(num: i128, den: i128) {
    let factor = P::SCALE.div(P2::SCALE);
    assert_eq!(factor.to_ratio(), Some((num as u128, den as u128)));
    let to = StorageKind::of::<T2>();
    let policies = [Overflow::Checked, Overflow::Saturate, Overflow::Wrap];
    let dynamic: Vec<Vec<Requantizer>> = MODES
        .iter()
        .map(|&r| {
            policies
                .iter()
                .map(|&o| Requantizer::new(&factor, to, r, o))
                .collect()
        })
        .collect();
    for x in i16::MIN..=i16::MAX {
        let v: Fixed<Volt, P, i16> = Fixed::new(x);
        for (ri, r) in MODES.into_iter().enumerate() {
            let q = reference(x as i128 * num, den, r);
            for (oi, o) in policies.into_iter().enumerate() {
                let want = settle(q, T2::MIN_INT, T2::MAX_INT, o);
                let got = v.requantize::<P2, T2>(r, o).map(|f| f.get().into());
                assert_eq!(got, want, "{x} {r:?} {o:?}");
                let dynamic = dynamic[ri][oi].apply(Repr::Int(x as i128));
                assert_eq!(dynamic, want.map(Repr::Int), "run time: {x} {r:?} {o:?}");
            }
            if x % 61 == 0 {
                // the named forms are requantize with one policy each
                assert_eq!(
                    v.to_rounded::<P2, T2>(r).map(|f| f.get().into()),
                    settle(q, T2::MIN_INT, T2::MAX_INT, Overflow::Checked)
                );
                assert_eq!(
                    v.to_saturating::<P2, T2>(r).get().into(),
                    settle(q, T2::MIN_INT, T2::MAX_INT, Overflow::Saturate).unwrap()
                );
                assert_eq!(
                    v.to_wrapping::<P2, T2>(r).get().into(),
                    settle(q, T2::MIN_INT, T2::MAX_INT, Overflow::Wrap).unwrap()
                );
            }
        }
    }
}

// every i16, one test per pair of scales so they run in parallel (each about 0.2 s in debug)

/// A division in i16's own width: mV to 0.1 V.
#[test]
fn every_i16_milli_to_deci() {
    exhaustive::<Milli, Deci, i8>(1, 100);
}

#[test]
fn every_i16_milli_to_one() {
    exhaustive::<Milli, One, i8>(1, 1000);
}

#[test]
fn every_i16_milli_to_centi_unsigned() {
    exhaustive::<Milli, Centi, u8>(1, 10);
}

/// A fraction: the multiply and divide in 64 bits.
#[test]
fn every_i16_fraction() {
    exhaustive::<Rational<7, 3>, One, i16>(7, 3);
}

#[test]
fn every_i16_power_of_two() {
    exhaustive::<Pow2<-15>, Pow2<-8>, i8>(1, 128);
}

/// Whole factors, past the target's range.
#[test]
fn every_i16_whole_factor() {
    exhaustive::<Milli, Micro, i16>(1000, 1);
    exhaustive::<Kilo, One, u16>(1000, 1);
}

#[test]
fn every_i16_duty() {
    exhaustive::<Rational<1, 65535>, Rational<1, 255>, u8>(1, 257);
}

#[test]
fn wide_storages() {
    // u64 µs to s: the division in u64, every mode
    let t: Fixed<Second, Micro, u64> = Fixed::new(u64::MAX);
    let s = |r| t.to_rounded::<One, u64>(r).unwrap().get();
    assert_eq!(s(Rounding::Truncate), 18_446_744_073_709);
    assert_eq!(s(Rounding::Ceil), 18_446_744_073_710);
    assert_eq!(s(Rounding::NearestEven), 18_446_744_073_710); // .551615
    // i64 by 7/3 in 128 bits
    let x: Fixed<Ratio, Rational<7, 3>, i64> = Fixed::new(i64::MIN);
    let want = reference(i64::MIN as i128 * 7, 3, Rounding::NearestEven);
    assert_eq!(
        x.to_saturating::<One, i64>(Rounding::NearestEven).get(),
        i64::MIN
    );
    assert_eq!(
        x.to_wrapping::<One, i64>(Rounding::NearestEven).get() as i128,
        settle(want, i64::MIN as i128, i64::MAX as i128, Overflow::Wrap).unwrap()
    );
    assert_eq!(x.to_rounded::<One, i64>(Rounding::NearestEven), None);
}

#[test]
fn floats_round_once() {
    let v: Fixed<Volt, One, f32> = Fixed::new(2.5);
    let r = |r| v.to_rounded::<One, i16>(r).map(Fixed::get);
    assert_eq!(r(Rounding::NearestEven), Some(2));
    assert_eq!(r(Rounding::NearestAway), Some(3));
    assert_eq!(r(Rounding::Floor), Some(2));
    let v: Fixed<Volt, One, f64> = Fixed::new(-2.5);
    let r = |r| v.to_rounded::<One, i16>(r).map(Fixed::get);
    assert_eq!(r(Rounding::NearestEven), Some(-2));
    assert_eq!(r(Rounding::NearestAway), Some(-3));
    assert_eq!(r(Rounding::Truncate), Some(-2));
    assert_eq!(r(Rounding::Ceil), Some(-2));
    assert_eq!(r(Rounding::Floor), Some(-3));
    // mV in f32 to V in i8: 1499.9 mV is 1 V to nearest
    let v: Fixed<Volt, Milli, f32> = Fixed::new(1499.9);
    assert_eq!(
        v.to_rounded::<One, i8>(Rounding::NearestEven)
            .map(Fixed::get),
        Some(1)
    );
    // out of range and NaN by policy
    let big: Fixed<Volt, One, f64> = Fixed::new(1e10);
    assert_eq!(big.to_rounded::<One, i16>(Rounding::Truncate), None);
    assert_eq!(
        big.to_saturating::<One, i16>(Rounding::Truncate).get(),
        i16::MAX
    );
    assert_eq!(
        big.to_wrapping::<One, i16>(Rounding::Truncate).get(),
        10_000_000_000i64 as i16
    );
    let nan: Fixed<Volt, One, f64> = Fixed::new(f64::NAN);
    assert_eq!(nan.to_rounded::<One, i16>(Rounding::Truncate), None);
    assert_eq!(nan.to_saturating::<One, i16>(Rounding::Truncate).get(), 0);
    assert_eq!(nan.to_wrapping::<One, u8>(Rounding::Truncate).get(), 0);
    let top: Fixed<Volt, One, f64> = Fixed::new(1.8446744073709552e19); // 2⁶⁴
    assert_eq!(
        top.to_saturating::<One, u64>(Rounding::Truncate).get(),
        u64::MAX
    );
    // integers into a float aren't rounded: the float's own rounding
    let n: Fixed<Volt, Milli, i32> = Fixed::new(1);
    assert_eq!(
        n.to_rounded::<One, f64>(Rounding::Floor).map(Fixed::get),
        Some(0.001)
    );
}

exact_units::scale! {
    /// Steps of 1/1000 °, over the unit radian: the scale is π/180000, not a fraction of integers.
    struct MilliDegree = "m°", Exact(1 / 180000, pi 1);
}

#[test]
fn fixed_points() {
    // the top 64 bits of each, rounded to nearest
    assert_eq!(Magnitude::PI.fixed_point(), (0xc90fdaa22168c235, -62));
    assert_eq!(Magnitude::LN2.fixed_point(), (0xb17217f7d1cf79ac, -64));
    assert_eq!(Magnitude::LN10.fixed_point(), (0x935d8dddaaa8ac17, -62));
    assert_eq!(
        Magnitude::pow2(1).sqrt().fixed_point(),
        (0xb504f333f9de6484, -63)
    );
    assert_eq!(
        Magnitude::ratio(1, 3).fixed_point(),
        (0xaaaaaaaaaaaaaaab, -65)
    );
    assert_eq!(Magnitude::ONE.fixed_point(), (1 << 63, -63));
    assert_eq!(Magnitude::pow10(-6).fixed_point().0, 0x8637bd05af6c69b6);
}

#[test]
fn degrees_to_radians() {
    // an integer of m° to µrad and back: a factor with π, rounded, never a compile error with to_rounded
    for x in (-360_000i32..=360_000).step_by(7) {
        let deg: Fixed<Radian, MilliDegree, i32> = Fixed::new(x);
        let urad = x as f64 * core::f64::consts::PI / 180_000.0 * 1e6;
        for r in MODES {
            let got = deg.to_rounded::<Micro, i32>(r).unwrap().get();
            let want = match r {
                Rounding::Truncate => urad.trunc(),
                Rounding::Floor => urad.floor(),
                Rounding::Ceil => urad.ceil(),
                Rounding::NearestEven => urad.round_ties_even(),
                Rounding::NearestAway => urad.round(),
            };
            // f64 is within 1e-9 of the truth here, nowhere near a rounding step for x != 0
            assert_eq!(got as f64, want, "{x} m° {r:?}");
        }
        let back: Fixed<Radian, MilliDegree, i32> = deg
            .to_rounded::<Micro, i32>(Rounding::NearestEven)
            .unwrap()
            .to_rounded(Rounding::NearestEven)
            .unwrap();
        // 1 µrad is 0.057 m°: back within one step
        assert!((back.get() - x).abs() <= 1, "{x}");
    }
    // the run-time path takes the same fixed point
    let factor = MilliDegree::SCALE.div(Micro::SCALE);
    for x in [-1_000_000i64, -1, 0, 1, 45_000, 90_000, i32::MAX as i64] {
        for r in MODES {
            let typed = Fixed::<Radian, MilliDegree, i64>::new(x)
                .to_rounded::<Micro, i64>(r)
                .map(|f| Repr::Int(f.get() as i128));
            let dynamic = requantize(
                Repr::Int(x as i128),
                &factor,
                StorageKind::of::<i64>(),
                r,
                Overflow::Checked,
            );
            assert_eq!(typed, dynamic, "{x} {r:?}");
        }
    }
}

exact_units::scale! {
    /// Steps of 1e-9 °: a radian is 5.7·10¹⁰ of them, a factor of more than 2³².
    struct NanoDegree = "n°", Exact(1 / 180000000000, pi 1);
}

/// `x` from `P` to `P2` in an i32 by every policy, through the 32-bit path and through the 64-bit one.
fn narrow_against_wide<P: FixedScale, P2: FixedScale>() {
    let xs = (i32::MIN..=i32::MAX)
        .step_by(1_048_583)
        .chain([i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX])
        .chain(-5_000..=5_000);
    for x in xs {
        for r in MODES {
            for o in [Overflow::Checked, Overflow::Saturate, Overflow::Wrap] {
                let narrow = Fixed::<Radian, P, i32>::new(x)
                    .requantize::<P2, i32>(r, o)
                    .map(Fixed::get);
                let wide = Fixed::<Radian, P, i64>::new(x as i64)
                    .requantize::<P2, i32>(r, o)
                    .map(Fixed::get);
                assert_eq!(narrow, wide, "{x} {r:?} {o:?}");
            }
        }
    }
}

#[test]
fn narrow_fixed_point() {
    // storages of up to 32 bits settle in 64 bits: the same results as in 128
    narrow_against_wide::<MilliDegree, Micro>();
    narrow_against_wide::<Micro, MilliDegree>();
    // a factor past 2³²: the product's quotient doesn't fit the short path
    narrow_against_wide::<One, NanoDegree>();
    narrow_against_wide::<NanoDegree, One>();
}

#[test]
fn policies_by_kind() {
    // binary angles wrap: 3/4 turn + 1/2 turn is 1/4 turn
    let a = Bam16::of(49152) + Bam16::of(32768);
    assert_eq!(a.get(), 16384);
    let mut b = Bam16::of(0);
    b -= Bam16::of(1);
    assert_eq!(b.get(), u16::MAX);
    assert_eq!((Bam16::of(16384) * 5).get(), 16384);
    // Q formats saturate: 0.75 + 0.75 is the largest Q15, -1 / -1 too
    let q: Q15 = Fixed::of(24576);
    assert_eq!((q + q).get(), i16::MAX);
    let min: Q15 = Fixed::of(i16::MIN);
    assert_eq!((min / -1).get(), i16::MAX);
    assert_eq!([q, q, q].into_iter().sum::<Q15>().get(), i16::MAX);
    // the overflow policy is only in the type: a free conversion between them
    let plain = q.with_overflow::<exact_units::Unbounded>();
    assert_eq!(plain.checked_add(plain), None);
    assert_eq!(
        plain
            .with_overflow::<exact_units::Wrapping>()
            .wrapping_add(plain.with_overflow())
            .get(),
        -16384
    );
}

#[test]
fn bounds_prove_conversions() {
    // a 0..=3300 mV rail in µV fits an i32
    let v = Fixed::<Volt, Milli, i16, Range<0, 3300>>::bounded(3300);
    let uv: Fixed<Volt, Micro, i32> = v.into();
    assert_eq!(uv.get(), 3_300_000);
    // and rounds into a u8 of 0.1 V steps; every mode, the ends of the range
    for r in MODES {
        let lo = Fixed::<Volt, Milli, i16, Range<0, 3300>>::bounded(0);
        let dv: Fixed<Volt, Deci, u8> = lo.into_rounded(r);
        assert_eq!(dv.get(), 0);
        let dv: Fixed<Volt, Deci, u8> = v.into_rounded(r);
        assert_eq!(dv.get(), 33);
    }
    let mid = Fixed::<Volt, Milli, i16, Range<0, 3300>>::bounded(1250);
    let dv: Fixed<Volt, Deci, u8> = mid.into_rounded(Rounding::NearestEven);
    assert_eq!(dv.get(), 12);
    let dv: Fixed<Volt, Deci, u8> = mid.into_rounded(Rounding::NearestAway);
    assert_eq!(dv.get(), 13);
    // without bounds the storage is the range: any u8 of ms is a u32 of µs
    let ms: Fixed<Second, Milli, u8> = Fixed::new(255);
    let us: Fixed<Second, Micro, u32> = ms.into_rounded(Rounding::Truncate);
    assert_eq!(us.get(), 255_000);
}
