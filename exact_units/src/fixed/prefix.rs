//! Scale markers: zero-size types that fix the scale of a [Fixed](crate::Fixed) in its type (`Fixed<Second, Micro,
//! u32>`), never sent themselves. The decimal SI prefixes `Quecto` to `Quetta` with [One] for none, the binary `Kibi`
//! to `Tebi` (2¹⁰ to 2⁴⁰), which exist only as markers (the run-time [Prefix] of a `ww_si::Quantity` is
//! decimal), and exact factors without a name: [Pow10], [Pow2], [Rational] and the full-scale fractions [FullScale]
//! and [FullScaleMax]. Every marker's scale is a [Magnitude], so markers of the same factor are the same scale
//! however spelled (`Milli`, `Pow10<-3>`, `Rational<1, 1000>`): conversions between them are free.
//!
//! The named markers are declared with [scale!](crate::scale), like the unit markers with [unit!](crate::unit): the
//! snapshot of this crate holds them with their docs and factors (INT-13).

use crate::{Magnitude, Prefix};

/// A scale fixed at compile time, the `P` of [Fixed](crate::Fixed): a zero-size marker, never sent. A prefix
/// ([Micro], [Kibi]) or a factor without a name ([Pow10], [Rational]). The markers of this crate are in this module; an
/// API crate declares its own with [scale!](crate::scale) (a hand-written impl compiles, but WireWeaver refuses an API
/// that uses it, as for [Unit](crate::Unit)).
pub trait FixedScale: Copy + Default + 'static {
    /// `10⁻⁶` for [Micro], `2¹⁰` for [Kibi], `1/65535` for `Rational<1, 65535>`.
    const SCALE: Magnitude;
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
/// A step that has a prefix reads better with it (`Milli`); `Pow10<-3>` is the same scale. `|E|` up to 63.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pow10<const E: i8>;

/// `2^E`: binary fractions and multiples, the Q formats (`Pow2<-15>` in [Q15](crate::Q15)), a binary angle
/// (`Pow2<-16>` of a turn, [Bam16](crate::Bam16)). `|E|` up to 63.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pow2<const E: i8>;

/// `NUM / DEN`, an exact scale that isn't a power of ten: a 16-bit duty where 65535 is 100 % is
/// `Fixed<Ratio, Rational<1, 65535>, u16>` ([Duty16](crate::Duty16)), a gyro at 70 mdps per LSB
/// `Fixed<DegreePerSecond, Rational<70, 1000>, i16>`. Not reduced in the type, the [Magnitude] is (factored into
/// primes at compile time; a prime factor above `u32::MAX` doesn't compile).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rational<const NUM: u64, const DEN: u64>;

/// `V / 2^BITS`: one step of a `BITS`-bit converter whose full scale `2^BITS` is `V` (a scale marker), the common ADC
/// convention (LSB = Vref / 2ᴺ). A 12-bit ADC at 3.3 V is `Fixed<Volt, FullScale<Rational<33, 10>, 12>, u16>`, a
/// step of 3.3/4096 V.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FullScale<V, const BITS: u8>(core::marker::PhantomData<V>);

/// `V / (2^BITS - 1)`: one step of a `BITS`-bit value whose largest code `2^BITS - 1` is `V`, the convention of a PWM
/// duty or a DAC that reaches its reference: [Duty16](crate::Duty16) is `FullScaleMax<One, 16>`'s scale, 1/65535.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FullScaleMax<V, const BITS: u8>(core::marker::PhantomData<V>);

impl<const E: i8> FixedScale for Pow10<E> {
    const SCALE: Magnitude = Magnitude::pow10(E);
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "Pow10";
    const RUNTIME: Option<Prefix> = None;
}

impl<const E: i8> FixedScale for Pow2<E> {
    const SCALE: Magnitude = Magnitude::pow2(E);
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "Pow2";
    const RUNTIME: Option<Prefix> = None;
}

impl<V: FixedScale, const BITS: u8> FixedScale for FullScale<V, BITS> {
    const SCALE: Magnitude = V::SCALE.div(Magnitude::pow2(bits(BITS)));
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "FullScale";
    const RUNTIME: Option<Prefix> = None;
}

impl<V: FixedScale, const BITS: u8> FixedScale for FullScaleMax<V, BITS> {
    const SCALE: Magnitude = V::SCALE.div(Magnitude::ratio(max_code(BITS), 1));
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "FullScaleMax";
    const RUNTIME: Option<Prefix> = None;
}

/// `BITS` of a full-scale marker, a compile error above 63.
const fn bits(b: u8) -> i8 {
    assert!(b >= 1 && b <= 63, "a full scale of 1 to 63 bits");
    b as i8
}

/// `2^BITS - 1`.
const fn max_code(b: u8) -> u64 {
    (1u64 << bits(b)) - 1
}

impl<const NUM: u64, const DEN: u64> FixedScale for Rational<NUM, DEN> {
    const SCALE: Magnitude = Magnitude::ratio(NUM, DEN);
    const SYMBOL: &'static str = "";
    const NAME: &'static str = "Rational";
    const RUNTIME: Option<Prefix> = None;
}

crate::scale! {
    /// Quecto, `q`, 10⁻³⁰.
    pub struct Quecto = "q", Pow10(-30), prefix Quecto;

    /// Ronto, `r`, 10⁻²⁷.
    pub struct Ronto = "r", Pow10(-27), prefix Ronto;

    /// Yocto, `y`, 10⁻²⁴.
    pub struct Yocto = "y", Pow10(-24), prefix Yocto;

    /// Zepto, `z`, 10⁻²¹.
    pub struct Zepto = "z", Pow10(-21), prefix Zepto;

    /// Atto, `a`, 10⁻¹⁸.
    pub struct Atto = "a", Pow10(-18), prefix Atto;

    /// Femto, `f`, 10⁻¹⁵.
    pub struct Femto = "f", Pow10(-15), prefix Femto;

    /// Pico, `p`, 10⁻¹².
    pub struct Pico = "p", Pow10(-12), prefix Pico;

    /// Nano, `n`, 10⁻⁹.
    pub struct Nano = "n", Pow10(-9), prefix Nano;

    /// Micro, `µ`, 10⁻⁶.
    pub struct Micro = "µ", Pow10(-6), prefix Micro;

    /// Milli, `m`, 10⁻³.
    pub struct Milli = "m", Pow10(-3), prefix Milli;

    /// Centi, `c`, 10⁻².
    pub struct Centi = "c", Pow10(-2), prefix Centi;

    /// Deci, `d`, 10⁻¹.
    pub struct Deci = "d", Pow10(-1), prefix Deci;

    /// No prefix, 10⁰.
    pub struct One = "", Pow10(0), prefix Unit;

    /// Deca, `da`, 10¹.
    pub struct Deca = "da", Pow10(1), prefix Deca;

    /// Hecto, `h`, 10².
    pub struct Hecto = "h", Pow10(2), prefix Hecto;

    /// Kilo, `k`, 10³.
    pub struct Kilo = "k", Pow10(3), prefix Kilo;

    /// Mega, `M`, 10⁶.
    pub struct Mega = "M", Pow10(6), prefix Mega;

    /// Giga, `G`, 10⁹.
    pub struct Giga = "G", Pow10(9), prefix Giga;

    /// Tera, `T`, 10¹².
    pub struct Tera = "T", Pow10(12), prefix Tera;

    /// Peta, `P`, 10¹⁵.
    pub struct Peta = "P", Pow10(15), prefix Peta;

    /// Exa, `E`, 10¹⁸.
    pub struct Exa = "E", Pow10(18), prefix Exa;

    /// Zetta, `Z`, 10²¹.
    pub struct Zetta = "Z", Pow10(21), prefix Zetta;

    /// Yotta, `Y`, 10²⁴.
    pub struct Yotta = "Y", Pow10(24), prefix Yotta;

    /// Ronna, `R`, 10²⁷.
    pub struct Ronna = "R", Pow10(27), prefix Ronna;

    /// Quetta, `Q`, 10³⁰.
    pub struct Quetta = "Q", Pow10(30), prefix Quetta;

    /// kibi, `Ki`, 2¹⁰ (binary, only as a marker).
    pub struct Kibi = "Ki", Pow2(10);

    /// mebi, `Mi`, 2²⁰ (binary, only as a marker).
    pub struct Mebi = "Mi", Pow2(20);

    /// gibi, `Gi`, 2³⁰ (binary, only as a marker).
    pub struct Gibi = "Gi", Pow2(30);

    /// tebi, `Ti`, 2⁴⁰ (binary, only as a marker).
    pub struct Tebi = "Ti", Pow2(40);
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
