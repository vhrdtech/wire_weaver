//! Prefix markers: zero-size types that fix a prefix in a type (`Fixed<Second, Micro, u32>`), never sent themselves.
//! The decimal SI prefixes `Quecto` to `Quetta` with [One] for none, and the binary `Kibi` to `Tebi` (2¹⁰ to 2⁴⁰), which
//! exist only as markers: the run-time [Prefix] of a [Quantity](crate::Quantity) is decimal.
//!
//! The markers are literal structs, so the snapshot of this crate holds them with their docs, like the unit markers.

use crate::Prefix;

/// The scale of a prefix: `10^n` or `2^n`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PrefixScale {
    /// `10^n`: `Pow10(-6)` for [Micro].
    Pow10(i8),
    /// `2^n`: `Pow2(10)` for [Kibi].
    Pow2(u8),
}

impl PrefixScale {
    /// The powers of 10 and of 2.
    pub const fn exps(self) -> (i16, i16) {
        match self {
            PrefixScale::Pow10(e) => (e as i16, 0),
            PrefixScale::Pow2(e) => (0, e as i16),
        }
    }
}

/// A prefix fixed at compile time: a zero-size marker, never sent. The markers of this crate are in this module.
pub trait UnitPrefix: Copy + Default + 'static {
    /// `Pow10(-6)` for [Micro], `Pow2(10)` for [Kibi].
    const SCALE: PrefixScale;
    /// As written before a unit's symbol: `µ`, `Ki`, empty for [One].
    const SYMBOL: &'static str;
    /// The marker's name: `"Micro"`.
    const NAME: &'static str;
    /// The run-time prefix of the same scale, `None` for a binary one.
    const RUNTIME: Option<Prefix>;
}

/// A decimal prefix, one a [Quantity](crate::Quantity) can have too.
pub trait DecimalPrefix: UnitPrefix {
    /// The run-time prefix of the same scale.
    const PREFIX: Prefix;
}

/// A prefix that a [core::time::Duration] holds exactly: s, ds, cs, ms, µs, ns.
pub trait SubSecond: DecimalPrefix {
    /// How many of it make a second.
    const PER_SECOND: u64;
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
            impl UnitPrefix for $ty {
                const SCALE: PrefixScale = PrefixScale::Pow10($e);
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
            impl UnitPrefix for $ty {
                const SCALE: PrefixScale = PrefixScale::Pow2($e);
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
