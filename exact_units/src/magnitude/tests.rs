use super::*;
use std::string::ToString;

/// The pound, 0.45359237 kg exactly (1959 yard and pound agreement).
const LB: Magnitude = Magnitude::ratio(45359237, 100_000_000);
/// Standard gravity, 9.80665 m/s² (CGPM 1901).
const GN: Magnitude = Magnitude::ratio(980665, 100_000);
/// Pound-force: lb · g_n.
const LBF: Magnitude = LB.mul(GN);
/// The inch, 0.0254 m.
const IN: Magnitude = Magnitude::ratio(254, 10_000);
/// psi = lbf / in².
const PSI: Magnitude = LBF.div(IN.powi(2));

#[test]
fn equal_however_spelled() {
    assert_eq!(Magnitude::pow10(-3), Magnitude::ratio(1, 1000));
    assert_eq!(Magnitude::ratio(2000, 2_000_000), Magnitude::pow10(-3));
    assert_eq!(Magnitude::ratio(1024, 1), Magnitude::pow2(10));
    assert_eq!(
        Magnitude::ratio(5, 1),
        Magnitude::pow10(1).div(Magnitude::pow2(1))
    );
    assert_eq!(
        Magnitude::new(1, 180, 0, 0, 1),
        Magnitude::new(2, 360, 0, 0, 1)
    );
    assert_eq!(
        Magnitude::PI.div(Magnitude::ratio(180, 1)),
        Magnitude::new(1, 180, 0, 0, 1)
    );
    assert_eq!(
        Magnitude::pow10(-3).div(Magnitude::pow10(-6)),
        Magnitude::pow10(3)
    );
    assert_eq!(Magnitude::pow10(0), Magnitude::ONE);
    assert_eq!(Magnitude::ratio(7, 7), Magnitude::ONE);
}

#[test]
fn factors() {
    let duty = Magnitude::ratio(1, 65535);
    // 65535 = 3 · 5 · 17 · 257
    let p: Vec<_> = duty.primes().map(|f| (f.prime, f.exp)).collect();
    assert_eq!(p, [(3, -2), (17, -2), (257, -2)]);
    assert_eq!(duty.exp_x2(5), -2);
    assert_eq!(duty.exp_x2(2), 0);
    // 45359237 = 7 · 11 · 97 · 6073
    let p: Vec<_> = LB.primes().map(|f| f.prime).collect();
    assert_eq!(p, [7, 11, 97, 6073]);
    assert_eq!((LB.exp_x2(2), LB.exp_x2(5)), (-16, -16));
    // US survey foot 1200/3937 m, 3937 = 31 · 127
    let survey_ft = Magnitude::ratio(1200, 3937);
    assert_eq!(survey_ft.exp_x2(31), -2);
    assert_eq!(survey_ft.exp_x2(127), -2);
    assert_eq!(survey_ft.exp_x2(3), 2);
    // eV 1.602176634e-19 J: 1602176634 = 2 · 3² · 19 · 389 · 12043
    let ev = Magnitude::ratio(1602176634, 1).mul(Magnitude::pow10(-28));
    let p: Vec<_> = ev.primes().map(|f| (f.prime, f.exp)).collect();
    assert_eq!(p, [(3, 4), (19, 2), (389, 2), (12043, 2)]);
    assert_eq!(ev.to_ratio(), Some((801088317, 5 * 10u128.pow(27))));
    // g_n = 5 · 7 · 28019 · 10⁻⁵ · ... : 980665 = 5 · 7 · 28019
    assert_eq!(GN.exp_x2(28019), 2);
}

#[test]
fn closed_under_products() {
    // psi² and (lbf·s)³ overflow u64 / u128 as num/den, not as prime powers
    let psi2 = PSI.powi(2);
    assert_eq!(psi2.div(PSI), PSI);
    let cube = LBF.powi(3);
    assert_eq!(cube.to_ratio(), None, "(lbf·s)³ doesn't fit u128");
    assert_eq!(cube.div(LBF).div(LBF), LBF);
    assert_eq!(cube.exp_x2(28019), 6);
    // x · y / y = x for every pair
    let all = [
        LB,
        GN,
        LBF,
        IN,
        PSI,
        Magnitude::PI,
        Magnitude::LN2,
        Magnitude::ratio(1, 65535),
    ];
    for a in all {
        for b in all {
            assert_eq!(a.mul(b).div(b), a);
            assert_eq!(a.div(b).mul(b), a);
            assert_eq!(a.mul(b), b.mul(a));
        }
        assert_eq!(a.mul(a.recip()), Magnitude::ONE);
    }
}

#[test]
fn irrational_and_roots() {
    let deg = Magnitude::new(1, 180, 0, 0, 1);
    assert_eq!(deg.irrational_x2().pi, 2);
    assert!(!deg.is_rational());
    assert_eq!(Magnitude::PI.powi(-2).irrational_x2().pi, -4);
    let sqrt2 = Magnitude::ratio(2, 1).sqrt();
    assert_eq!(sqrt2.mul(sqrt2), Magnitude::ratio(2, 1));
    assert_eq!(sqrt2.to_ratio(), None);
    assert_eq!(sqrt2.checked_sqrt(), Err(MagnitudeError::NotSquare));
    // √(mHz): 10^(-3/2)
    let sqrt_milli = Magnitude::pow10(-3).sqrt();
    assert_eq!(sqrt_milli.powi(2), Magnitude::pow10(-3));
    assert_eq!(sqrt_milli.exp_x2(5), -3);
    assert!((sqrt_milli.to_f64() - 0.001f64.sqrt()).abs() < 1e-17);
    // a decibel in neper: ln10 / 20; a decade in octaves: ln10 / ln2
    let db = Magnitude::LN10.div(Magnitude::ratio(20, 1));
    assert!((db.to_f64() - core::f64::consts::LN_10 / 20.0).abs() < 1e-16);
    let decade = Magnitude::LN10.div(Magnitude::LN2);
    assert!((decade.to_f64() - core::f64::consts::LOG2_10).abs() < 1e-15);
    assert_eq!(
        decade.irrational_x2(),
        Irrational {
            pi: 0,
            ln2: -2,
            ln10: 2
        }
    );
}

#[test]
fn large_and_refused() {
    // two primes above 2¹⁶: found by Pollard's rho
    let m = Magnitude::ratio(65537 * 65539, 1);
    assert_eq!((m.exp_x2(65537), m.exp_x2(65539)), (2, 2));
    let m = Magnitude::ratio(65537 * 65539 * 65543, 1);
    assert_eq!(m.exp_x2(65543), 2);
    // the largest u32 prime is a factor, a larger one is refused
    assert_eq!(Magnitude::ratio(4294967291, 1).exp_x2(4294967291), 2);
    assert_eq!(
        Magnitude::checked_ratio((1 << 61) - 1, 1),
        Err(MagnitudeError::PrimeTooLarge)
    );
    assert_eq!(Magnitude::checked_ratio(0, 1), Err(MagnitudeError::Zero));
    assert_eq!(Magnitude::checked_ratio(1, 0), Err(MagnitudeError::Zero));
    // 13 primes other than 2 and 5
    let n: u64 = [3u64, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47]
        .iter()
        .product();
    assert_eq!(
        Magnitude::checked_ratio(n, 1),
        Err(MagnitudeError::TooManyFactors)
    );
    assert!(Magnitude::checked_ratio(n / 47, 1).is_ok());
    assert_eq!(Magnitude::checked_pow10(64), None);
    assert!(Magnitude::checked_pow10(-63).is_some());
    assert_eq!(
        Magnitude::pow10(40).checked_mul(Magnitude::pow10(30)),
        Err(MagnitudeError::ExponentRange)
    );
    assert_eq!(u64::MAX, 3 * 5 * 17 * 257 * 641 * 65537 * 6700417);
    let m = Magnitude::ratio(u64::MAX, 1);
    assert_eq!(m.to_ratio(), Some((u64::MAX as u128, 1)));
}

#[test]
fn primality() {
    let small: Vec<u64> = (0..100).filter(|&n| is_prime(n)).collect();
    assert_eq!(
        small,
        [
            2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83,
            89, 97
        ]
    );
    assert!(is_prime((1 << 61) - 1));
    assert!(!is_prime(3215031751)); // strong pseudoprime to 2, 3, 5, 7
    assert!(!is_prime(65537 * 65539));
}

#[test]
fn apply() {
    let duty = Magnitude::ratio(1, 65535);
    assert_eq!(duty.recip().apply_int(1), Some(65535));
    assert_eq!(duty.apply_int(65535), Some(1));
    assert_eq!(duty.apply_int(32767), None);
    assert_eq!(Magnitude::pow2(10).apply_int(3), Some(3072));
    assert_eq!(Magnitude::pow10(-3).apply_int(1500), None);
    assert_eq!(Magnitude::pow10(-3).apply_int(-3000), Some(-3));
    assert_eq!(Magnitude::PI.apply_int(1), None);
    let deg = Magnitude::new(1, 180, 0, 0, 1);
    assert!((deg.apply_f64(90.0) - core::f64::consts::FRAC_PI_2).abs() < 1e-15);
    assert!((deg.to_f64() - core::f64::consts::PI / 180.0).abs() < 1e-18);
    assert_eq!(Magnitude::pow10(-1).apply_f64(30.0), 3.0);
    assert_eq!(LB.to_ratio(), Some((45359237, 100_000_000)));
    assert_eq!(Magnitude::pow10(-3).as_pow10(), Some(-3));
    assert_eq!(Magnitude::pow2(10).as_pow10(), None);
    assert_eq!(Magnitude::pow10(-3).sqrt().as_pow10(), None);
}

#[test]
fn display() {
    let s = |m: Magnitude| m.to_string();
    assert_eq!(s(Magnitude::ONE), "");
    assert_eq!(s(Magnitude::pow10(-4)), "·10⁻⁴");
    assert_eq!(s(Magnitude::pow2(10)), "·2¹⁰");
    assert_eq!(s(Magnitude::ratio(1, 3)), "·1/3");
    assert_eq!(s(Magnitude::new(1, 180, 0, 0, 1)), "·π/180");
    assert_eq!(s(Magnitude::new(2, 1, 0, 0, 1)), "·2π");
    assert_eq!(s(Magnitude::ratio(1, 65535)), "·1/65535");
    assert_eq!(s(Magnitude::ratio(3, 1024)), "·3/1024");
    assert_eq!(s(Magnitude::ratio(2, 1).sqrt()), "·√2");
    assert_eq!(s(Magnitude::ratio(1, 2).sqrt()), "·1/√2");
    assert_eq!(s(Magnitude::LN10.div(Magnitude::ratio(20, 1))), "·ln10/20");
    assert_eq!(s(Magnitude::PI.sqrt()), "·√π");
    assert_eq!(s(Magnitude::ONE.div(Magnitude::PI)), "·1/π");
    assert_eq!(s(LB), "·45359237/100000000");
    assert_eq!(
        s(Magnitude::pow10(-60).mul(Magnitude::ratio(3, 1))),
        "·2⁻⁶⁰·5⁻⁶⁰·3"
    );
}

#[test]
fn sizes() {
    // what a `&'static Magnitude` costs a firmware that holds one at run time
    assert_eq!(core::mem::size_of::<PrimePower>(), 6);
    assert_eq!(core::mem::size_of::<Magnitude>(), 104);
}

#[cfg(feature = "shrink_wrap")]
#[test]
fn wire() {
    let mut buf = [0u8; 64];
    let bytes = |m: Magnitude, buf: &mut [u8]| m.to_ww_bytes(buf).unwrap().to_vec();
    assert_eq!(bytes(Magnitude::pow10(-3), &mut buf), [0xfa, 0x00]);
    assert_eq!(bytes(Magnitude::ONE, &mut buf), [0x00, 0x00]);
    for m in [
        Magnitude::ONE,
        Magnitude::pow2(10),
        Magnitude::ratio(1, 65535),
        Magnitude::new(1, 180, 0, 0, 1),
        Magnitude::ratio(2, 1).sqrt(),
        LBF.powi(3),
        Magnitude::LN10.div(Magnitude::ratio(20, 1)),
    ] {
        let b = bytes(m, &mut buf);
        assert_eq!(Magnitude::from_ww_bytes(&b).unwrap(), m, "{m}");
    }
    let refused = |m: Magnitude, buf: &mut [u8]| {
        let b = bytes(m, buf);
        assert_eq!(
            Magnitude::from_ww_bytes(&b),
            Err(shrink_wrap::Error::SubtypeOutOfRange),
            "{m:?}"
        );
    };
    let mut factors = [None; MAX_FACTORS];
    // a rest that is 1
    let one_rest = MagnitudeRest {
        pow2: 0,
        irrational: None,
        factors,
    };
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(one_rest),
        },
        &mut buf,
    );
    // a 2 among the primes
    factors[0] = Some(PrimePower { prime: 2, exp: 2 });
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                factors,
                ..one_rest
            }),
        },
        &mut buf,
    );
    // not a prime
    factors[0] = Some(PrimePower { prime: 9, exp: 2 });
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                factors,
                ..one_rest
            }),
        },
        &mut buf,
    );
    // not sorted
    factors[0] = Some(PrimePower { prime: 7, exp: 2 });
    factors[1] = Some(PrimePower { prime: 3, exp: 2 });
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                factors,
                ..one_rest
            }),
        },
        &mut buf,
    );
    // a gap before a used slot
    factors[0] = None;
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                factors,
                ..one_rest
            }),
        },
        &mut buf,
    );
    // an exponent 0
    factors[0] = Some(PrimePower { prime: 3, exp: 0 });
    factors[1] = None;
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                factors,
                ..one_rest
            }),
        },
        &mut buf,
    );
    // irrational powers that are all 0
    let irrational = Some(Irrational {
        pi: 0,
        ln2: 0,
        ln10: 0,
    });
    refused(
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                irrational,
                factors: [None; MAX_FACTORS],
                ..one_rest
            }),
        },
        &mut buf,
    );
}

