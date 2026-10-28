//! Scale markers: zero-size types that fix the scale of a [Fixed](crate::Fixed) in its type (`Fixed<Second, Micro,
//! u32>`), never sent themselves. The decimal SI prefixes `Quecto` to `Quetta` with [One] for none, the binary `Kibi`
//! to `Tebi` (2¹⁰ to 2⁴⁰), which exist only as markers (the run-time [Prefix] of a `ww_si::Quantity` is
//! decimal), and exact factors without a name: [Pow10] and [Rational].
//!
//! The markers are literal structs, so the snapshot of this crate holds them with their docs, like the unit markers.

use crate::{ExactScale, Prefix};

/// A scale fixed at compile time, the `P` of [Fixed](crate::Fixed): a zero-size marker, never sent. A prefix
/// ([Micro], [Kibi]) or a factor without a name ([Pow10], [Rational]). The markers of this crate are in this module.
pub trait FixedScale: Copy + Default + 'static {
    /// `10⁻⁶` for [Micro], `2¹⁰` for [Kibi], `1/65535` for `Rational<1, 65535>`.
    const SCALE: ExactScale;
    /// As written before a unit's symbol: `µ`, `Ki`; empty for [One] and for a scale that isn't a prefix.
    const SYMBOL: &'static str;
    /// The marker's name: `"Micro"`, `"Pow10"`.
    const NAME: &'static str;
    /// The run-time prefix of the same scale, `None` for a binary prefix and other factors.
    const RUNTIME: Option<Prefix>;
}

/// A decimal prefix, one a `ww_si::Quantity` can have too.
pub trait DecimalPrefix: FixedScale {
    /// The run-time prefix of the same scale.
    const PREFIX: Prefix;
}

/// A prefix that a [core::time::Duration] holds exactly: s, ds, cs, ms, µs, ns.
pub trait SubSecond: DecimalPrefix {
    /// How many of it make a second.
    const PER_SECOND: u64;
}

/// `10^E` for a decimal step without a prefix of its own: tenths of a millisecond are `Fixed<Second, Pow10<-4>, u32>`.
/// A step that has a prefix is written with it (`Milli`, not `Pow10<-3>`), so one scale has one type.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pow10<const E: i8>;

/// `NUM / DEN`, an exact scale that isn't a power of ten: a 16-bit duty where 65535 is 100 % is
/// `Fixed<Ratio, Rational<1, 65535>, u16>` ([Duty16](crate::Duty16)). Not reduced in the type, the [ExactScale] is.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rational<const NUM: u32, const DEN: u32>;

impl<const E: i8> FixedScale for Pow10<E> {
    const SCALE: ExactScale = ExactScale::pow10(E);
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "Pow10";
    const RUNTIME: Option<Prefix> = None;
}

impl<const NUM: u32, const DEN: u32> FixedScale for Rational<NUM, DEN> {
    const SCALE: ExactScale = ExactScale::rational(NUM, DEN);
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "Rational";
    const RUNTIME: Option<Prefix> = None;
}

/// Quecto, `q`, 10⁻³⁰.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Quecto;

/// Ronto, `r`, 10⁻²⁷.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ronto;

/// Yocto, `y`, 10⁻²⁴.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Yocto;

/// Zepto, `z`, 10⁻²¹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Zepto;

/// Atto, `a`, 10⁻¹⁸.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Atto;

/// Femto, `f`, 10⁻¹⁵.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Femto;

/// Pico, `p`, 10⁻¹².
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pico;

/// Nano, `n`, 10⁻⁹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Nano;

/// Micro, `µ`, 10⁻⁶.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Micro;

/// Milli, `m`, 10⁻³.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Milli;

/// Centi, `c`, 10⁻².
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Centi;

/// Deci, `d`, 10⁻¹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Deci;

/// No prefix, 10⁰.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct One;

/// Deca, `da`, 10¹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Deca;

/// Hecto, `h`, 10².
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hecto;

/// Kilo, `k`, 10³.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kilo;

/// Mega, `M`, 10⁶.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mega;

/// Giga, `G`, 10⁹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Giga;

/// Tera, `T`, 10¹².
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tera;

/// Peta, `P`, 10¹⁵.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Peta;

/// Exa, `E`, 10¹⁸.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Exa;

/// Zetta, `Z`, 10²¹.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Zetta;

/// Yotta, `Y`, 10²⁴.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Yotta;

/// Ronna, `R`, 10²⁷.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ronna;

/// Quetta, `Q`, 10³⁰.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Quetta;

/// kibi, `Ki`, 2¹⁰ (binary, only as a marker).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kibi;

/// mebi, `Mi`, 2²⁰ (binary, only as a marker).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mebi;

/// gibi, `Gi`, 2³⁰ (binary, only as a marker).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Gibi;

/// tebi, `Ti`, 2⁴⁰ (binary, only as a marker).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tebi;

macro_rules! decimal {
    ($($ty:ident $prefix:ident $symbol:literal $e:literal;)*) => {
        $(
            impl FixedScale for $ty {
                const SCALE: ExactScale = ExactScale::pow10($e);
                const SYMBOL: &'static str = $symbol;
                const NAME: &'static str = stringify!($ty);
                const RUNTIME: Option<Prefix> = Some(Prefix::$prefix);
            }
            impl DecimalPrefix for $ty {
                const PREFIX: Prefix = Prefix::$prefix;
            }
        )*
    };
}

macro_rules! binary {
    ($($ty:ident $symbol:literal $e:literal;)*) => {
        $(
            impl FixedScale for $ty {
                const SCALE: ExactScale = ExactScale::pow2($e);
                const SYMBOL: &'static str = $symbol;
                const NAME: &'static str = stringify!($ty);
                const RUNTIME: Option<Prefix> = None;
            }
        )*
    };
}

decimal! {
    Quecto Quecto "q" -30;
    Ronto Ronto "r" -27;
    Yocto Yocto "y" -24;
    Zepto Zepto "z" -21;
    Atto Atto "a" -18;
    Femto Femto "f" -15;
    Pico Pico "p" -12;
    Nano Nano "n" -9;
    Micro Micro "µ" -6;
    Milli Milli "m" -3;
    Centi Centi "c" -2;
    Deci Deci "d" -1;
    One Unit "" 0;
    Deca Deca "da" 1;
    Hecto Hecto "h" 2;
    Kilo Kilo "k" 3;
    Mega Mega "M" 6;
    Giga Giga "G" 9;
    Tera Tera "T" 12;
    Peta Peta "P" 15;
    Exa Exa "E" 18;
    Zetta Zetta "Z" 21;
    Yotta Yotta "Y" 24;
    Ronna Ronna "R" 27;
    Quetta Quetta "Q" 30;
}

binary! {
    Kibi "Ki" 10;
    Mebi "Mi" 20;
    Gibi "Gi" 30;
    Tebi "Ti" 40;
}

impl SubSecond for One {
    const PER_SECOND: u64 = 1;
}
impl SubSecond for Deci {
    const PER_SECOND: u64 = 10;
}
impl SubSecond for Centi {
    const PER_SECOND: u64 = 100;
}
impl SubSecond for Milli {
    const PER_SECOND: u64 = 1_000;
}
impl SubSecond for Micro {
    const PER_SECOND: u64 = 1_000_000;
}
impl SubSecond for Nano {
    const PER_SECOND: u64 = 1_000_000_000;
}
