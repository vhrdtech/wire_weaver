//! The SI prefixes as a value: what a prefix fixed in a type stands for at run time, and what a quantity whose prefix
//! is chosen at run time carries on the wire.

#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// An SI prefix, from quecto (10⁻³⁰) to quetta (10³⁰).
///
/// On the wire a `UNib32`: the prefixes in use most (`Unit`, `Milli` .. `Giga`) take one nibble.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        self_describing,
        series(cells),
        ww_repr = unib32
    )
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

impl From<Prefix> for i8 {
    fn from(val: Prefix) -> Self {
        match val {
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

impl Prefix {
    /// Every prefix, in the order of the enum (by discriminant).
    pub const ALL: [Prefix; 25] = [
        Prefix::Unit,
        Prefix::Milli,
        Prefix::Micro,
        Prefix::Nano,
        Prefix::Pico,
        Prefix::Kilo,
        Prefix::Mega,
        Prefix::Giga,
        Prefix::Deca,
        Prefix::Hecto,
        Prefix::Tera,
        Prefix::Peta,
        Prefix::Exa,
        Prefix::Zetta,
        Prefix::Yotta,
        Prefix::Ronna,
        Prefix::Quetta,
        Prefix::Deci,
        Prefix::Centi,
        Prefix::Femto,
        Prefix::Atto,
        Prefix::Zepto,
        Prefix::Yocto,
        Prefix::Ronto,
        Prefix::Quecto,
    ];

    /// The variant's name, `"Milli"`: how a dynamic client sees it in a decoded value.
    pub const fn name(&self) -> &'static str {
        match self {
            Prefix::Unit => "Unit",
            Prefix::Milli => "Milli",
            Prefix::Micro => "Micro",
            Prefix::Nano => "Nano",
            Prefix::Pico => "Pico",
            Prefix::Kilo => "Kilo",
            Prefix::Mega => "Mega",
            Prefix::Giga => "Giga",
            Prefix::Deca => "Deca",
            Prefix::Hecto => "Hecto",
            Prefix::Tera => "Tera",
            Prefix::Peta => "Peta",
            Prefix::Exa => "Exa",
            Prefix::Zetta => "Zetta",
            Prefix::Yotta => "Yotta",
            Prefix::Ronna => "Ronna",
            Prefix::Quetta => "Quetta",
            Prefix::Deci => "Deci",
            Prefix::Centi => "Centi",
            Prefix::Femto => "Femto",
            Prefix::Atto => "Atto",
            Prefix::Zepto => "Zepto",
            Prefix::Yocto => "Yocto",
            Prefix::Ronto => "Ronto",
            Prefix::Quecto => "Quecto",
        }
    }

    /// The prefix whose [name](Self::name) is `name`.
    pub fn from_name(name: &str) -> Option<Prefix> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// `x` in units of this prefix as a number in the unit itself: `Prefix::Milli.scale_f64(3300.0)` is 3.3, one
    /// multiplication or division by an exact power of ten.
    pub fn scale_f64(&self, x: f64) -> f64 {
        crate::num::scale_f64(x, (*self).into())
    }

    /// `x` in units of this prefix as a number in the unit itself, in `f32`, as [scale_f64](Self::scale_f64).
    pub fn scale_f32(&self, x: f32) -> f32 {
        crate::num::scale_f32(x, (*self).into())
    }

    /// The SI symbol: `m`, `µ`, `k`, `da`; empty for `Unit`.
    pub const fn symbol(&self) -> &'static str {
        match self {
            Prefix::Unit => "",
            Prefix::Milli => "m",
            Prefix::Micro => "µ",
            Prefix::Nano => "n",
            Prefix::Pico => "p",
            Prefix::Kilo => "k",
            Prefix::Mega => "M",
            Prefix::Giga => "G",
            Prefix::Deca => "da",
            Prefix::Hecto => "h",
            Prefix::Tera => "T",
            Prefix::Peta => "P",
            Prefix::Exa => "E",
            Prefix::Zetta => "Z",
            Prefix::Yotta => "Y",
            Prefix::Ronna => "R",
            Prefix::Quetta => "Q",
            Prefix::Deci => "d",
            Prefix::Centi => "c",
            Prefix::Femto => "f",
            Prefix::Atto => "a",
            Prefix::Zepto => "z",
            Prefix::Yocto => "y",
            Prefix::Ronto => "r",
            Prefix::Quecto => "q",
        }
    }

    /// The prefix as a ratio is written with it, for `defmt`: ` %` for `Centi`, ` ‰`, ` ppm`, ` ppb`, `e-6` for
    /// the others, nothing for `Unit`. `Display` of a ratio writes the same.
    #[cfg(feature = "defmt")]
    pub fn defmt_ratio(prefix: Prefix, f: defmt::Formatter<'_>) {
        match prefix {
            Prefix::Unit => {}
            Prefix::Centi => defmt::write!(f, " %"),
            Prefix::Milli => defmt::write!(f, " ‰"),
            Prefix::Micro => defmt::write!(f, " ppm"),
            Prefix::Nano => defmt::write!(f, " ppb"),
            p => defmt::write!(f, "e{=i8}", i8::from(p)),
        }
    }
}
