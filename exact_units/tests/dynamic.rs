//! Units known only at run time (STD-37): `Dim` and `Scale` on the wire, `SIExpr`; golden bytes and round trips,
//! and that each is shown like the typed unit it maps onto.
#![cfg(all(feature = "std", feature = "quantities"))]

use exact_units::units::{self, AngularVelocity, Per, Times, Torque};
use exact_units::{Dim, MAX_UNIT_DEPTH, Magnitude, SIExpr, SIExprOwned, Scale, Unit, UnitExpr};
use shrink_wrap::prelude::*;

type Viscous = Per<Torque, AngularVelocity>;

#[test]
fn dim_bytes() {
    let mut buf = [0u8; 8];
    let volt = <units::Volt as Unit>::DIM;
    // s -3, m 2, kg 1, A -1, K 0, mol 0, cd 0, rad 0, bit 0: nine nibbles, the root bit, then four bits of empty
    // counted slots
    assert_eq!(
        volt.to_ww_bytes(&mut buf).unwrap(),
        [0xd2, 0x1f, 0x00, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        Dim::from_ww_bytes(&[0xd2, 0x1f, 0x00, 0x00, 0x00, 0x00]).unwrap(),
        volt
    );
    assert_eq!(
        <Dim as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    );
    let edge = Dim::new(-8, 7, 0, 0, 0, 0, -1).with_rad(7).with_bit(-8);
    let bytes = edge.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes, [0x87, 0x00, 0x00, 0xf7, 0x80, 0x00]);
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), edge);
    // samples per second: one counted slot, its name and exponent
    let rate = <units::Per<units::Sample, units::Second> as Unit>::DIM;
    let mut buf = [0u8; 32];
    let bytes = rate.to_ww_bytes(&mut buf).unwrap().to_vec();
    // s -1 and eight 0 nibbles, no root, slot 1 used: crate "exact_units", "Sample" and exponent 1, three empty slots
    assert_eq!(
        bytes,
        [
            0xf0, 0x00, 0x00, 0x00, 0x04, 0x65, 0x78, 0x61, 0x63, 0x74, 0x5f, 0x75, 0x6e, 0x69,
            0x74, 0x73, 0x53, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x10, 0x06, 0x39
        ]
    );
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), rate);
    assert_eq!(rate.to_string(), "s⁻¹·Sample");
}

/// Counted bases: sorted by name, added up, gone at exponent 0, at most `MAX_COUNTED`; a dimension that isn't in
/// that form doesn't decode.
#[test]
fn counted_bases() {
    let sample = Dim::counted("exact_units", "Sample");
    let block = Dim::counted("exact_units", "Block");
    let d = sample.mul(block).div(Dim::counted("exact_units", "Frame"));
    assert_eq!(
        d.counted_bases().collect::<Vec<_>>(),
        [("Block", 1), ("Frame", -1), ("Sample", 1)]
    );
    assert_eq!(
        d,
        block.div(Dim::counted("exact_units", "Frame")).mul(sample)
    );
    assert!(
        d.div(sample)
            .div(block)
            .mul(Dim::counted("exact_units", "Frame"))
            .is_one()
    );
    let four = d.mul(Dim::counted("exact_units", "Item"));
    assert_eq!(
        four.checked_mul(Dim::counted("exact_units", "Packet")),
        None
    );
    assert_eq!(
        four.checked_mul(Dim::counted("exact_units", "Item"))
            .unwrap()
            .to_string(),
        "Block·Frame⁻¹·Item²·Sample"
    );
    assert_ne!(sample, block);
    assert_ne!(sample, Dim::ONE);

    // a base is its crate and marker name: another crate's Sample is another base, sorted by crate first
    let theirs = Dim::counted("my_api", "Sample");
    assert_ne!(theirs, sample);
    let both = sample.mul(theirs);
    assert_eq!(both.counted[0].unwrap().crate_name, "exact_units");
    assert_eq!(both.counted[1].unwrap().crate_name, "my_api");
    assert!(both.div(theirs).same(sample));
    assert_eq!(
        exact_units::counted!(Bucket),
        Dim::counted("dynamic", "Bucket")
    );

    let mut buf = [0u8; 128];
    let bytes = d.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), d);
    // out of order: refused on decode
    let mut unsorted = d;
    unsorted.counted.swap(0, 2);
    assert!(!unsorted.is_canonical());
    let bytes = unsorted.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(Dim::from_ww_bytes(&bytes).is_err());
}

#[test]
fn dim_range() {
    assert_eq!(Dim::checked_new([8, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    assert_eq!(Dim::checked_new([0, 0, 0, 0, 0, 0, 0, 0, -9]), None);
    let m4 = Dim::new(0, 4, 0, 0, 0, 0, 0);
    assert_eq!(m4.checked_mul(m4), None);
    assert_eq!(m4.checked_pow(2), None);
    assert_eq!(m4.checked_pow(-1), Some(Dim::new(0, -4, 0, 0, 0, 0, 0)));
    assert_eq!(m4.checked_div(m4), Some(Dim::ONE));
}

#[test]
fn scale_bytes() {
    let mut buf = [0u8; 32];
    let cases: [(Scale, &[u8]); 4] = [
        (Scale::Coherent, &[0x00]),
        (Scale::Pow10(-3), &[0x10, 0xfd]),
        // a degree, π/180
        (
            Scale::Exact(Magnitude::new(1, 180, 0, 0, 1)),
            &[0x30, 0xfe, 0x80, 0xfe, 0x80, 0x02, 0, 0, 0x83, 0xfc, 0, 0],
        ),
        (
            Scale::Affine {
                factor: 1.0,
                offset: 273.15,
            },
            &[
                0x20, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f, 0x66, 0x66, 0x66, 0x66, 0x66, 0x12, 0x71, 0x40,
            ],
        ),
    ];
    for (scale, bytes) in cases {
        assert_eq!(scale.to_ww_bytes(&mut buf).unwrap(), bytes, "{scale:?}");
        assert_eq!(Scale::from_ww_bytes(bytes).unwrap(), scale);
    }
}

const VOLT_BYTES: [u8; 15] = [
    0x00, b'V', b'o', b'l', b't', b'V', 0xd2, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x14,
];

#[test]
fn named_bytes() {
    let mut buf = [0u8; 64];
    let volt = <units::Volt as Unit>::SI_EXPR;
    assert_eq!(volt.to_ww_bytes(&mut buf).unwrap(), VOLT_BYTES);
    let back = SIExpr::from_ww_bytes(&VOLT_BYTES).unwrap();
    assert_eq!(&back, volt);
    assert_eq!(back, *<units::Volt as Unit>::EXPR);
    assert_eq!(back.known(), Some(<units::Volt as Unit>::EXPR));
    assert_eq!(back.to_string(), "V");
    assert_eq!(back.name().to_string(), "Volt");
    assert_eq!(back.dim(), Some(<units::Volt as Unit>::DIM));
}

#[test]
fn compound_round_trip() {
    let mut buf = [0u8; 128];
    let si = <Viscous as Unit>::SI_EXPR;
    let bytes = si.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes.len(), 53);
    assert_eq!(&bytes[..2], [0x20, 0x00]);
    let back = SIExpr::from_ww_bytes(&bytes).unwrap();
    assert_eq!(&back, si);
    assert_eq!(back, *<Viscous as Unit>::EXPR);
    assert_eq!(back.known(), None);
    // shown exactly as the typed unit
    assert_eq!(back.to_string(), <Viscous as Unit>::EXPR.to_string());
    assert_eq!(back.to_string(), "N·m/(rad/s)");
    assert_eq!(back.name().to_string(), "Per<Torque, AngularVelocity>");
    assert_eq!(back.dim(), Some(<Viscous as Unit>::DIM));
    assert_eq!(back.factor(), Some(1.0));

    type Fan = Per<Torque, Times<AngularVelocity, AngularVelocity>>;
    let fan = SIExpr::from_ww_bytes(<Fan as Unit>::SI_EXPR.to_ww_bytes(&mut buf).unwrap()).unwrap();
    assert_eq!(fan.to_string(), "N·m/((rad/s)·(rad/s))");
    assert_eq!(fan.to_string(), <Fan as Unit>::EXPR.to_string());
    assert_eq!(fan.dim().unwrap().to_string(), "kg·m²·rad⁻³");

    // a different tree of the same dimension is not the same unit
    assert_ne!(back, *<Fan as Unit>::EXPR);
    assert_ne!(back, *<units::Torque as Unit>::EXPR);
}

#[test]
fn owned_and_unit_expr() {
    let owned = SIExprOwned::from(<Viscous as Unit>::EXPR);
    assert_eq!(owned, *<Viscous as Unit>::EXPR);
    let bytes = owned.to_ww_bytes_owned().unwrap();
    let mut buf = [0u8; 128];
    assert_eq!(
        bytes,
        <Viscous as Unit>::SI_EXPR.to_ww_bytes(&mut buf).unwrap()
    );
    let borrowed = SIExpr::from_ww_bytes(&bytes).unwrap();
    assert_eq!(SIExprOwned::from(&borrowed), owned);
    assert_eq!(SIExprOwned::from_ww_bytes_owned(&bytes).unwrap(), owned);
    assert_eq!(owned.to_string(), "N·m/(rad/s)");
    assert_eq!(owned.name().to_string(), "Per<Torque, AngularVelocity>");
}

#[test]
fn pow() {
    static METER: UnitExpr = UnitExpr::Named {
        name: "Meter",
        symbol: "m",
        dim: Dim::new(0, 1, 0, 0, 0, 0, 0),
        scale: Scale::Coherent,
    };
    static SQUARE: UnitExpr = UnitExpr::Pow(&METER, 2);
    static SPEED: UnitExpr = UnitExpr::Named {
        name: "Speed",
        symbol: "m/s",
        dim: Dim::new(-1, 1, 0, 0, 0, 0, 0),
        scale: Scale::Coherent,
    };
    static SPEED_SQ: UnitExpr = UnitExpr::Pow(&SPEED, 2);
    static PER_SQ: UnitExpr = UnitExpr::Div(&METER, &SQUARE);
    assert_eq!(SQUARE.to_string(), "m²");
    assert_eq!(SQUARE.name().to_string(), "Pow<Meter, 2>");
    assert_eq!(SPEED_SQ.to_string(), "(m/s)²");
    assert_eq!(PER_SQ.to_string(), "m/m²");
    assert_eq!(SPEED_SQ.dim().to_string(), "m²·s⁻²");

    let owned = SIExprOwned::from(&SPEED_SQ);
    let bytes = owned.to_ww_bytes_owned().unwrap();
    let back = SIExpr::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back, SPEED_SQ);
    assert_eq!(back.to_string(), "(m/s)²");
    assert_eq!(back.dim(), Some(SPEED_SQ.dim()));
    // out of -8..=7 at run time: no dimension, no panic
    let too_big = SIExprOwned::Pow(Box::new(owned), 4);
    let back = too_big.to_ww_bytes_owned().unwrap();
    assert_eq!(SIExpr::from_ww_bytes(&back).unwrap().dim(), None);
}

#[test]
fn depth_limit() {
    let mut e = SIExprOwned::from(<units::Volt as Unit>::EXPR);
    for _ in 0..MAX_UNIT_DEPTH + 8 {
        e = SIExprOwned::Pow(Box::new(e), 1);
    }
    let bytes = e.to_ww_bytes_owned().unwrap();
    let back = SIExpr::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back.dim(), None);
    assert_eq!(back.factor(), None);
    assert!(back.to_string().contains('…'));
    assert!(back.name().to_string().contains('…'));
    // `==` stops at the depth limit instead of recursing
    assert_ne!(back, *<units::Volt as Unit>::EXPR);
}

#[test]
fn bad_bytes() {
    // `Mul` whose children don't decode: shown as `?`, no dimension
    let mut buf = [0u8; 128];
    let si = <Viscous as Unit>::SI_EXPR;
    let mut bytes = si.to_ww_bytes(&mut buf).unwrap().to_vec();
    bytes[2] = 0xf0; // a Named child's tag turned into an unknown variant
    if let Ok(back) = SIExpr::from_ww_bytes(&bytes) {
        assert_eq!(back.dim(), None);
        let _ = back.to_string();
    }
    for len in 0..bytes.len() {
        if let Ok(back) = SIExpr::from_ww_bytes(&bytes[..len]) {
            let _ = (
                back.dim(),
                back.factor(),
                back.to_string(),
                back.name().to_string(),
            );
        }
    }
}

/// The root flag: a noise density in V/√Hz is the square root of V²/Hz, shown with half exponents; roots of even
/// exponents are plain dimensions, and a root that isn't canonical doesn't decode.
#[test]
fn root() {
    let volt = <units::Volt as Unit>::DIM;
    let hertz = <units::Hertz as Unit>::DIM;
    let density = volt.div(hertz.sqrt());
    assert!(density.root);
    assert_eq!(density.exps()[..4], [-5, 4, 2, -2]);
    assert_eq!(density.to_string(), "kg·m²·s⁻⁵ᐟ²·A⁻¹");
    // back to volts: times √Hz, or squared times Hz
    assert_eq!(density.mul(hertz.sqrt()), volt);
    assert_eq!(
        density.checked_pow(2).unwrap().mul(hertz),
        volt.checked_pow(2).unwrap()
    );
    // √(m²) is m, not a root
    let m2 = Dim::new(0, 2, 0, 0, 0, 0, 0);
    assert_eq!(m2.sqrt(), Dim::new(0, 1, 0, 0, 0, 0, 0));
    assert_eq!(density.checked_sqrt(), None);

    let mut buf = [0u8; 16];
    let bytes = density.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes, [0xb4, 0x2e, 0x00, 0x00, 0x08, 0x00]);
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), density);
    // m² with the root bit is √(m²) written the long way: refused
    let mut long = m2;
    long.root = true;
    assert!(!long.is_canonical());
    let bytes = long.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(Dim::from_ww_bytes(&bytes).is_err());
}

/// The typed root, `Sqrt<U>`: V/√Hz as `Per<Volt, Sqrt<Hertz>>` has the root dimension, the symbol `V/√Hz`, and goes
/// through the wire as an `SIExpr::Sqrt` node.
#[test]
fn sqrt_marker() {
    use exact_units::units::{Hertz, Sqrt, Volt};
    type Density = Per<Volt, Sqrt<Hertz>>;
    let dim = <Density as Unit>::DIM;
    assert_eq!(dim, <Volt as Unit>::DIM.div(<Hertz as Unit>::DIM.sqrt()));
    assert!(dim.root);
    let expr = <Density as Unit>::EXPR;
    assert_eq!(expr.to_string(), "V/√Hz");
    assert_eq!(expr.name().to_string(), "Per<Volt, Sqrt<Hertz>>");
    assert_eq!(expr.dim(), dim);
    assert_eq!(
        <Sqrt<Per<Volt, Hertz>> as Unit>::EXPR.to_string(),
        "√(V/Hz)"
    );
    // √(m²) is m, √Hz in kHz is √1000 times √Hz
    assert_eq!(
        <Sqrt<Times<units::Meter, units::Meter>> as Unit>::DIM,
        <units::Meter as Unit>::DIM
    );
    assert_eq!(<Sqrt<units::Hertz> as Unit>::EXPR.factor(), 1.0);
    assert_eq!(exact_units::num::sqrt_f64(4.0), 2.0);
    assert_eq!(exact_units::num::sqrt_f64(1e6), 1e3);
    // rounded like std's sqrt
    for x in [
        2.0,
        3.0,
        0.5,
        1e-3,
        5e-324,
        1e-310,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        assert_eq!(
            exact_units::num::sqrt_f64(x).to_bits(),
            x.sqrt().to_bits(),
            "{x:e}"
        );
    }
    assert!(exact_units::num::sqrt_f64(-1.0).is_nan());

    let mut buf = [0u8; 128];
    let bytes = <Density as Unit>::SI_EXPR
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();
    let back = SIExpr::from_ww_bytes(&bytes).unwrap();
    assert!(matches!(back, SIExpr::Div(..)));
    assert_eq!(back, *expr);
    assert_eq!(back.to_string(), "V/√Hz");
    assert_eq!(back.dim(), Some(dim));
    let owned = SIExprOwned::from(expr);
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(SIExprOwned::from(&back), owned);
    // the Sqrt node itself: variant 4 in the first nibble
    let sqrt = <Sqrt<Hertz> as Unit>::SI_EXPR
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();
    assert_eq!(sqrt[0] >> 4, 4, "{sqrt:02x?}");
}
