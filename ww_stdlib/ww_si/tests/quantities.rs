//! Constructors, conversions, comparisons, formatting and algebra of quantities, and their bytes on the wire.
#![cfg(feature = "std")]

use shrink_wrap::prelude::*;
use ww_si::units::{self, Per, Times};
use ww_si::ww_numeric::{NumericBaseType, NumericValue, UVlq32};
use ww_si::{
    Acceleration, Ampere, AngularVelocity, Coulomb, DeciBel, Degree, DegreeCelsius, Dim, Gram,
    Hertz, Joule, Meter, Newton, Prefix, Quantity, Radian, Ratio, Second, Speed, Timed, Torque,
    UnitExpr, Volt, Watt,
};

#[test]
fn constructors() {
    let v = Volt::milli(3300u16);
    assert_eq!(v.prefix, Prefix::Milli);
    assert_eq!(v.value, NumericValue::U16(3300));
    let v = Volt::from_f32(3.3);
    assert_eq!(v.prefix, Prefix::Unit);
    assert_eq!(v.value, NumericValue::F32(3.3));
    assert_eq!(Hertz::kilo(400u32).prefix, Prefix::Kilo);
    assert_eq!(Hertz::mega(8u8).prefix, Prefix::Mega);
    assert_eq!(Hertz::giga(1u8).prefix, Prefix::Giga);
    assert_eq!(
        Second::micro(UVlq32(7)).value,
        NumericValue::UVlq32(UVlq32(7))
    );
    assert_eq!(Second::nano(5u8).prefix, Prefix::Nano);
    assert_eq!(Second::pico(5u8).prefix, Prefix::Pico);
    assert_eq!(Second::new(Prefix::Nano, 5u8).value, NumericValue::U8(5));
    assert_eq!(Volt::base(3u8).prefix, Prefix::Unit);
    assert_eq!(Ratio::centi(50u8), Ratio::percent(50u8));
    const LIMIT: Ampere = Ampere::from_parts(Prefix::Milli, NumericValue::U16(1500));
    assert_eq!(LIMIT, Ampere::base(1.5f32));
}

#[test]
fn conversions() {
    assert_eq!(Volt::milli(3300u16).to_f64(), 3.3);
    assert_eq!(Volt::from_f32(3.3).to_f64_in(Prefix::Milli).round(), 3300.0);
    assert_eq!(Volt::from_f32(3.3).to_i128_in(Prefix::Milli), Some(3300));
    assert_eq!(
        Volt::milli(3300u16).to_i128_in(Prefix::Micro),
        Some(3_300_000)
    );
    // truncated toward zero when the target prefix is bigger
    assert_eq!(Volt::milli(3999u16).to_i128_in(Prefix::Unit), Some(3));
    assert_eq!(Hertz::mega(8u8).to_f32(), 8e6);
    assert_eq!(Second::micro(1_500_000u32).to_f64(), 1.5);
    assert_eq!(
        Second::new(Prefix::Quetta, u64::MAX).to_i128_in(Prefix::Quecto),
        None
    );
    assert_eq!(DeciBel::new(-3i8).to_f64(), -3.0);
}

/// Through each unit's scale: what plots and Python want.
#[test]
fn to_si() {
    assert!((Degree::base(90u8).to_si_f64() - core::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((DegreeCelsius::base(20u8).to_si_f64() - 293.15).abs() < 1e-9);
    assert!((DegreeCelsius::milli(-40_000i32).to_si_f64() - 233.15).abs() < 1e-9);
    assert_eq!(Gram::base(5u8).to_si_f64(), 0.005);
    assert_eq!(Gram::kilo(5u8).to_si_f64(), 5.0);
    assert_eq!(Volt::milli(3300u16).to_si_f64(), 3.3);
}

#[test]
fn quantize() {
    let i = Ampere::quantize(0.1523, Prefix::Milli, NumericBaseType::I32);
    assert!(i.same_repr(&Ampere::milli(152i32)));
    let i = Ampere::quantize(-0.1525, Prefix::Milli, NumericBaseType::I32);
    assert!(i.same_repr(&Ampere::milli(-153i32)));
    // saturates at the storage's range
    let v = Volt::quantize(100.0, Prefix::Milli, NumericBaseType::U16);
    assert!(v.same_repr(&Volt::milli(u16::MAX)));
    let v = Volt::quantize(-1.0, Prefix::Milli, NumericBaseType::U16);
    assert!(v.same_repr(&Volt::milli(0u16)));
    let v = Volt::quantize(1.25, Prefix::Unit, NumericBaseType::F32);
    assert!(v.same_repr(&Volt::from_f32(1.25)));
}

#[test]
fn with_prefix_and_same_repr() {
    let v = Volt::milli(3000u16);
    assert!(
        v.with_prefix(Prefix::Unit)
            .unwrap()
            .same_repr(&Volt::base(3u16))
    );
    // not exact
    assert!(Volt::milli(3300u16).with_prefix(Prefix::Unit).is_none());
    // doesn't fit the storage
    assert!(Volt::milli(3300u16).with_prefix(Prefix::Micro).is_none());
    assert!(
        Volt::milli(3300u32)
            .with_prefix(Prefix::Micro)
            .unwrap()
            .same_repr(&Volt::micro(3_300_000u32))
    );
    let f = Volt::from_f32(3.3).with_prefix(Prefix::Milli).unwrap();
    assert_eq!(f.prefix, Prefix::Milli);
    assert!((f.to_f32() - 3.3).abs() < 1e-6);

    assert!(Volt::milli(1000u16).same_repr(&Volt::milli(1000u16)));
    // equal, but not the same representation
    assert_eq!(Volt::milli(1000u16), Volt::base(1u8));
    assert!(!Volt::milli(1000u16).same_repr(&Volt::base(1u8)));
    assert!(!Volt::milli(1000u16).same_repr(&Volt::milli(1000u32)));
}

#[test]
fn comparisons_across_prefix_and_storage() {
    assert_eq!(Volt::milli(1000u16), Volt::base(1u8));
    assert_eq!(Volt::milli(3300u16), Volt::micro(3_300_000u32));
    assert_eq!(Volt::from_f32(1.5), Volt::milli(1500u16));
    assert_ne!(Volt::milli(1001u16), Volt::base(1u8));
    assert!(Volt::milli(999u16) < Volt::base(1u8));
    assert!(Hertz::giga(1u8) > Hertz::base(999_999_999u32));
    // prefixes 60 orders of magnitude apart: no overflow, still exact
    assert!(Second::new(Prefix::Quetta, 1u8) > Second::new(Prefix::Quecto, u128::MAX));
    assert!(Second::new(Prefix::Quecto, -1i8) < Second::new(Prefix::Quetta, 0u8));
    assert!(
        Volt::from_f32(f32::NAN)
            .partial_cmp(&Volt::base(0u8))
            .is_none()
    );
    assert!(DeciBel::new(-3i8) < DeciBel::new(0u8));
}

#[test]
fn display_and_debug() {
    assert_eq!(Volt::milli(3300u16).to_string(), "3300 mV");
    assert_eq!(format!("{:#}", Volt::milli(3300u16)), "3.3 V");
    assert_eq!(Volt::from_f32(3.3).to_string(), "3.3 V");
    assert_eq!(format!("{:#}", Volt::from_f32(3.3)), "3.3 V");
    assert_eq!(Ampere::micro(-152_000i32).to_string(), "-152000 µA");
    assert_eq!(Gram::kilo(5u8).to_string(), "5 kg");
    assert_eq!(Torque::milli(250i32).to_string(), "250 mN·m");
    assert_eq!(format!("{:#}", Torque::milli(250i32)), "0.25 N·m");
    assert_eq!(format!("{:#}", Gram::kilo(5u8)), "5000 g");
    assert_eq!(format!("{:#}", Ratio::percent(50u8)), "0.5");
    assert_eq!(Ratio::percent(50u8).to_string(), "50 %");
    assert_eq!(Ratio::ppm(250u16).to_string(), "250 ppm");
    assert_eq!(Ratio::from_f32(0.5).to_string(), "0.5");
    assert_eq!(DegreeCelsius::base(21u8).to_string(), "21 °C");
    let b = Quantity::<Per<units::Torque, units::AngularVelocity>>::from_f32(1.2e-4);
    assert_eq!(b.to_string(), "0.00012 N·m/(rad/s)");
    let b = Quantity::<Per<units::Torque, units::AngularVelocity>>::milli(5u8);
    assert_eq!(b.to_string(), "5 m(N·m/(rad/s))");
    type Fan = Per<units::Torque, Times<units::AngularVelocity, units::AngularVelocity>>;
    assert_eq!(
        Quantity::<Fan>::base(2u8).to_string(),
        "2 N·m/((rad/s)·(rad/s))"
    );

    assert_eq!(format!("{:?}", Volt::milli(3300u16)), "Volt(3300 mV U16)");
    assert_eq!(
        format!(
            "{:?}",
            Quantity::<Per<units::Meter, units::Second>>::base(3u8)
        ),
        "Per<Meter, Second>(3 m/s U8)"
    );
}

#[test]
fn units_and_dimensions() {
    assert_eq!(Volt::unit().dim(), Dim::new(-3, 2, 1, -1, 0, 0, 0));
    assert_eq!(Volt::unit().dim().exps(), [-3, 2, 1, -1, 0, 0, 0, 0, 0]);
    assert_eq!(Volt::unit().dim().to_string(), "kg·m²·s⁻³·A⁻¹");
    assert_eq!(Ratio::unit().dim().to_string(), "1");
    let fan = <Per<units::Torque, Times<units::AngularVelocity, units::AngularVelocity>> as ww_si::Unit>::DIM;
    assert_eq!(fan.to_string(), "kg·m²·rad⁻³");
    let volt = ww_si::units::by_name("Volt").unwrap();
    assert!(matches!(volt, UnitExpr::Named { symbol: "V", .. }));
    assert_eq!(volt, Volt::unit());
    assert!(ww_si::units::by_name("Volts").is_none());
    assert_eq!(ww_si::units::ALL.len(), 58);
}

/// rad, bit and counted bases are dimensions of their own (STD-60): bytes never turn into a ratio, an angular
/// velocity is not a frequency, and a count per thing times the thing is the count.
#[test]
fn angle_information_counting() {
    use ww_si::Unit;
    let byte = <units::Byte as Unit>::DIM;
    assert_eq!(byte, <units::Bit as Unit>::DIM);
    assert_eq!(byte.to_string(), "bit");
    assert!(!byte.same(<units::Ratio as Unit>::DIM));
    assert!(!byte.same(<units::Radian as Unit>::DIM));
    assert_eq!(<units::Byte as Unit>::EXPR.factor(), 8.0);
    assert_eq!(<units::Word as Unit>::EXPR.factor(), 32.0);
    assert!(!<units::AngularVelocity as Unit>::DIM.same(<units::Hertz as Unit>::DIM));
    assert!(
        <units::AngularVelocity as Unit>::DIM
            .strip_radian()
            .same(<units::Hertz as Unit>::DIM)
    );
    assert!(
        <units::Torque as Unit>::DIM
            .strip_radian()
            .same(<units::Joule as Unit>::DIM)
    );
    assert!(!<units::Torque as Unit>::DIM.same(<units::Joule as Unit>::DIM));
    assert_eq!(<units::AngularVelocity as Unit>::DIM.to_string(), "s⁻¹·rad");
    assert!(!<units::Sample as Unit>::DIM.same(<units::Packet as Unit>::DIM));
    assert!(!<units::Sample as Unit>::DIM.is_one());
    let per_sample = <Per<units::Byte, units::Sample> as Unit>::DIM;
    assert_eq!(per_sample.to_string(), "bit·Sample⁻¹");
    assert!(per_sample.mul(<units::Sample as Unit>::DIM).same(byte));
    let rate = <Per<units::Sample, units::Second> as Unit>::DIM;
    assert!(!rate.same(<units::Hertz as Unit>::DIM));
    assert!(
        rate.mul(<units::Second as Unit>::DIM)
            .same(<units::Sample as Unit>::DIM)
    );
}

#[test]
fn algebra() {
    let p: Watt = Volt::milli(3300u16) * Ampere::milli(500u16);
    assert!((p.to_f32() - 1.65).abs() < 1e-6);
    assert_eq!(p.prefix, Prefix::Unit);
    let p: Watt = Ampere::milli(500u16) * Volt::milli(3300u16);
    assert!((p.to_f32() - 1.65).abs() < 1e-6);
    let i: Ampere = Watt::base(10u8) / Volt::base(5u8);
    assert_eq!(i, Ampere::base(2u8));
    let v: Volt = Watt::base(10u8) / Ampere::base(5u8);
    assert_eq!(v, Volt::base(2u8));
    let q: Coulomb = Ampere::milli(100u8) * Second::base(30u8);
    assert!((q.to_f32() - 3.0).abs() < 1e-6);
    let p: Watt = Torque::base(2u8) * AngularVelocity::base(100u8);
    assert_eq!(p, Watt::base(200u8));
    let angle: Radian = AngularVelocity::base(2u8) * Second::milli(500u16);
    assert_eq!(angle, Radian::base(1u8));
    let s: Speed = Meter::base(10u8) / Second::base(2u8);
    assert_eq!(s, Speed::base(5u8));
    // through the scale of `Gram`
    let f: Newton = Gram::kilo(2u8) * Acceleration::base(3u8);
    assert!((f.to_f32() - 6.0).abs() < 1e-5);
    let w: Joule = Newton::base(3u8) * Meter::base(2u8);
    assert_eq!(w, Joule::base(6u8));
    let r: Ratio = Hertz::kilo(1u8) * Second::milli(2u8);
    assert_eq!(r, Ratio::base(2u8));
    // a ratio keeps the unit
    let v: Volt = Volt::base(48u8) * Ratio::percent(50u8);
    assert_eq!(v, Volt::base(24u8));
    let v: Volt = Volt::base(24u8) / Ratio::from_f32(0.5);
    assert_eq!(v, Volt::base(48u8));
    // same unit
    assert_eq!(Volt::milli(500u16) + Volt::base(1u8), Volt::milli(1500u16));
    assert_eq!(Volt::base(1u8) - Volt::milli(250u16), Volt::milli(750u16));
    assert_eq!(Volt::milli(500u16) * 3.0, Volt::milli(1500u16));
    assert_eq!(Volt::base(3u8) / 2.0, Volt::milli(1500u16));
    // f64 storage computes in f64
    let p = Volt::base(1.0f64 / 3.0) * Ampere::base(3u8);
    assert_eq!(p.value.ty(), NumericBaseType::F64);
    assert_eq!(p.to_f64(), 1.0);
}

/// Prefix and storage are the device's choice, they show on the wire unchanged: the same bytes as ww_si 0.1, where
/// each quantity was a struct of its own.
#[test]
fn wire_bytes() {
    let mut buf = [0u8; 16];
    let bytes = Volt::milli(3300u16).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x12, 0xe4, 0x0c]);
    let bytes = Ampere::micro(-152_000i32).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x26, 0x40, 0xae, 0xfd, 0xff]);
    let bytes = Volt::from_f32(3.3).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x07, 0x33, 0x33, 0x53, 0x40]);
    let bytes = Second::micro(UVlq32(100)).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x28, 0x20, 0x64]);

    let back = Volt::from_ww_bytes(&[0x12, 0xe4, 0x0c]).unwrap();
    assert!(back.same_repr(&Volt::milli(3300u16)));
    let owned = Ampere::micro(-152_000i32).to_ww_bytes_owned().unwrap();
    assert_eq!(owned, [0x26, 0x40, 0xae, 0xfd, 0xff]);
    assert!(
        Ampere::from_ww_bytes_owned(&owned)
            .unwrap()
            .same_repr(&Ampere::micro(-152_000i32))
    );
    assert_eq!(size_of::<Volt>(), size_of::<(Prefix, NumericValue)>());
}

/// `Timed` is derived with a type parameter (DER-14): the bytes are what the derive gives the same fields without one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Debug),
    final_structure
)]
struct TimedAmpere {
    t: Second,
    value: Ampere,
}

#[test]
fn timed_wire_bytes() {
    let t = Second::micro(1_000_000u32);
    let value = Ampere::milli(-1200i32);
    let mut buf = [0u8; 32];
    let derived = TimedAmpere { t, value }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();
    let mut buf = [0u8; 32];
    let timed = Timed::new(t, value).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(timed, derived.as_slice());
    let back = Timed::<Ampere>::from_ww_bytes(timed).unwrap();
    assert_eq!(back, Timed::new(t, value));
    let owned = Timed::new(t, value).to_ww_bytes_owned().unwrap();
    assert_eq!(owned, derived);
    assert_eq!(
        Timed::<Ampere>::from_ww_bytes_owned(&owned).unwrap(),
        Timed::new(t, value)
    );
}

#[test]
fn prefix_names() {
    for prefix in Prefix::ALL {
        let bytes = prefix.to_ww_bytes_owned().unwrap();
        let back = Prefix::from_ww_bytes_owned(&bytes).unwrap();
        assert_eq!(back, prefix);
        assert_eq!(Prefix::from_name(prefix.name()), Some(prefix));
        assert_eq!(format!("{prefix:?}"), prefix.name());
    }
    assert_eq!(Prefix::from_name("Milli"), Some(Prefix::Milli));
    assert_eq!(Prefix::from_name("milli"), None);
}
