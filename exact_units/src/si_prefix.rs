//! The SI prefixes as a value: what a prefix fixed in a type stands for at run time, and what a quantity whose prefix
//! is chosen at run time carries on the wire.

use crate::{ExactScale, Magnitude};
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// A scale chosen at run time: an SI prefix from quecto (10⁻³⁰) to quetta (10³⁰), a binary prefix (Ki .. Ti), or
/// any other exact scale ([Exact](Prefix::Exact): `1/65535`, `10⁻⁴`, `π/2¹⁵`).
///
/// One byte in memory. On the wire a `UNib32`: the prefixes in use most (`Unit`, `Milli` .. `Giga`) take one nibble,
/// `Exact` its tag and the full [Magnitude], so every reader gets the exact scale. An `Exact` never holds a named
/// prefix's scale ([from_magnitude](Self::from_magnitude) gives the named one), so equal scales are equal prefixes.
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

    /// 2¹⁰
    Kibi,
    /// 2²⁰
    Mebi,
    /// 2³⁰
    Gibi,
    /// 2⁴⁰
    Tebi,

    /// An exact scale that isn't a named prefix, by a one-byte handle; the full magnitude on the wire.
    Exact(#[cfg_attr(feature = "shrink_wrap", wire(Magnitude))] ExactScale),
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

    /// The power of ten the scale is, for an SI prefix and an `Exact` power of ten (`Some(-4)`), `None` for any other
    /// scale.
    pub fn pow10(&self) -> Option<i8> {
        match self {
            Prefix::Exact(s) => s.magnitude().as_pow10(),
            p => p.si_pow10(),
        }
    }

    /// The power of ten of an SI prefix, `None` for a binary one and for `Exact` (even an `Exact` power of ten, see
    /// [pow10](Self::pow10)): the fast path of arithmetic across prefixes, no [Magnitude] built.
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
            Prefix::Kibi | Prefix::Mebi | Prefix::Gibi | Prefix::Tebi | Prefix::Exact(_) => {
                return None;
            }
        })
    }

    /// The SI prefix that is `10^e`, `None` when there is none (`10⁻⁴`: [from_magnitude](Self::from_magnitude)
    /// gives it as `Exact`).
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

    /// `10^e` as a prefix: the SI one, else `Exact`. `None` without a handle for it (`|e| > 30` without `std`).
    pub fn from_pow10(e: i8) -> Option<Prefix> {
        match Self::si_from_pow10(e) {
            Some(p) => Some(p),
            None => Self::from_magnitude(&Magnitude::checked_pow10(e)?),
        }
    }

    /// The scale, exact: `10⁻³` for `Milli`, `2¹⁰` for `Kibi`.
    pub fn magnitude(&self) -> Magnitude {
        match self {
            Prefix::Kibi => Magnitude::pow2(10),
            Prefix::Mebi => Magnitude::pow2(20),
            Prefix::Gibi => Magnitude::pow2(30),
            Prefix::Tebi => Magnitude::pow2(40),
            Prefix::Exact(s) => s.magnitude(),
            // an SI prefix
            p => Magnitude::pow10(p.si_pow10().unwrap_or(0)),
        }
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

    /// The prefix whose scale is `m`: the named one when there is one, else `Exact`. `None` when `m` has no handle
    /// (see [ExactScale]: without `std` for a scale outside its computed ones).
    pub fn from_magnitude(m: &Magnitude) -> Option<Prefix> {
        match Self::named_of(m) {
            Some(p) => Some(p),
            None => ExactScale::of(m).map(Prefix::Exact),
        }
    }

    /// Whether this is one of the named prefixes (not `Exact`).
    pub const fn is_named(&self) -> bool {
        !matches!(self, Prefix::Exact(_))
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
            Prefix::Exact(_) => "Exact",
        }
    }

    /// The named prefix whose [name](Self::name) is `name`.
    pub fn from_name(name: &str) -> Option<Prefix> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// `x` in units of this prefix as a number in the unit itself: `Prefix::Milli.scale_f64(3300.0)` is 3.3, one
    /// multiplication or division by an exact power of ten (or by the exact fraction of another scale).
    pub fn scale_f64(&self, x: f64) -> f64 {
        match self.si_pow10() {
            Some(e) => crate::num::scale_f64(x, e),
            None => self.magnitude().apply_f64(x),
        }
    }

    /// `x` in units of this prefix as a number in the unit itself, in `f32`, as [scale_f64](Self::scale_f64).
    pub fn scale_f32(&self, x: f32) -> f32 {
        match self.si_pow10() {
            Some(e) => crate::num::scale_f32(x, e),
            None => self.magnitude().apply_f64(x as f64) as f32,
        }
    }

    /// The symbol: `m`, `µ`, `k`, `da`, `Ki`; empty for `Unit` and `Exact`, which [Display](core::fmt::Display)
    /// writes as its magnitude (`·1/65535`).
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
            Prefix::Exact(_) => "",
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
            Prefix::Exact(s) => defmt::write!(f, " {}", s),
            p => match p.si_pow10() {
                Some(e) => defmt::write!(f, "e{=i8}", e),
                None => defmt::write!(f, " {=str}", p.symbol()),
            },
        }
    }
}

/// The symbol, or for `Exact` its magnitude: `m`, `Ki`, `·1/65535`.
impl core::fmt::Display for Prefix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Prefix::Exact(s) => core::fmt::Display::fmt(s, f),
            p => f.write_str(p.symbol()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_one_byte() {
        assert_eq!(size_of::<Prefix>(), 1);
        assert_eq!(size_of::<Option<Prefix>>(), 1);
    }

    #[test]
    fn named_and_exact_scales() {
        for p in Prefix::ALL {
            assert_eq!(Prefix::from_magnitude(&p.magnitude()), Some(p));
            assert_eq!(Prefix::named_of(&p.magnitude()), Some(p));
        }
        let duty = Prefix::from_magnitude(&Magnitude::ratio(1, 65535)).unwrap();
        assert!(matches!(duty, Prefix::Exact(_)));
        assert_eq!(std::format!("{duty}"), "·1/65535");
        assert_eq!(Prefix::from_pow10(-4).unwrap().pow10(), Some(-4));
        assert_eq!(Prefix::from_pow10(-3), Some(Prefix::Milli));
        assert_eq!(Prefix::Kibi.scale_f64(2.0), 2048.0);
        assert_eq!(Prefix::Kibi.pow10(), None);
    }
}

/// A `Series` store and observe cells keep a prefix as one byte (the derive's would keep the tag and the `Exact`
/// handle apart, two): the discriminant of a named prefix, `29 +` the handle's index for `Exact`. Rows of one named
/// prefix are written column-wise (its tag run), rows with an `Exact` one item by item.
#[cfg(feature = "shrink_wrap")]
mod series_store {
    use super::Prefix;
    use crate::ExactScale;
    use core::mem::MaybeUninit;
    use core::ops::Range;
    use shrink_wrap::Error;
    use shrink_wrap::series::{
        BitSink, Level, SeriesCells, SeriesPolicy, SeriesStore, write_enum_tags,
    };

    const EXACT_CODE: u8 = Prefix::ALL.len() as u8;

    impl Prefix {
        #[inline(always)]
        fn code(&self) -> u8 {
            match self {
                Prefix::Exact(s) => EXACT_CODE + s.index(),
                p => p.discriminant() as u8,
            }
        }

        #[inline(always)]
        fn from_code(code: u8) -> Prefix {
            match Prefix::ALL.get(code as usize) {
                Some(p) => *p,
                // only codes of prefixes are stored, an empty slot is 0 (`Unit`)
                None => {
                    ExactScale::from_index(code - EXACT_CODE).map_or(Prefix::Unit, Prefix::Exact)
                }
            }
        }
    }

    impl SeriesStore for Prefix {
        type Store<const N: usize> = <u8 as SeriesStore>::Store<N>;

        fn empty_store<const N: usize>() -> Self::Store<N> {
            u8::empty_store::<N>()
        }

        fn init_store<const N: usize>(
            slot: &mut MaybeUninit<Self::Store<N>>,
        ) -> &mut Self::Store<N> {
            u8::init_store::<N>(slot)
        }

        #[inline(always)]
        fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
            u8::set::<N>(store, i, &item.code());
        }

        #[inline(always)]
        fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
            Prefix::from_code(u8::get::<N>(store, i))
        }

        fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
            let first = u8::get::<N>(store, rows.start);
            first < EXACT_CODE && rows.into_iter().all(|i| u8::get::<N>(store, i) == first)
        }

        fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
            store: &Self::Store<N>,
            rows: Range<usize>,
            sink: &mut S,
            _policy: &P,
            _level: Level,
        ) -> Result<(), Error> {
            // a named prefix is a unit variant: its tag run is all there is
            let tag = u8::get::<N>(store, rows.start) as u32;
            write_enum_tags(sink, core::iter::repeat_n(tag, rows.len()))
        }
    }

    // SAFETY: the cells of `u8`, read back through `from_code`, which maps any byte to a prefix
    unsafe impl SeriesCells for Prefix {
        type Cells<const R: usize> = <u8 as SeriesCells>::Cells<R>;

        #[inline(always)]
        fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
            u8::put::<R>(cells, i, &item.code());
        }

        fn copy_rows<const R: usize, const N: usize>(
            cells: &Self::Cells<R>,
            from: usize,
            store: &mut Self::Store<N>,
            to: usize,
            n: usize,
        ) {
            u8::copy_rows::<R, N>(cells, from, store, to, n);
        }
    }
}
