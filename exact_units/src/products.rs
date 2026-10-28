//! The product table: which units multiply and divide into which (`Volt · Ampere = Watt`), as [UnitMul] and
//! [UnitDiv] impls. Each line is checked against the dimensions of its units at compile time, so the table can't be
//! wrong.

#[allow(unused_imports)] // with no unit group enabled
use crate::units;
#[allow(unused_imports)]
use crate::{Unit, UnitDiv, UnitMul};

/// A ratio keeps the unit: 48 V · 0.5 duty = 24 V.
#[cfg(feature = "dimensionless")]
impl<U: Unit> UnitMul<units::Ratio> for U {
    type Output = U;
}

/// A ratio keeps the unit: 24 V / 0.5 = 48 V.
#[cfg(feature = "dimensionless")]
impl<U: Unit> UnitDiv<units::Ratio> for U {
    type Output = U;
}

/// `A * B = C;` gives `A · B`, `B · A`, `C / A` and `C / B`, and checks at compile time that the SI exponents of `A`
/// and `B` add up to those of `C` and that none of them has an offset scale (°C).
macro_rules! products {
    ($($(#[$meta:meta])* $a:ident * $b:ident = $o:ident;)*) => {
        $(
            $(#[$meta])*
            impl UnitMul<units::$b> for units::$a {
                type Output = units::$o;
            }
            $(#[$meta])*
            impl UnitMul<units::$a> for units::$b {
                type Output = units::$o;
            }
            $(#[$meta])*
            impl UnitDiv<units::$a> for units::$o {
                type Output = units::$b;
            }
            $(#[$meta])*
            impl UnitDiv<units::$b> for units::$o {
                type Output = units::$a;
            }
            $(#[$meta])*
            const _: () = {
                assert!(
                    <units::$a as Unit>::DIM.mul(<units::$b as Unit>::DIM).same(<units::$o as Unit>::DIM),
                    concat!(stringify!($a * $b = $o), ": SI exponents differ")
                );
                assert!(
                    !<units::$a as Unit>::EXPR.has_offset()
                        && !<units::$b as Unit>::EXPR.has_offset()
                        && !<units::$o as Unit>::EXPR.has_offset(),
                    concat!(stringify!($a * $b = $o), ": a unit with an offset scale can't be in a product")
                );
            };
        )*
    };
}

products! {
    #[cfg(feature = "electrical")]
    Volt * Ampere = Watt;
    #[cfg(feature = "electrical")]
    Ampere * Ohm = Volt;
    #[cfg(feature = "electrical")]
    Henry * Ampere = Weber;
    #[cfg(feature = "electrical")]
    Farad * Volt = Coulomb;
    #[cfg(all(feature = "electrical", feature = "time"))]
    Ampere * Second = Coulomb;
    #[cfg(all(feature = "electrical", feature = "time"))]
    Volt * Second = Weber;
    #[cfg(all(feature = "electrical", feature = "time", feature = "mechanical"))]
    Watt * Second = Joule;
    #[cfg(all(feature = "electrical", feature = "mechanical", feature = "angle"))]
    Torque * AngularVelocity = Watt;
    #[cfg(all(feature = "angle", feature = "time"))]
    AngularVelocity * Second = Radian;
    #[cfg(all(feature = "mechanical", feature = "time"))]
    Speed * Second = Meter;
    #[cfg(all(feature = "mechanical", feature = "time"))]
    Acceleration * Second = Speed;
    #[cfg(feature = "mechanical")]
    Gram * Acceleration = Newton;
    #[cfg(feature = "mechanical")]
    Newton * Meter = Joule;
    #[cfg(all(feature = "time", feature = "dimensionless"))]
    Hertz * Second = Ratio;
    #[cfg(feature = "time")]
    HertzPerSecond * Second = Hertz;
}
