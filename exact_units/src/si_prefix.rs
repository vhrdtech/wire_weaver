//! The SI prefixes as a value: what a prefix fixed in a type stands for at run time, and what a quantity whose prefix
//! is chosen at run time carries on the wire.

use crate::Magnitude;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// A scale chosen at run time: an SI prefix from quecto (10⁻³⁰) to quetta (10³⁰), a binary prefix (Ki .. Ti), or one
/// of three families of exact scales that aren't named: [Pow10](Prefix::Pow10) (`10⁻⁴`), [Pow2](Prefix::Pow2) (`2⁻¹⁵`,
/// a Q format) and [FullScale](Prefix::FullScale) (`1/65535`, a 16-bit duty).
///
/// Four bytes in memory (two for the tag, two for [Unknown](Prefix::Unknown)'s). On the wire a `UNib32` tag: the prefixes in use most (`Unit`, `Milli` .. `Giga`) take one
/// nibble, the other named ones a byte, and the three families one more byte for their parameter, so a scale takes 2
/// bytes at most and is self-contained. A family never holds a named prefix's scale (its parameter type refuses it,
/// also when read), so equal scales are equal prefixes. Other exact scales (`1/5`, `π/1000`) aren't a `Prefix`: a
/// value with one is an `AnyQuantity`.
///
/// Scale kinds added after `exact_units` 0.1 are tag-only (DER-25): their numbers are packed into the `UNib32` tag,
/// never bytes after it, so an older reader gets [Unknown(tag)](Prefix::Unknown), skips the self-describing value
/// and reads the rest of the message.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        self_describing,
        series(no_store),
        ww_repr = unib32,
        niche
    )
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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

    /// 2¹⁰
    Kibi,
    /// 2²⁰
    Mebi,
    /// 2³⁰
    Gibi,
    /// 2⁴⁰
    Tebi,

    /// `10^e` that isn't an SI prefix: `10⁻⁴`, `10⁵`.
    Pow10(#[cfg_attr(feature = "shrink_wrap", wire(i8))] Pow10Exp),
    /// `2^e` that isn't a binary prefix: `2⁻¹⁵` (a Q15 step, an RTC tick), `2⁸`.
    Pow2(#[cfg_attr(feature = "shrink_wrap", wire(i8))] Pow2Exp),
    /// `1/(2^n − 1)`: the step of an `n`-bit full scale, `1/65535` for a 16-bit duty.
    FullScale(#[cfg_attr(feature = "shrink_wrap", wire(u8))] FullScaleBits),
    /// A scale from a newer version, its tag (DER-25): no scale known here, [magnitude](Self::magnitude) is `None`.
    /// Read past and passed on as it is; not in a `Series`, which can't know its fields.
    #[cfg_attr(feature = "shrink_wrap", future_version)]
    Unknown(u16),
}

/// The exponent of [Prefix::Pow10]: -63..=63, not an SI prefix's (`-3` is `Milli`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pow10Exp(i8);

impl Pow10Exp {
    /// `None` for an SI prefix's exponent and out of -63..=63.
    pub const fn new(e: i8) -> Option<Self> {
        if Prefix::si_from_pow10(e).is_none() && -63 <= e && e <= 63 {
            Some(Pow10Exp(e))
        } else {
            None
        }
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// The exponent of [Prefix::Pow2]: -63..=63, not 0 or a binary prefix's (10, 20, 30, 40).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pow2Exp(i8);

impl Pow2Exp {
    /// `None` for 0, a binary prefix's exponent and out of -63..=63.
    pub const fn new(e: i8) -> Option<Self> {
        if !matches!(e, 0 | 10 | 20 | 30 | 40) && -63 <= e && e <= 63 {
            Some(Pow2Exp(e))
        } else {
            None
        }
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// The bits of [Prefix::FullScale], 2..=32 (1 bit would be `Unit`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FullScaleBits(u8);

impl FullScaleBits {
    /// `None` out of 2..=32.
    pub const fn new(n: u8) -> Option<Self> {
        if 2 <= n && n <= 32 {
            Some(FullScaleBits(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// `1/(2^n − 1)`, built from the primes of `2^n − 1` (no trial division).
    pub const fn magnitude(self) -> Magnitude {
        let b = (self.0 - 2) as usize;
        let start = if b == 0 { 0 } else { FULL_ENDS[b - 1] as usize };
        let (_, tail) = FULL_PRIMES.split_at(start);
        let (primes, _) = tail.split_at(FULL_ENDS[b] as usize - start);
        Magnitude::recip_of_primes(primes)
    }

    /// The bits whose full scale step is `m`, `None` when `m` isn't one.
    pub fn of(m: &Magnitude) -> Option<Self> {
        match m.to_ratio() {
            Some((1, den)) if (den + 1).is_power_of_two() => {
                FullScaleBits::new((den + 1).trailing_zeros() as u8)
            }
            _ => None,
        }
    }
}

/// The primes of `2^n - 1` for `n` in 2..=32, a power repeated, back to back; [FULL_ENDS] marks where each ends.
const FULL_PRIMES: [u32; 97] = [
    3, // 2
    7, // 3
    3, 5,  // 4
    31, // 5
    3, 3, 7,   // 6
    127, // 7
    3, 5, 17, // 8
    7, 73, // 9
    3, 11, 31, // 10
    23, 89, // 11
    3, 3, 5, 7, 13,   // 12
    8191, // 13
    3, 43, 127, // 14
    7, 31, 151, // 15
    3, 5, 17, 257,    // 16
    131071, // 17
    3, 3, 3, 7, 19, 73,     // 18
    524287, // 19
    3, 5, 5, 11, 31, 41, // 20
    7, 7, 127, 337, // 21
    3, 23, 89, 683, // 22
    47, 178481, // 23
    3, 3, 5, 7, 13, 17, 241, // 24
    31, 601, 1801, // 25
    3, 2731, 8191, // 26
    7, 73, 262657, // 27
    3, 5, 29, 43, 113, 127, // 28
    233, 1103, 2089, // 29
    3, 3, 7, 11, 31, 151, 331,        // 30
    2147483647, // 31
    3, 5, 17, 257, 65537, // 32
];

/// The end of each `n`'s primes in [FULL_PRIMES], `n` = 2 first.
const FULL_ENDS: [u8; 31] = [
    1, 2, 4, 5, 8, 9, 12, 14, 17, 19, 24, 25, 28, 31, 35, 36, 42, 43, 49, 53, 57, 59, 66, 69, 72,
    75, 81, 84, 91, 92, 97,
];

#[cfg(feature = "shrink_wrap")]
mod wire {
    use super::{FullScaleBits, Pow2Exp, Pow10Exp};
    use shrink_wrap::{Error, WireAs};

    impl WireAs<Pow10Exp> for i8 {
        fn from_plain(value: Pow10Exp) -> Result<Self, Error> {
            Ok(value.get())
        }

        fn into_plain(self) -> Result<Pow10Exp, Error> {
            Pow10Exp::new(self).ok_or(Error::SubtypeOutOfRange)
        }
    }

    impl WireAs<Pow2Exp> for i8 {
        fn from_plain(value: Pow2Exp) -> Result<Self, Error> {
            Ok(value.get())
        }

        fn into_plain(self) -> Result<Pow2Exp, Error> {
            Pow2Exp::new(self).ok_or(Error::SubtypeOutOfRange)
        }
    }

    impl WireAs<FullScaleBits> for u8 {
        fn from_plain(value: FullScaleBits) -> Result<Self, Error> {
            Ok(value.get())
        }

        fn into_plain(self) -> Result<FullScaleBits, Error> {
            FullScaleBits::new(self).ok_or(Error::SubtypeOutOfRange)
        }
    }
}

impl Prefix {
    /// Every named prefix, in the order of the enum (by discriminant).
    pub const ALL: [Prefix; 29] = [
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
        Prefix::Kibi,
        Prefix::Mebi,
        Prefix::Gibi,
        Prefix::Tebi,
    ];

    /// The power of ten the scale is, for an SI prefix and `Pow10` (`Some(-4)`), `None` for any other scale.
    pub const fn pow10(&self) -> Option<i8> {
        match self {
            Prefix::Pow10(e) => Some(e.get()),
            p => p.si_pow10(),
        }
    }

    /// The power of ten of an SI prefix, `None` for any other (even `Pow10`, see [pow10](Self::pow10)): the fast path
    /// of arithmetic across prefixes, no [Magnitude] built.
    pub const fn si_pow10(&self) -> Option<i8> {
        Some(match self {
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
            Prefix::Kibi
            | Prefix::Mebi
            | Prefix::Gibi
            | Prefix::Tebi
            | Prefix::Pow10(_)
            | Prefix::Pow2(_)
            | Prefix::FullScale(_)
            | Prefix::Unknown(_) => return None,
        })
    }

    /// The SI prefix that is `10^e`, `None` when there is none (`10⁻⁴`: [from_pow10](Self::from_pow10) gives it as
    /// `Pow10`).
    pub const fn si_from_pow10(e: i8) -> Option<Prefix> {
        Some(match e {
            30 => Prefix::Quetta,
            27 => Prefix::Ronna,
            24 => Prefix::Yotta,
            21 => Prefix::Zetta,
            18 => Prefix::Exa,
            15 => Prefix::Peta,
            12 => Prefix::Tera,
            9 => Prefix::Giga,
            6 => Prefix::Mega,
            3 => Prefix::Kilo,
            2 => Prefix::Hecto,
            1 => Prefix::Deca,
            0 => Prefix::Unit,
            -1 => Prefix::Deci,
            -2 => Prefix::Centi,
            -3 => Prefix::Milli,
            -6 => Prefix::Micro,
            -9 => Prefix::Nano,
            -12 => Prefix::Pico,
            -15 => Prefix::Femto,
            -18 => Prefix::Atto,
            -21 => Prefix::Zepto,
            -24 => Prefix::Yocto,
            -27 => Prefix::Ronto,
            -30 => Prefix::Quecto,
            _ => return None,
        })
    }

    /// `10^e` as a prefix: the SI one, else `Pow10`. `None` for `|e| > 63`.
    pub const fn from_pow10(e: i8) -> Option<Prefix> {
        match Self::si_from_pow10(e) {
            Some(p) => Some(p),
            None => match Pow10Exp::new(e) {
                Some(e) => Some(Prefix::Pow10(e)),
                None => None,
            },
        }
    }

    /// `2^e` as a prefix: `Unit`, a binary one, else `Pow2`. `None` for `|e| > 63`.
    pub const fn from_pow2(e: i8) -> Option<Prefix> {
        Some(match e {
            0 => Prefix::Unit,
            10 => Prefix::Kibi,
            20 => Prefix::Mebi,
            30 => Prefix::Gibi,
            40 => Prefix::Tebi,
            e => match Pow2Exp::new(e) {
                Some(e) => Prefix::Pow2(e),
                None => return None,
            },
        })
    }

    /// The scale, exact: `10⁻³` for `Milli`, `2¹⁰` for `Kibi`; `None` for [Unknown](Prefix::Unknown).
    pub fn magnitude(&self) -> Option<Magnitude> {
        Some(match self {
            Prefix::Kibi => Magnitude::pow2(10),
            Prefix::Mebi => Magnitude::pow2(20),
            Prefix::Gibi => Magnitude::pow2(30),
            Prefix::Tebi => Magnitude::pow2(40),
            Prefix::Pow10(e) => Magnitude::pow10(e.get()),
            Prefix::Pow2(e) => Magnitude::pow2(e.get()),
            Prefix::FullScale(n) => n.magnitude(),
            Prefix::Unknown(_) => return None,
            // an SI prefix
            p => Magnitude::pow10(p.si_pow10().unwrap_or(0)),
        })
    }

    /// The named prefix whose scale is `m`, `None` when no named one is.
    pub fn named_of(m: &Magnitude) -> Option<Prefix> {
        if let Some(e) = m.as_pow10() {
            return Self::si_from_pow10(e);
        }
        match m.pow10_pow2_x2() {
            (0, 20) if *m == Magnitude::pow2(10) => Some(Prefix::Kibi),
            (0, 40) if *m == Magnitude::pow2(20) => Some(Prefix::Mebi),
            (0, 60) if *m == Magnitude::pow2(30) => Some(Prefix::Gibi),
            (0, 80) if *m == Magnitude::pow2(40) => Some(Prefix::Tebi),
            _ => None,
        }
    }

    /// The prefix whose scale is `m`: the named one when there is one, else `Pow10`, `Pow2` or `FullScale`. `None`
    /// for any other scale (`1/5`, `π/1000`): a value with it is an `AnyQuantity`.
    pub fn from_magnitude(m: &Magnitude) -> Option<Prefix> {
        if let Some(p) = Self::named_of(m) {
            return Some(p);
        }
        if let Some(e) = m.as_pow10() {
            return Self::from_pow10(e);
        }
        let (pow10_x2, pow2_x2) = m.pow10_pow2_x2();
        let irr = m.irrational_x2();
        if pow10_x2 == 0
            && pow2_x2 % 2 == 0
            && m.primes().next().is_none()
            && (irr.pi, irr.ln2, irr.ln10) == (0, 0, 0)
        {
            return Self::from_pow2(pow2_x2 / 2);
        }
        FullScaleBits::of(m).map(Prefix::FullScale)
    }

    /// Whether this is one of the named prefixes (not `Pow10`, `Pow2`, `FullScale` or `Unknown`).
    pub const fn is_named(&self) -> bool {
        !matches!(
            self,
            Prefix::Pow10(_) | Prefix::Pow2(_) | Prefix::FullScale(_) | Prefix::Unknown(_)
        )
    }

    /// The parameter of a family: the exponent of [Pow10](Prefix::Pow10) and [Pow2](Prefix::Pow2), the bits of
    /// [FullScale](Prefix::FullScale); `None` for a named prefix. With [name](Self::name), the prefix as a dynamic
    /// client sees it (`{"Pow10": -4}`).
    pub const fn param(&self) -> Option<i8> {
        match *self {
            Prefix::Pow10(e) => Some(e.get()),
            Prefix::Pow2(e) => Some(e.get()),
            Prefix::FullScale(n) => Some(n.get() as i8),
            _ => None,
        }
    }

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
            Prefix::Kibi => "Kibi",
            Prefix::Mebi => "Mebi",
            Prefix::Gibi => "Gibi",
            Prefix::Tebi => "Tebi",
            Prefix::Pow10(_) => "Pow10",
            Prefix::Pow2(_) => "Pow2",
            Prefix::FullScale(_) => "FullScale",
            Prefix::Unknown(_) => "Unknown",
        }
    }

    /// The named prefix whose [name](Self::name) is `name`.
    pub fn from_name(name: &str) -> Option<Prefix> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// `x` in units of this prefix as a number in the unit itself: `Prefix::Milli.scale_f64(3300.0)` is 3.3, one
    /// multiplication or division by an exact power of ten (or by the exact fraction of another scale). NaN for
    /// [Unknown](Prefix::Unknown).
    pub fn scale_f64(&self, x: f64) -> f64 {
        match (self.si_pow10(), self.magnitude()) {
            (Some(e), _) => crate::num::scale_f64(x, e),
            (None, Some(m)) => m.apply_f64(x),
            (None, None) => f64::NAN,
        }
    }

    /// `x` in units of this prefix as a number in the unit itself, in `f32`, as [scale_f64](Self::scale_f64).
    pub fn scale_f32(&self, x: f32) -> f32 {
        match self.si_pow10() {
            Some(e) => crate::num::scale_f32(x, e),
            None => self.scale_f64(x as f64) as f32,
        }
    }

    /// The symbol: `m`, `µ`, `k`, `da`, `Ki`; empty for `Unit`, the families and `Unknown`, which
    /// [Display](core::fmt::Display) writes as their magnitude (`·1/65535`) or tag (`·<scale 1000>`).
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
            Prefix::Kibi => "Ki",
            Prefix::Mebi => "Mi",
            Prefix::Gibi => "Gi",
            Prefix::Tebi => "Ti",
            Prefix::Pow10(_) | Prefix::Pow2(_) | Prefix::FullScale(_) | Prefix::Unknown(_) => "",
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
            p @ (Prefix::Pow10(_) | Prefix::Pow2(_) | Prefix::FullScale(_)) => {
                if let Some(m) = p.magnitude() {
                    defmt::write!(f, " {}", m)
                }
            }
            Prefix::Unknown(tag) => defmt::write!(f, " ·<scale {=u16}>", tag),
            p => match p.si_pow10() {
                Some(e) => defmt::write!(f, "e{=i8}", e),
                None => defmt::write!(f, " {=str}", p.symbol()),
            },
        }
    }
}

/// The symbol, for the families the magnitude, for a scale from a newer version its tag: `m`, `Ki`, `·10⁻⁴`,
/// `·1/65535`, `·<scale 1000>`.
impl core::fmt::Display for Prefix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.is_named(), self.magnitude()) {
            (true, _) => f.write_str(self.symbol()),
            (false, Some(m)) => core::fmt::Display::fmt(&m, f),
            (false, None) => write!(f, "·<scale {}>", self.discriminant()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_four_bytes() {
        assert_eq!(size_of::<Prefix>(), 4);
        assert_eq!(size_of::<Option<Prefix>>(), 4);
    }

    #[test]
    fn named_and_family_scales() {
        for p in Prefix::ALL {
            assert_eq!(Prefix::from_magnitude(&p.magnitude().unwrap()), Some(p));
            assert_eq!(Prefix::named_of(&p.magnitude().unwrap()), Some(p));
        }
        let duty = Prefix::from_magnitude(&Magnitude::ratio(1, 65535)).unwrap();
        assert_eq!(duty, Prefix::FullScale(FullScaleBits::new(16).unwrap()));
        assert_eq!(std::format!("{duty}"), "·1/65535");
        assert_eq!(Prefix::from_pow10(-4).unwrap().pow10(), Some(-4));
        assert_eq!(Prefix::from_pow10(-3), Some(Prefix::Milli));
        assert_eq!(Prefix::from_pow2(10), Some(Prefix::Kibi));
        assert_eq!(
            Prefix::from_magnitude(&Magnitude::pow2(-15)),
            Some(Prefix::Pow2(Pow2Exp::new(-15).unwrap()))
        );
        assert_eq!(Prefix::Kibi.scale_f64(2.0), 2048.0);
        assert_eq!(Prefix::Kibi.pow10(), None);
        // 1/3 is the step of a 2-bit full scale; 1/5 is none of the families: an AnyQuantity
        assert_eq!(
            Prefix::from_magnitude(&Magnitude::ratio(1, 3)),
            Some(Prefix::FullScale(FullScaleBits::new(2).unwrap()))
        );
        assert_eq!(Prefix::from_magnitude(&Magnitude::ratio(1, 5)), None);
        assert_eq!(
            Prefix::from_magnitude(&Magnitude::PI.mul(Magnitude::pow2(-15))),
            None
        );
    }

    #[test]
    fn every_family_member_round_trips() {
        for e in i8::MIN..=i8::MAX {
            if let Some(p) = Prefix::from_pow10(e) {
                assert_eq!(
                    Prefix::from_magnitude(&p.magnitude().unwrap()),
                    Some(p),
                    "10^{e}"
                );
            }
            if let Some(p) = Prefix::from_pow2(e) {
                assert_eq!(
                    Prefix::from_magnitude(&p.magnitude().unwrap()),
                    Some(p),
                    "2^{e}"
                );
            }
        }
        for n in 2..=32 {
            let p = Prefix::FullScale(FullScaleBits::new(n).unwrap());
            assert_eq!(p.magnitude(), Some(Magnitude::ratio(1, (1u64 << n) - 1)));
            assert_eq!(Prefix::from_magnitude(&p.magnitude().unwrap()), Some(p));
        }
        assert_eq!(Pow10Exp::new(-3), None);
        assert_eq!(Pow2Exp::new(20), None);
        assert_eq!(FullScaleBits::new(33), None);
    }
}

/// A `Series` store and observe cells keep a prefix as a `u16`: its discriminant in the high byte, a family's
/// parameter in the low one. Rows of one named prefix are written column-wise (its tag run), rows with a family
/// item by item. Any [Unknown](Prefix::Unknown) is [UNKNOWN_CODE] and comes back as `Unknown(u16::MAX)`: its tag
/// doesn't fit the code, and a `Series` refuses the item either way.
#[cfg(feature = "shrink_wrap")]
mod series_store {
    use super::{FullScaleBits, Pow2Exp, Pow10Exp, Prefix};
    use core::mem::MaybeUninit;
    use core::ops::Range;
    use shrink_wrap::Error;
    use shrink_wrap::series::{
        BitSink, Level, SeriesCells, SeriesPolicy, SeriesStore, write_enum_tags,
    };

    /// The first code of a family: named prefixes are below it.
    const FAMILY_CODE: u16 = (Prefix::ALL.len() as u16) << 8;
    /// The code of any `Unknown`, above every other: its high byte is no discriminant of this version.
    const UNKNOWN_CODE: u16 = u16::MAX;

    impl Prefix {
        #[inline(always)]
        fn code(&self) -> u16 {
            if let Prefix::Unknown(_) = self {
                return UNKNOWN_CODE;
            }
            let param = match self {
                Prefix::Pow10(e) => e.get() as u8,
                Prefix::Pow2(e) => e.get() as u8,
                Prefix::FullScale(n) => n.get(),
                _ => 0,
            };
            ((self.discriminant() as u16) << 8) | param as u16
        }

        #[inline(always)]
        fn from_code(code: u16) -> Prefix {
            if code == UNKNOWN_CODE {
                return Prefix::Unknown(u16::MAX);
            }
            let param = code as u8;
            let family = match (code >> 8) as usize {
                29 => Pow10Exp::new(param as i8).map(Prefix::Pow10),
                30 => Pow2Exp::new(param as i8).map(Prefix::Pow2),
                31 => FullScaleBits::new(param).map(Prefix::FullScale),
                tag => Prefix::ALL.get(tag).copied(),
            };
            // only codes of prefixes are stored, an empty slot is 0 (`Unit`)
            family.unwrap_or(Prefix::Unit)
        }
    }

    impl SeriesStore for Prefix {
        type Store<const N: usize> = <u16 as SeriesStore>::Store<N>;

        fn empty_store<const N: usize>() -> Self::Store<N> {
            u16::empty_store::<N>()
        }

        fn init_store<const N: usize>(
            slot: &mut MaybeUninit<Self::Store<N>>,
        ) -> &mut Self::Store<N> {
            u16::init_store::<N>(slot)
        }

        #[inline(always)]
        fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
            u16::set::<N>(store, i, &item.code());
        }

        #[inline(always)]
        fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
            Prefix::from_code(u16::get::<N>(store, i))
        }

        fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
            let first = u16::get::<N>(store, rows.start);
            first < FAMILY_CODE && rows.into_iter().all(|i| u16::get::<N>(store, i) == first)
        }

        fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
            store: &Self::Store<N>,
            rows: Range<usize>,
            sink: &mut S,
            _policy: &P,
            _level: Level,
        ) -> Result<(), Error> {
            // a named prefix is a unit variant: its tag run is all there is
            let tag = (u16::get::<N>(store, rows.start) >> 8) as u32;
            write_enum_tags(sink, core::iter::repeat_n(tag, rows.len()))
        }
    }

    // SAFETY: the cells of `u16`, read back through `from_code`, which maps any value to a prefix
    unsafe impl SeriesCells for Prefix {
        type Cells<const R: usize> = <u16 as SeriesCells>::Cells<R>;

        #[inline(always)]
        fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
            u16::put::<R>(cells, i, &item.code());
        }

        fn copy_rows<const R: usize, const N: usize>(
            cells: &Self::Cells<R>,
            from: usize,
            store: &mut Self::Store<N>,
            to: usize,
            n: usize,
        ) {
            u16::copy_rows::<R, N>(cells, from, store, to, n);
        }
    }
}
