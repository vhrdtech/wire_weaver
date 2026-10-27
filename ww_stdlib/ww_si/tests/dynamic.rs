//! Units known only at run time (STD-37): `Dim` and `Scale` on the wire, `SIExpr`, `AnyQuantity`; golden bytes and
//! round trips, and that each is shown like the typed unit it maps onto.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::units::{self, AngularVelocity, Per, Times, Torque};
use ww_si::ww_numeric::NumericValue;
use ww_si::{
    AnyQuantity, AnyQuantityOwned, DegreeCelsius, Dim, MAX_UNIT_DEPTH, Prefix, Quantity, Ratio,
    SIExpr, SIExprOwned, Scale, Unit, UnitExpr, Volt,
};

type Viscous = Per<Torque, AngularVelocity>;

#[test]
fn dim_bytes() {
    let mut buf = [0u8; 8];
    let volt = <units::Volt as Unit>::DIM;
    // s -3, m 2, kg 1, A -1, K 0, mol 0, cd 0, rad 0, bit 0: nine nibbles, then four bits of empty counted slots
    assert_eq!(
        volt.to_ww_bytes(&mut buf).unwrap(),
        [0xd2, 0x1f, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        Dim::from_ww_bytes(&[0xd2, 0x1f, 0x00, 0x00, 0x00]).unwrap(),
        volt
    );
    assert_eq!(
        <Dim as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    );
    let edge = Dim::new(-8, 7, 0, 0, 0, 0, -1).with_rad(7).with_bit(-8);
    let bytes = edge.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes, [0x87, 0x00, 0x00, 0xf7, 0x80]);
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), edge);
    // samples per second: one counted slot, its name and exponent
    let rate = <ww_si::units::Per<units::Sample, units::Second> as Unit>::DIM;
    let mut buf = [0u8; 32];
    let bytes = rate.to_ww_bytes(&mut buf).unwrap().to_vec();
    // s -1 and eight 0 nibbles, slot 1 used: "Sample" and exponent 1, three empty slots
    assert_eq!(
        bytes,
        [
            0xf0, 0x00, 0x00, 0x00, 0x08, 0x53, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x10, 0x06
        ]
    );
    assert_eq!(Dim::from_ww_bytes(&bytes).unwrap(), rate);
    assert_eq!(rate.to_string(), "s⁻¹·Sample");
}

/// Counted bases: sorted by name, added up, gone at exponent 0, at most `MAX_COUNTED`; a dimension that isn't in
/// that form doesn't decode.
#[test]
fn counted_bases() {
    let sample = Dim::counted("Sample");
    let block = Dim::counted("Block");
    let d = sample.mul(block).div(Dim::counted("Frame"));
    assert_eq!(
        d.counted_bases().collect::<Vec<_>>(),
        [("Block", 1), ("Frame", -1), ("Sample", 1)]
    );
    assert_eq!(d, block.div(Dim::counted("Frame")).mul(sample));
    assert!(d.div(sample).div(block).mul(Dim::counted("Frame")).is_one());
    let four = d.mul(Dim::counted("Item"));
    assert_eq!(four.checked_mul(Dim::counted("Packet")), None);
    assert_eq!(
        four.checked_mul(Dim::counted("Item")).unwrap().to_string(),
        "Block·Frame⁻¹·Item²·Sample"
    );
    assert_ne!(sample, block);
    assert_ne!(sample, Dim::ONE);

    let mut buf = [0u8; 64];
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
        (Scale::Factor(0.5), &[0x20, 0, 0, 0, 0, 0, 0, 0xe0, 0x3f]),
        (
            Scale::Affine {
                factor: 1.0,
                offset: 273.15,
            },
            &[
                0x30, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f, 0x66, 0x66, 0x66, 0x66, 0x66, 0x12, 0x71, 0x40,
            ],
        ),
    ];
    for (scale, bytes) in cases {
        assert_eq!(scale.to_ww_bytes(&mut buf).unwrap(), bytes, "{scale:?}");
        assert_eq!(Scale::from_ww_bytes(bytes).unwrap(), scale);
    }
}

const VOLT_BYTES: [u8; 14] = [
    0x00, b'V', b'o', b'l', b't', b'V', 0xd2, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x01, 0x14,
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
    assert_eq!(bytes.len(), 51);
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

#[test]
fn any_quantity() {
    let mut buf = [0u8; 128];
    let v = AnyQuantity::from(Volt::milli(3300u16));
    let bytes = v.to_ww_bytes(&mut buf).unwrap().to_vec();
    // the typed value's bytes, then the unit
    let mut expected = vec![0x12, 0xe4, 0x0c];
    expected.extend_from_slice(&VOLT_BYTES);
    expected.push(0x69);
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 18);
    let back = AnyQuantity::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back, v);
    assert_eq!(back.to_string(), Volt::milli(3300u16).to_string());
    assert_eq!(format!("{back:#}"), format!("{:#}", Volt::milli(3300u16)));
    assert_eq!(format!("{back:?}"), "AnyQuantity(3300 mV U16)");
    assert_eq!(
        back.to_quantity::<units::Volt>(),
        Some(Volt::milli(3300u16))
    );
    assert_eq!(back.to_quantity::<units::Ampere>(), None);
    assert_eq!(back.to_si_f64(), Some(3.3));

    let owned = AnyQuantityOwned::from(&back);
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(
        AnyQuantityOwned::from_ww_bytes_owned(&bytes).unwrap(),
        owned
    );
    assert_eq!(owned, AnyQuantityOwned::from(Volt::milli(3300u16)));
    assert_eq!(owned.to_string(), "3300 mV");
    assert_eq!(
        owned.to_quantity::<units::Volt>(),
        Some(Volt::milli(3300u16))
    );

    // shown like the typed quantities: a ratio's prefix, a compound unit with a prefix, an affine scale
    let r = AnyQuantity::from(Ratio::percent(50u8));
    assert_eq!(r.to_string(), "50 %");
    let b = AnyQuantity::from(Quantity::<Viscous>::new(Prefix::Milli, 12u8));
    assert_eq!(b.to_string(), "12 m(N·m/(rad/s))");
    assert_eq!(
        b.to_string(),
        Quantity::<Viscous>::new(Prefix::Milli, 12u8).to_string()
    );
    let t = AnyQuantity::from(DegreeCelsius::base(20u8));
    assert_eq!(t.to_string(), "20 °C");
    assert_eq!(t.to_si_f64(), Some(293.15));

    // a device's own unit, not in ww_si
    let rpm = SIExpr::Named {
        name: "Rpm",
        symbol: "rpm",
        dim: Dim::new(-1, 0, 0, 0, 0, 0, 0),
        scale: Scale::Factor(core::f64::consts::TAU / 60.0),
    };
    let q = AnyQuantity::new(Prefix::Kilo, 3u8, rpm);
    assert_eq!(q.to_string(), "3 krpm");
    assert_eq!(q.unit.known(), None);
    assert!((q.to_si_f64().unwrap() - 3000.0 * core::f64::consts::TAU / 60.0).abs() < 1e-9);
    assert_eq!(q.value, NumericValue::U8(3));
}
