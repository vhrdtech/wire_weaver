//! `AnyQuantity` (STD-37): a number with a unit known only at run time; golden bytes, round trips, and that it is
//! shown like the typed quantity it maps onto. `Dim`, `Scale` and `SIExpr` on their own: exact_units' tests.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::units::{self, AngularVelocity, Per, Torque};
use ww_si::ww_numeric::NumericValue;
use ww_si::{
    AnyQuantity, AnyQuantityOwned, DegreeCelsius, Dim, Prefix, Quantity, Ratio, SIExpr, Scale, Volt,
};

type Viscous = Per<Torque, AngularVelocity>;

const VOLT_BYTES: [u8; 15] = [
    0x00, b'V', b'o', b'l', b't', b'V', 0xd2, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x14,
];

#[test]
fn any_quantity() {
    let mut buf = [0u8; 128];
    let v = AnyQuantity::from(Volt::milli(3300u16));
    let bytes = v.to_ww_bytes(&mut buf).unwrap().to_vec();
    // the typed value's bytes, then the unit
    let mut expected = vec![0x12, 0xe4, 0x0c];
    expected.extend_from_slice(&VOLT_BYTES);
    expected.push(0x79);
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 19);
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

/// A unit with an exact scale (`Scale::Exact`) keeps it through the wire: 90 ° is exactly `90 · π/180` rad on the
/// reader, and a turn is `2π` rad.
#[test]
fn exact_scale_through_the_wire() {
    let mut buf = [0u8; 64];
    let deg = AnyQuantity::from(ww_si::Degree::base(90u8));
    let bytes = deg.to_ww_bytes(&mut buf).unwrap().to_vec();
    let back = AnyQuantity::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back.to_string(), "90 °");
    assert_eq!(back.unit.known(), deg.unit.known());
    assert!((back.to_si_f64().unwrap() - core::f64::consts::FRAC_PI_2).abs() < 1e-15);

    let rev = AnyQuantity::from(ww_si::Turn::base(1u8));
    assert!((rev.to_si_f64().unwrap() - core::f64::consts::TAU).abs() < 1e-15);
}
