//! Unit markers: zero-size types that name a unit in a type (`Quantity<units::Volt>`), never sent themselves. Every
//! one has an alias of its quantity at the crate root under the same name (`ww_si::Volt`), which is what APIs write.
//!
//! Compound units outside the list are [Per] and [Times] of markers: `Quantity<Per<Torque, AngularVelocity>>` is in
//! N·m/(rad/s). [by_name] and [ALL] give the [UnitExpr] of every named marker of this crate, for dynamic clients that
//! see a unit only by its name in introspection data.
//!
//! The markers are literal structs, so the snapshot of this crate holds them with their docs; their [Unit] impls come
//! from one table below, the `SI:` dimension of each line is checked against the product table at compile time.

use crate::SIExpr;
#[allow(unused_imports)] // `Scale` with no quantity group enabled
use crate::unit::{Dim, Scale, Unit, UnitExpr};
use core::marker::PhantomData;
use shrink_wrap::RefBox;

// Time

/// Time in seconds, s.
///
/// As a device timestamp it is the device's time since its clock started, see [Second](crate::Second).
#[cfg(feature = "time")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Second;

/// Frequency in hertz, Hz (s⁻¹).
#[cfg(feature = "time")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hertz;

/// Frequency drift in hertz per second, Hz/s (s⁻²).
#[cfg(feature = "time")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct HertzPerSecond;

/// Symbol rate in baud, Bd (s⁻¹): one symbol per second, for a UART one bit per second on the line. A frequency of
/// its own kind, so it is not mixed up with a clock in [Hertz].
#[cfg(feature = "time")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Baud;

// Electrical

/// Electric current in amperes, A.
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ampere;

/// Voltage, electric potential or electromotive force in volts, V (kg·m²·s⁻³·A⁻¹, W/A).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Volt;

/// Resistance, reactance or impedance in ohms, Ω (kg·m²·s⁻³·A⁻², V/A).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ohm;

/// Capacitance in farads, F (kg⁻¹·m⁻²·s⁴·A², C/V).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Farad;

/// Admittance, conductance or susceptance in siemens, S (kg⁻¹·m⁻²·s³·A², A/V).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Siemens;

/// Inductance in henries, H (kg·m²·s⁻²·A⁻², Wb/A).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Henry;

/// Electric charge in coulombs, C (s·A).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Coulomb;

/// Power in watts, W (kg·m²·s⁻³, J/s, V·A).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Watt;

/// Magnetic flux in webers, Wb (kg·m²·s⁻²·A⁻¹, V·s).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Weber;

/// Magnetic flux density in teslas, T (kg·s⁻²·A⁻¹, Wb/m²).
#[cfg(feature = "electrical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tesla;

// Mechanical

/// Length in meters, m.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Meter;

/// Mass in grams, g (10⁻³ kg): 5 kg is `Gram::kilo(5u8)`.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Gram;

/// Force or weight in newtons, N (kg·m·s⁻²).
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Newton;

/// Pressure or stress in pascals, Pa (kg·m⁻¹·s⁻², N/m²).
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pascal;

/// Energy, work or heat in joules, J (kg·m²·s⁻², N·m, W·s).
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Joule;

/// Torque or moment of force in newton-meters, N·m (kg·m²·s⁻²). Same dimension as [Joule], but not the same thing:
/// a torque times an angular velocity is a power.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Torque;

/// Volume in litres, L (10⁻³ m³).
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Litre;

/// Volumetric flow in cubic meters per second, m³/s.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct VolumetricFlow;

/// Speed in meters per second, m/s.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Speed;

/// Acceleration in meters per second squared, m/s².
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Acceleration;

/// Jerk in meters per second cubed, m/s³.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Jerk;

/// Snap or jounce in meters per second to the fourth, m/s⁴.
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Snap;

/// Yank, the rate of change of force, in newtons per second, N/s (kg·m·s⁻³).
#[cfg(feature = "mechanical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Yank;

// Angle

/// Angle in radians, rad (dimensionless, m/m).
#[cfg(feature = "angle")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Radian;

/// Angle in degrees of arc, ° (π/180 rad).
#[cfg(feature = "angle")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Degree;

/// Solid angle in steradians, sr (dimensionless, m²/m²).
#[cfg(feature = "angle")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Steradian;

/// Angular velocity in radians per second, rad/s (s⁻¹).
#[cfg(feature = "angle")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AngularVelocity;

/// Angular acceleration in radians per second squared, rad/s² (s⁻²).
#[cfg(feature = "angle")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AngularAcceleration;

// Thermal

/// Thermodynamic temperature in kelvins, K.
#[cfg(feature = "thermal")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kelvin;

/// Temperature in degrees Celsius, °C (K − 273.15). No products: a product with an offset scale means nothing.
#[cfg(feature = "thermal")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DegreeCelsius;

// Chemical

/// Amount of substance in moles, mol.
#[cfg(feature = "chemical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mole;

/// Catalytic activity in katals, kat (mol·s⁻¹).
#[cfg(feature = "chemical")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Katal;

// Photometric

/// Luminous intensity in candelas, cd.
#[cfg(feature = "photometric")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Candela;

/// Luminous flux in lumens, lm (cd·sr).
#[cfg(feature = "photometric")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lumen;

/// Illuminance in lux, lx (cd·m⁻², lm/m²).
#[cfg(feature = "photometric")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lux;

// Radiation

/// Radioactivity in becquerels, Bq (s⁻¹).
#[cfg(feature = "radiation")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Becquerel;

/// Absorbed dose in grays, Gy (m²·s⁻², J/kg).
#[cfg(feature = "radiation")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Gray;

/// Equivalent dose in sieverts, Sv (m²·s⁻², J/kg).
#[cfg(feature = "radiation")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sievert;

/// Energy in electron-volts, eV (1.602176634·10⁻¹⁹ J).
#[cfg(feature = "radiation")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ElectronVolt;

// Dimensionless

/// A fraction: a duty cycle, a probability, a scale factor, an efficiency (dimensionless, 1). 0.5 is half, not a
/// level in decibels. Written with `%`, `‰`, `ppm`, `ppb` for `Centi`, `Milli`, `Micro`, `Nano`. Any quantity times a ratio keeps its
/// unit (48 V · 0.5 = 24 V).
#[cfg(feature = "dimensionless")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ratio;

// Information

/// Information in bits, bit (dimensionless): a data size or a count of bits, told apart from a [Ratio] by the marker.
#[cfg(feature = "information")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bit;

/// Information in bytes, B (8 [Bit]): a data size, a buffer's capacity, an offset. A rate is `Per<Byte, Second>`.
#[cfg(feature = "information")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Byte;

// Counting

/// A count of things, item (dimensionless): a queue's length, a number of samples. An API that counts something of its
/// own (blocks, sectors, chunks) declares its own marker like [Unit]'s example, so the count shows as "blocks".
#[cfg(feature = "counting")]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Item;

// Compound

/// `A / B`, a unit outside the named list: `Per<Torque, AngularVelocity>` is N·m/(rad/s). Not normalized,
/// `Per<Meter, Second>` is not [Speed] (convert with `to_si_f64` / `from_f32`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Per<A, B> {
    _a: PhantomData<A>,
    _b: PhantomData<B>,
}

/// `A · B`, a unit outside the named list: `Times<AngularVelocity, AngularVelocity>` is (rad/s)·(rad/s).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Times<A, B> {
    _a: PhantomData<A>,
    _b: PhantomData<B>,
}

impl<A: Unit, B: Unit> Unit for Per<A, B> {
    const DIM: Dim = A::DIM.div(B::DIM);
    const EXPR: &'static UnitExpr = &UnitExpr::Div(A::EXPR, B::EXPR);
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Div(
        RefBox::Ref { value: A::SI_EXPR },
        RefBox::Ref { value: B::SI_EXPR },
    );
}

impl<A: Unit, B: Unit> Unit for Times<A, B> {
    const DIM: Dim = A::DIM.mul(B::DIM);
    const EXPR: &'static UnitExpr = &UnitExpr::Mul(A::EXPR, B::EXPR);
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Mul(
        RefBox::Ref { value: A::SI_EXPR },
        RefBox::Ref { value: B::SI_EXPR },
    );
}

/// `impl Unit` for each marker, and [ALL] from the same lines.
macro_rules! units {
    ($($feature:literal $ty:ident $symbol:literal [$($d:ident $e:literal),*] $scale:expr;)*) => {
        $(
            #[cfg(feature = $feature)]
            impl Unit for $ty {
                const DIM: Dim = {
                    #[allow(unused_mut)]
                    let mut dim = Dim::ONE;
                    $(dim.$d = crate::unit::exp($e);)*
                    dim
                };
                const EXPR: &'static UnitExpr = &UnitExpr::Named {
                    name: stringify!($ty),
                    symbol: $symbol,
                    dim: Self::DIM,
                    scale: $scale,
                };
            }
        )*

        /// Every named unit of this crate that is enabled, in the order of the docs.
        pub static ALL: &[&UnitExpr] = &[
            $(
                #[cfg(feature = $feature)]
                <$ty as Unit>::EXPR,
            )*
        ];
    };
}

units! {
    "time" Second "s" [s 1] Scale::Coherent;
    "time" Hertz "Hz" [s -1] Scale::Coherent;
    "time" HertzPerSecond "Hz/s" [s -2] Scale::Coherent;
    "time" Baud "Bd" [s -1] Scale::Coherent;

    "electrical" Ampere "A" [a 1] Scale::Coherent;
    "electrical" Volt "V" [kg 1, m 2, s -3, a -1] Scale::Coherent;
    "electrical" Ohm "Ω" [kg 1, m 2, s -3, a -2] Scale::Coherent;
    "electrical" Farad "F" [kg -1, m -2, s 4, a 2] Scale::Coherent;
    "electrical" Siemens "S" [kg -1, m -2, s 3, a 2] Scale::Coherent;
    "electrical" Henry "H" [kg 1, m 2, s -2, a -2] Scale::Coherent;
    "electrical" Coulomb "C" [s 1, a 1] Scale::Coherent;
    "electrical" Watt "W" [kg 1, m 2, s -3] Scale::Coherent;
    "electrical" Weber "Wb" [kg 1, m 2, s -2, a -1] Scale::Coherent;
    "electrical" Tesla "T" [kg 1, s -2, a -1] Scale::Coherent;

    "mechanical" Meter "m" [m 1] Scale::Coherent;
    "mechanical" Gram "g" [kg 1] Scale::Pow10(-3);
    "mechanical" Newton "N" [kg 1, m 1, s -2] Scale::Coherent;
    "mechanical" Pascal "Pa" [kg 1, m -1, s -2] Scale::Coherent;
    "mechanical" Joule "J" [kg 1, m 2, s -2] Scale::Coherent;
    "mechanical" Torque "N·m" [kg 1, m 2, s -2] Scale::Coherent;
    "mechanical" Litre "L" [m 3] Scale::Pow10(-3);
    "mechanical" VolumetricFlow "m³/s" [m 3, s -1] Scale::Coherent;
    "mechanical" Speed "m/s" [m 1, s -1] Scale::Coherent;
    "mechanical" Acceleration "m/s²" [m 1, s -2] Scale::Coherent;
    "mechanical" Jerk "m/s³" [m 1, s -3] Scale::Coherent;
    "mechanical" Snap "m/s⁴" [m 1, s -4] Scale::Coherent;
    "mechanical" Yank "N/s" [kg 1, m 1, s -3] Scale::Coherent;

    "angle" Radian "rad" [] Scale::Coherent;
    "angle" Degree "°" [] Scale::Factor(core::f64::consts::PI / 180.0);
    "angle" Steradian "sr" [] Scale::Coherent;
    "angle" AngularVelocity "rad/s" [s -1] Scale::Coherent;
    "angle" AngularAcceleration "rad/s²" [s -2] Scale::Coherent;

    "thermal" Kelvin "K" [k 1] Scale::Coherent;
    "thermal" DegreeCelsius "°C" [k 1] Scale::Affine { factor: 1.0, offset: 273.15 };

    "chemical" Mole "mol" [mol 1] Scale::Coherent;
    "chemical" Katal "kat" [mol 1, s -1] Scale::Coherent;

    "photometric" Candela "cd" [cd 1] Scale::Coherent;
    "photometric" Lumen "lm" [cd 1] Scale::Coherent;
    "photometric" Lux "lx" [cd 1, m -2] Scale::Coherent;

    "radiation" Becquerel "Bq" [s -1] Scale::Coherent;
    "radiation" Gray "Gy" [m 2, s -2] Scale::Coherent;
    "radiation" Sievert "Sv" [m 2, s -2] Scale::Coherent;
    "radiation" ElectronVolt "eV" [kg 1, m 2, s -2] Scale::Factor(1.602176634e-19);

    "dimensionless" Ratio "" [] Scale::Coherent;

    "information" Bit "bit" [] Scale::Coherent;
    "information" Byte "B" [] Scale::Factor(8.0);

    "counting" Item "item" [] Scale::Coherent;
}

/// The named unit of this crate called `name` (the marker's name, `"Volt"`), if its feature is enabled: what a
/// dynamic client needs to show a `Quantity<Volt>` it only knows from introspection data.
pub fn by_name(name: &str) -> Option<&'static UnitExpr> {
    ALL.iter()
        .copied()
        .find(|expr| matches!(expr, UnitExpr::Named { name: n, .. } if *n == name))
}
