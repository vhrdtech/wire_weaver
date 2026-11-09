//! `AnyQuantity` (STD-37): a number with a unit known only at run time; golden bytes, round trips, and that it is
//! shown like the typed quantity it maps onto. `Dim`, `Scale` and `SIExpr` on their own: exact_units' tests.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::units::{self, AngularVelocity, Per, Torque};
use ww_si::ww_numeric::NumericValue;
use ww_si::{
    AnyQuantity, AnyQuantityOwned, Dim, Magnitude, Prefix, Quantity, Ratio, SIExpr, SIExprOwned,
    Unit, Volt,
};

type Viscous = Per<Torque, AngularVelocity>;

// the unit's definition: crate `exact_units`, marker `Volt`, names (symbol `V`, long `volt`, plural `volts`, UCUM
// `V`), dimension (seven exponents, no base), kind, scale one, provenance
const VOLT_BYTES: [u8; 45] = [
    0x00, b'e', b'x', b'a', b'c', b't', b'_', b'u', b'n', b'i', b't', b's', b'V', b'o', b'l', b't',
    b'V', 0x00, b'v', b'o', b'l', b't', b'v', b'o', b'l', b't', b's', 0x80, b'V', 0x11, 0x05, 0x41,
    0xb1, 0x81, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x70, 0xa4, 0x39, 0x3d,
];

#[test]
fn any_quantity() {
    let mut buf = [0u8; 128];
    let v = AnyQuantity::from(Volt::milli(3300u16));
    let bytes = v.to_ww_bytes(&mut buf).unwrap().to_vec();
    // the typed value's bytes, then the unit
    let mut expected = vec![0x12, 0xe4, 0x0c];
    expected.extend_from_slice(&VOLT_BYTES);
    expected.push(0x5d);
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 49);
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
    assert_eq!(back.to_si_f64(&()), Some(3.3));

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

    // shown like the typed quantities: a ratio's prefix, a compound unit with a prefix
    let r = AnyQuantity::from(Ratio::percent(50u8));
    assert_eq!(r.to_string(), "50 %");
    let b = AnyQuantity::from(Quantity::<Viscous>::new(Prefix::Milli, 12u8));
    assert_eq!(b.to_string(), "12 m(N·m/(rad/s))");
    assert_eq!(
        b.to_string(),
        Quantity::<Viscous>::new(Prefix::Milli, 12u8).to_string()
    );

    // a device's own unit, not in ww_si
    let rpm = SIExpr::Def(ww_si::exact_units::UnitDef {
        id: ww_si::exact_units::MarkerId {
            crate_name: "my_api",
            name: "Rpm",
        },
        names: ww_si::exact_units::UnitNames::new("rpm", ww_si::PrefixPolicy::Decimal),
        dim: Dim::new(-1, 0, 0, 0, 0, 0, 0),
        quantity: None,
        scale: Magnitude::new(1, 30, 0, 0, 1),
        provenance: ww_si::Provenance::Defined,
    });
    let q = AnyQuantity::new(Prefix::Kilo, 3u8, rpm);
    assert_eq!(q.to_string(), "3 krpm");
    assert_eq!(q.unit.known(), None);
    assert!((q.to_si_f64(&()).unwrap() - 3000.0 * core::f64::consts::TAU / 60.0).abs() < 1e-9);
    assert_eq!(q.value, NumericValue::U8(3));
}

/// A unit with an exact scale (`Scale::Exact`) keeps it through the wire: 90 ° is exactly `90 · π/180` rad on the
/// reader, and a turn is `2π` rad.
#[test]
fn exact_scale_through_the_wire() {
    let mut buf = [0u8; 128];
    let deg = AnyQuantity::from(ww_si::Degree::base(90u8));
    let bytes = deg.to_ww_bytes(&mut buf).unwrap().to_vec();
    let back = AnyQuantity::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back.to_string(), "90 °");
    assert_eq!(back.unit.known(), deg.unit.known());
    assert!((back.to_si_f64(&()).unwrap() - core::f64::consts::FRAC_PI_2).abs() < 1e-15);

    let rev = AnyQuantity::from(ww_si::Turn::base(1u8));
    assert!((rev.to_si_f64(&()).unwrap() - core::f64::consts::TAU).abs() < 1e-15);
}

/// A value compacted against a table names its units by identity: `Volt` is `exact_units::Volt`, 17 bytes instead of
/// the 31 of its definition. It reads through any table that lists the unit, and by its name alone through none.
#[test]
fn compacted_to_references() {
    let mut buf = [0u8; 128];
    let v = AnyQuantityOwned::from(Volt::milli(3300u16));
    let r = v.compact(&units::TABLE).unwrap();
    let id = ww_si::MarkerId {
        crate_name: "exact_units",
        name: "Volt",
    };
    assert_eq!(r.unit, SIExprOwned::Ref(id.into()));
    let bytes = r.to_ww_bytes_owned().unwrap();
    // the variant nibble 1, the two names, their lengths at the end
    let mut expected = vec![0x12, 0xe4, 0x0c, 0x10];
    expected.extend_from_slice(b"exact_unitsVolt");
    expected.extend_from_slice(&[0x04, 0x39, 0x2a]);
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 22);

    let back = AnyQuantity::from_ww_bytes(&bytes).unwrap();
    assert_eq!(back.unit, SIExpr::Ref(id));
    assert_eq!(back.to_string(), "3300 mV");
    assert_eq!(back.to_si_f64(&units::TABLE), Some(3.3));
    assert_eq!(
        back.to_quantity::<units::Volt>(),
        Some(Volt::milli(3300u16))
    );
    assert_eq!(back.to_quantity::<units::Ampere>(), None);
    // no table lists it: shown by its name, no factor
    assert_eq!(back.display(&()).to_string(), "3300 mVolt");
    assert_eq!(back.to_si_f64(&()), None);
    // borrowed, with no arena for a named unit, the same bytes
    let typed = AnyQuantity::from(Volt::milli(3300u16));
    let compact = typed.compact_in(&units::TABLE, &mut []).unwrap();
    assert_eq!(compact.to_ww_bytes(&mut buf).unwrap(), bytes);
    // and back to the definition
    assert_eq!(r.resolve(&units::TABLE).unwrap(), v);

    // a compound unit: each named part a reference
    let speed = AnyQuantityOwned::from(Quantity::<Per<units::Meter, units::Second>>::new(
        Prefix::Kilo,
        3u8,
    ));
    let compact = speed.compact(&units::TABLE).unwrap();
    assert_eq!(speed.to_string(), "3 km/s");
    assert_eq!(compact.to_string(), speed.to_string());
    assert_eq!(compact.to_si_f64(&units::TABLE), Some(3000.0));

    // a unit no table lists stays in full; one listed under its name with another definition is refused
    let mut fake = *units::Volt::EXPR.def().unwrap();
    fake.scale = Magnitude::new(1, 1000, 0, 0, 1);
    let q = AnyQuantityOwned {
        prefix: Prefix::Unit,
        value: NumericValue::U8(1),
        unit: SIExprOwned::Def(fake.into()),
    };
    assert_eq!(
        q.compact(&units::TABLE),
        Err(ww_si::CompactError::Conflict(id))
    );
}
