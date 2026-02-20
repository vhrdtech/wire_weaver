#![cfg_attr(not(feature = "std"), no_std)]

mod convert;

use shrink_wrap::prelude::*;
pub use ww_numeric;
use ww_numeric::NumericValue;

pub enum SIExpr<'i> {
    Num(NumericValue),
    Unit { prefix: Prefix, unit: Unit<'i> },
    Mul((RefBox<'i, SIExpr<'i>>, RefBox<'i, SIExpr<'i>>)),
    Div((RefBox<'i, SIExpr<'i>>, RefBox<'i, SIExpr<'i>>)),
}

#[derive_shrink_wrap]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[ww_repr(unib32)]
#[self_describing]
pub enum Prefix {
    Unit,
    Milli,
    Micro,
    Nano,
    Pico,
    Kilo,
    Mega,
    Giga,

    Deca,
    Hecto,
    Tera,
    Peta,
    Exa,
    Zetta,
    Yotta,
    Ronna,
    Quetta,

    Deci,
    Centi,
    Femto,
    Atto,
    Zepto,
    Yocto,
    Ronto,
    Quecto,
}

impl Into<i8> for Prefix {
    fn into(self) -> i8 {
        match self {
            Prefix::Quetta => 30,
            Prefix::Ronna => 27,
            Prefix::Yotta => 24,
            Prefix::Zetta => 21,
            Prefix::Exa => 18,
            Prefix::Peta => 15,
            Prefix::Tera => 12,
            Prefix::Giga => 9,
            Prefix::Mega => 6,
            Prefix::Kilo => 3,
            Prefix::Hecto => 2,
            Prefix::Deca => 1,
            Prefix::Unit => 0,
            Prefix::Deci => -1,
            Prefix::Centi => -2,
            Prefix::Milli => -3,
            Prefix::Micro => -6,
            Prefix::Nano => -9,
            Prefix::Pico => -12,
            Prefix::Femto => -15,
            Prefix::Atto => -18,
            Prefix::Zepto => -21,
            Prefix::Yocto => -24,
            Prefix::Ronto => -27,
            Prefix::Quecto => -30,
        }
    }
}

impl TryFrom<i8> for Prefix {
    type Error = ();

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            30 => Ok(Prefix::Quetta),
            27 => Ok(Prefix::Ronna),
            24 => Ok(Prefix::Yotta),
            21 => Ok(Prefix::Zetta),
            18 => Ok(Prefix::Exa),
            15 => Ok(Prefix::Peta),
            12 => Ok(Prefix::Tera),
            9 => Ok(Prefix::Giga),
            6 => Ok(Prefix::Mega),
            3 => Ok(Prefix::Kilo),
            2 => Ok(Prefix::Hecto),
            1 => Ok(Prefix::Deca),
            0 => Ok(Prefix::Unit),
            -1 => Ok(Prefix::Deci),
            -2 => Ok(Prefix::Centi),
            -3 => Ok(Prefix::Milli),
            -6 => Ok(Prefix::Micro),
            -9 => Ok(Prefix::Nano),
            -12 => Ok(Prefix::Pico),
            -15 => Ok(Prefix::Femto),
            -18 => Ok(Prefix::Atto),
            -21 => Ok(Prefix::Zepto),
            -24 => Ok(Prefix::Yocto),
            -27 => Ok(Prefix::Ronto),
            -30 => Ok(Prefix::Quecto),
            _ => Err(()),
        }
    }
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[owned = "std"]
#[final_structure]
pub enum BaseUnit<'i> {
    Unitless,
    Second,
    Ampere,
    Volt,
    Ohm,
    Hertz,
    Watt,
    Kelvin,

    Meter,
    Kilogram,
    Mole,
    Candela,
    Radian,
    Steradian,
    Newton,
    Pascal,
    Joule,
    Coulomb,
    Farad,
    Siemens,
    Weber,
    Tesla,
    Henry,
    DegreeCelsius,
    Lumen,
    Lux,
    Becquerel,
    Gray,
    Sievert,
    Katal,

    Named {
        name: &'i str,
        symbol: &'i str,
        exp: SIExp,
    },
}

pub struct Unit<'i> {
    pub prefix: Prefix,
    pub base: BaseUnit<'i>,
    pub exp: i8,
}

#[derive_shrink_wrap]
pub struct SIExp {
    pub second: INib8P3,
    pub meter: INib8P3,
    pub kilogram: INib8P3,
    pub ampere: INib8P3,
    #[flag]
    candela: bool,
    #[flag]
    mole: bool,
    #[flag]
    kelvin: bool,
    pub kelvin: Option<INib8P3>,
    pub mole: Option<INib8P3>,
    pub candela: Option<INib8P3>,
}

/// Any SI quantity with prefix (unit, milli, micro, kilo, mega, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
pub struct Quantity<'i> {
    pub number: NumericValue,
    pub unit: Unit<'i>,
}

// Base units

/// Time quantity in seconds with prefix (s, ms, μs, ns, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Second {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Length quantity in meters with prefix (m, mm, μm, km, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Meter {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Mass quantity in grams with prefix (g, mg, μg, kg, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// Note that gram is expressed with Prefix::Milli (milli-kilo-gram)
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct KiloGram {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Electric current quantity in Amperes with prefix (A, mA, μA, kA, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Ampere {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Thermodynamic temperature quantity in Kelvin with prefix (K, mK, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Kelvin {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Amount of substance quantity in mole with prefix (mol, mmol, μmol, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Mole {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Luminous intensity quantity in candela with prefix (cd, mcd, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Candela {
    pub prefix: Prefix,
    pub value: NumericValue,
}

// Derived units

/// Frequency quantity in Hertz with prefix (Hz, mHz, kHz, MHz, GHz, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−1, 1/s
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Hertz {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Angle quantity in radians with prefix (rad, mrad, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: 1, m/m
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Radian {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Angle quantity in degree of arc with prefix (deg, mdeg, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: (π / 180) rad (≈ 17.5 mrad)
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Degree {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Solid angle quantity in steradian with prefix (sr, msr, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: 1, m2/m2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Steradian {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Force or weight quantity in Newtons with prefix (N, mN, kN, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m⋅s−2, kg⋅m/s2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Newton {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Pressure or stress quantity in Pascals with prefix (Pa, mPa, kPa, MPa, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m−1⋅s−2, N/m2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Pascal {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Energy, work or heat quantity in Joules with prefix (J, mJ, kJ, MJ, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−2, m⋅N, C⋅V, W⋅s
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Joule {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Power or radiant flux quantity in Watts with prefix (W, mW, kW, MW, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−3, J/s, V⋅A
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Watt {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Electric charge quantity in Coulombs with prefix (C, mC, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s⋅A, A⋅s, F⋅V
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Coulomb {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Voltage, electric potential or electromotive force quantity in Volts with prefix (V, mV, kV, MV, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−3⋅A−1, J/C, W/A
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Volt {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Capacitance quantity in Farads with prefix (F, pF, nF, uF, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg−1⋅m−2⋅s4⋅A2, C/V, s/Ω
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Farad {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Electrical resistance, reactance or impedance quantity in Ohms with prefix (Ω, kΩ, MΩ, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−3⋅A−2, V/A, 1/S
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Ohm {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Electrical admittance, conductance, susceptance quantity in Siemens with prefix (S, mS, kS, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg−1⋅m−2⋅s3⋅A2, A/V, 1/Ω
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Siemens {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Magnetic flux quantity in Webers with prefix (Wb, mWb, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−2⋅A−1, V⋅s, T⋅m2, J/A
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Weber {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Magnetic flux density quantity in Tesla with prefix (T, mT, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅s−2⋅A−1, N/(A⋅m), Wb/m2, V⋅s/m2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Tesla {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Inductance or permeance quantity in Henry with prefix (H, uH, mH, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: kg⋅m2⋅s−2⋅A−2, V⋅s/A, Wb/A, Ω⋅s
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Henry {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Temperature relative to -273.15K quantity in °C
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: K
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct DegreeCelsius {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Luminous flux quantity in candela (cd)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: cd, cd⋅sr
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Lumen {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Illuminance quantity in lux (lx)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: cd⋅m−2, lm/m2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Lux {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Radioactivity quantity in Becquerel (Bq)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−1, 1/s
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Becquerel {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Absorbed does quantity in Gray (Gy)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m2⋅s−2, J/kg
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Gray {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Equivalent dose quantity in Sievert (Sv)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m2⋅s−2, J/kg
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Sievert {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Catalytic activity quantity in katal (kat)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−1⋅mol, mol/s
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Katal {
    pub prefix: Prefix,
    pub value: NumericValue,
}

// Related units
/// Logarithmic ratio quantity
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct DeciBel {
    pub value: NumericValue,
}

/// Logarithmic ratio quantity relative to mW
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct DeciBelmW {
    pub value: NumericValue,
}

/// Volume quantity in liters with prefix (l, ml, μl, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: 0.001 m3
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Litre {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Energy quantity in electron-volts with prefix (eV, kEv, MEv, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: J, kg⋅m2⋅s−2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct ElectronVolt {
    pub prefix: Prefix,
    pub value: NumericValue,
}

// Kinematics
/// Speed quantity in meters per second with prefix (m/s, mm/s, km/s, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m⋅s−1
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Speed {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Acceleration quantity in meters per second squared with prefix (m/s^2, mm/s^2, km/s^2, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m⋅s−2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Acceleration {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Jerk or jolt quantity in meters per second cubed with prefix (m/s^3, mm/s^3, km/s^3, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m⋅s−3
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Jerk {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Snap or jounce quantity in meters per second to the fourth with prefix (m/s^4, mm/s^4, km/s^4, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m⋅s−4
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Snap {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Yank quantity in kilogram meters per second cubed with prefix (kg * m/s^3, kg * mm/s^3, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m⋅kg⋅s−3
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct Yank {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Angular velocity quantity in radians per second with prefix (rad/s, krad/s, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−1
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct AngularVelocity {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Angular acceleration quantity in radians per second squared with prefix (rad/s^2, krad/s^2, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct AngularAcceleration {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Frequency drift quantity in Hertz per second with prefix (Hz/s, kHz/s, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: s−2
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct HertzPerSecond {
    pub prefix: Prefix,
    pub value: NumericValue,
}

/// Volumetric flow quantity in cubic meters per second with prefix (m^3/s, mm^3/s, km^3/s, etc.)
/// backed by [NumericValue] which can be u8-u128, i8-i128, f32, f64, etc.
///
/// SI: m3⋅s−1
#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[final_structure]
pub struct VolumetricFlow {
    pub prefix: Prefix,
    pub value: NumericValue,
}

// Dynamic
pub struct INib8P3(i8);

impl INib8P3 {
    pub const fn new(_n: u8) -> Option<INib8P3> {
        todo!()
    }

    pub const fn minus_three() -> Self {
        INib8P3(-3 + 3)
    }
    pub const fn minus_two() -> Self {
        INib8P3(-2 + 3)
    }
    pub const fn minus_one() -> Self {
        INib8P3(-1 + 3)
    }
    pub const fn zero() -> Self {
        INib8P3(3)
    }
    pub const fn one() -> Self {
        INib8P3(1 + 3)
    }
    pub const fn two() -> Self {
        INib8P3(2 + 3)
    }
    pub const fn three() -> Self {
        INib8P3(3 + 3)
    }
    pub const fn four() -> Self {
        INib8P3(4 + 3)
    }

    pub const fn value(&self) -> i8 {
        self.0 - 3
    }
}

impl SerializeShrinkWrap for INib8P3 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, _wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        todo!()
    }
}

impl<'i> DeserializeShrinkWrap<'i> for INib8P3 {
    const ELEMENT_SIZE: ElementSize = <INib8P3 as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(_rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        todo!()
    }
}

impl DeserializeShrinkWrapOwned for INib8P3 {
    const ELEMENT_SIZE: ElementSize = <INib8P3 as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap_owned(_rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        todo!()
    }
}

#[macro_export]
macro_rules! quantity {
    ($value:literal s $value_ty:ident) => {
        ww_si::Second { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal m $value_ty:ident) => {
        ww_si::Meter { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal g $value_ty:ident) => {
        ww_si::Gram { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal mA $value_ty:ident) => {
        ww_si::Ampere { prefix: ww_si::Prefix::Milli, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal A $value_ty:ident) => {
        ww_si::Ampere { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal mV $value_ty:ident) => {
        ww_si::Volt { prefix: ww_si::Prefix::Milli, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal V $value_ty:ident) => {
        ww_si::Volt { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
    ($value:literal m/s $value_ty:ident) => {
        ww_si::Speed { prefix: ww_si::Prefix::Unit, value: ww_si::ww_numeric::value!(relative_path $value $value_ty) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_numeric::NumericBaseType;

    #[test]
    fn quantity_macro() {
        use crate as ww_si;
        let time = quantity!(5 s u32);
        assert_eq!(time.prefix, Prefix::Unit);
        assert_eq!(time.value.ty(), NumericBaseType::U32);
    }
}