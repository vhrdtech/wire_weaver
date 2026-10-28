//! Parity of [Fixed] and [Quantity] (STD-59): every `Fixed` value is a `Quantity` and back exactly, whatever its
//! scale (a named prefix, else `Prefix::Exact`), and adding, converting and comparing give the same result or the same
//! refusal on both sides. STD-74 extends this to kinds, points, levels and niches.
#![cfg(feature = "std")]

use core::fmt::Debug;
use proptest::prelude::*;
use ww_si::exact_units::{Bounds, FixedScale, Storage, Unit};
use ww_si::prefix::{
    FullScale, FullScaleMax, Kibi, Kilo, Micro, Milli, One, Pow2, Pow10, Rational,
};
use ww_si::units::{Byte, Radian, Ratio, Second, Volt};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Fixed, Prefix, Quantity, Unbounded};

ww_si::exact_units::scale! {
    /// One step of a 16-bit binary angle in radians, 2π/65536
    pub struct Bam16Rad = "", Exact(1 / 1, pow2 -15, pi 1);

    /// One step of an 8-bit binary angle in radians, 2π/256
    pub struct Bam8Rad = "", Exact(1 / 1, pow2 -7, pi 1);
}

fn quantity<U: Unit, P: FixedScale, T: Storage + Into<NumericValue>, B: Bounds<T>>(
    f: Fixed<U, P, T, B>,
) -> Quantity<U> {
    Quantity::try_from_fixed(f).expect("every scale has a run-time prefix on std")
}

/// Round trip, comparison and (for integers) sum of two values of one `Fixed` type.
fn pair<U: Unit, P: FixedScale, T: Storage + Into<NumericValue> + Debug>(
    a: T,
    b: T,
    integer: bool,
) {
    let (fa, fb) = (Fixed::<U, P, T>::new(a), Fixed::<U, P, T>::new(b));
    let (qa, qb) = (quantity(fa), quantity(fb));
    assert_eq!(qa.to_fixed::<P, T, Unbounded>(), Some(fa), "{a:?} back");
    assert_eq!(fa.partial_cmp(&fb), qa.partial_cmp(&qb), "{a:?} cmp {b:?}");
    assert_eq!(fa == fb, qa == qb, "{a:?} == {b:?}");
    if integer {
        assert_eq!(
            fa.checked_add(fb),
            qa.checked_add(&qb)
                .map(|q| q.to_fixed().expect("a sum converts back")),
            "{a:?} + {b:?}"
        );
        assert_eq!(
            fa.checked_sub(fb),
            qa.checked_sub(&qb)
                .map(|q| q.to_fixed().expect("a difference converts back")),
            "{a:?} - {b:?}"
        );
    }
}

/// `Fixed::to` and `Quantity::to_fixed` into another scale and storage: the same value or both `None`.
fn convert<
    U: Unit,
    P: FixedScale,
    T: Storage + Into<NumericValue> + Debug,
    P2: FixedScale,
    T2: Storage + Debug,
>(
    a: T,
) {
    let f = Fixed::<U, P, T>::new(a);
    assert_eq!(
        f.to::<P2, T2>(),
        quantity(f).to_fixed::<P2, T2, Unbounded>(),
        "{a:?} {} -> {}",
        P::NAME,
        P2::NAME
    );
}

type Adc = FullScale<Rational<33, 10>, 12>;

#[test]
fn every_scale_has_a_prefix() {
    let prefix = |q: Quantity<Volt>| q.prefix;
    assert_eq!(
        prefix(quantity(Fixed::<Volt, Milli, u16>::new(1))),
        Prefix::Milli
    );
    assert_eq!(
        quantity(Fixed::<Byte, Kibi, u16>::new(1)).prefix,
        Prefix::Kibi
    );
    for p in [
        quantity(Fixed::<Second, Pow10<-4>, u32>::new(1)).prefix,
        quantity(Fixed::<Ratio, FullScaleMax<One, 16>, u16>::new(1)).prefix,
        quantity(Fixed::<Radian, Bam16Rad, u16>::new(1)).prefix,
        prefix(quantity(Fixed::<Volt, Adc, u16>::new(1))),
    ] {
        assert!(!p.is_named(), "{p:?}");
    }
    // shown alike: the number, then the factor and the unit
    let duty = Fixed::<Ratio, FullScaleMax<One, 16>, u16>::new(32768);
    assert_eq!(duty.to_string(), "32768·1/65535");
    assert_eq!(quantity(duty).to_string(), duty.to_string());
    let t = Fixed::<Second, Pow10<-4>, u32>::new(15);
    assert_eq!(quantity(t).to_string(), "15·10⁻⁴ s");
    assert_eq!(quantity(t).to_string(), t.to_string());
    assert_eq!(format!("{:#}", quantity(t)), format!("{t:#}"));
}

proptest! {
    #[test]
    fn milli_volt_i16(a: i16, b: i16) {
        pair::<Volt, Milli, i16>(a, b, true);
        convert::<Volt, Milli, i16, Micro, i32>(a);
        convert::<Volt, Milli, i16, One, i8>(a);
        convert::<Volt, Milli, i16, Pow10<-4>, i32>(a);
    }

    #[test]
    fn tenth_ms_u32(a: u32, b: u32) {
        pair::<Second, Pow10<-4>, u32>(a, b, true);
        convert::<Second, Pow10<-4>, u32, Milli, u16>(a);
        convert::<Second, Pow10<-4>, u32, Micro, u32>(a);
    }

    #[test]
    fn kibibytes_u16(a: u16, b: u16) {
        pair::<Byte, Kibi, u16>(a, b, true);
        convert::<Byte, Kibi, u16, One, u32>(a);
        convert::<Byte, Kibi, u16, Kilo, u16>(a);
        convert::<Byte, Kibi, u16, Pow2<-6>, u32>(a);
    }

    #[test]
    fn duty_u16(a: u16, b: u16) {
        pair::<Ratio, FullScaleMax<One, 16>, u16>(a, b, true);
        convert::<Ratio, FullScaleMax<One, 16>, u16, Pow2<-16>, u32>(a);
        convert::<Ratio, FullScaleMax<One, 16>, u16, FullScaleMax<One, 8>, u8>(a);
    }

    #[test]
    fn bam_u16(a: u16, b: u16) {
        pair::<Radian, Bam16Rad, u16>(a, b, true);
        // into a scale without π `Fixed` refuses at compile time, `Quantity` at run time: not comparable here
        convert::<Radian, Bam16Rad, u16, Bam8Rad, u8>(a);
    }

    #[test]
    fn adc_u16(a in 0u16..4096, b in 0u16..4096) {
        pair::<Volt, Adc, u16>(a, b, true);
        convert::<Volt, Adc, u16, Micro, u32>(a);
        convert::<Volt, Adc, u16, FullScale<Rational<33, 10>, 16>, u16>(a);
    }

    #[test]
    fn milli_volt_f32(a in -1e6f32..1e6, b in -1e6f32..1e6) {
        pair::<Volt, Milli, f32>(a, b, false);
        convert::<Volt, Milli, f32, Micro, f32>(a);
        convert::<Volt, Milli, f32, One, i32>(a);
    }
}
