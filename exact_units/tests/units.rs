//! Marker units of the angle, information and counting groups, and prefix names.
#![cfg(all(feature = "std", feature = "quantities"))]

use exact_units::units::{self, Per};
use exact_units::{Prefix, Unit};
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// rad, bit and counted bases are dimensions of their own (STD-60): bytes never turn into a ratio, an angular
/// velocity is not a frequency, and a count per thing times the thing is the count.
#[test]
fn angle_information_counting() {
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
fn prefix_names() {
    for prefix in Prefix::ALL {
        #[cfg(feature = "shrink_wrap")]
        {
            let bytes = prefix.to_ww_bytes_owned().unwrap();
            let back = Prefix::from_ww_bytes_owned(&bytes).unwrap();
            assert_eq!(back, prefix);
        }
        assert_eq!(Prefix::from_name(prefix.name()), Some(prefix));
        assert_eq!(format!("{prefix:?}"), prefix.name());
    }
    assert_eq!(Prefix::from_name("Milli"), Some(Prefix::Milli));
    assert_eq!(Prefix::from_name("milli"), None);
}
