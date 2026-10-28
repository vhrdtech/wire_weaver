//! Unit markers: zero-size types that name a unit in a type (`Fixed<units::Volt, Milli, u16>`), never sent themselves.
//! `ww_si` has an alias of the run-time quantity of each under the same name (`ww_si::Volt`).
//!
//! Compound units outside the list are [Per] and [Times] of markers: `Fixed<Per<Torque, AngularVelocity>, One, f32>` is in
//! N·m/(rad/s). [by_name] and [ALL] give the [UnitExpr] of every named marker of this crate, for dynamic clients that
//! see a unit only by its name in introspection data.
//!
//! The markers are declared with [unit!](crate::unit), one line each with its symbol, dimension and scale: the
//! compiler gets their [Unit] impls from it, and the snapshot of this crate (WireWeaver's walker reads the same
//! lines) their docs and definitions, so clients know them without a table (INT-13).

#[cfg(feature = "shrink_wrap")]
use crate::SIExpr;
#[allow(unused_imports)] // `Scale` with no quantity group enabled
use crate::unit::{Dim, Scale, Unit, UnitExpr};
use core::marker::PhantomData;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::RefBox;

#[cfg(feature = "customary")]
mod customary;
#[cfg(feature = "customary")]
pub use customary::*;

crate::unit! {
    @table
    /// Every named unit of this crate that is enabled, in the order of the docs.
    pub static ALL;

    // Time

    /// Time in seconds, s.
    ///
    /// As a device timestamp it is the device's time since its clock started, see `ww_si::Second`.
    #[cfg(feature = "time")]
    pub struct Second = "s", [s 1];

    /// Frequency in hertz, Hz (s⁻¹).
    #[cfg(feature = "time")]
    pub struct Hertz = "Hz", [s -1];

    /// Frequency drift in hertz per second, Hz/s (s⁻²).
    #[cfg(feature = "time")]
    pub struct HertzPerSecond = "Hz/s", [s -2];

    /// Symbol rate in baud, Bd (s⁻¹): one symbol per second, for a UART one bit per second on the line. A frequency of
    /// its own kind, so it is not mixed up with a clock in [Hertz].
    #[cfg(feature = "time")]
    pub struct Baud = "Bd", [s -1];

    // Electrical

    /// Electric current in amperes, A.
    #[cfg(feature = "electrical")]
    pub struct Ampere = "A", [a 1];

    /// Voltage, electric potential or electromotive force in volts, V (kg·m²·s⁻³·A⁻¹, W/A).
    #[cfg(feature = "electrical")]
    pub struct Volt = "V", [kg 1, m 2, s -3, a -1];

    /// Resistance, reactance or impedance in ohms, Ω (kg·m²·s⁻³·A⁻², V/A).
    #[cfg(feature = "electrical")]
    pub struct Ohm = "Ω", [kg 1, m 2, s -3, a -2];

    /// Capacitance in farads, F (kg⁻¹·m⁻²·s⁴·A², C/V).
    #[cfg(feature = "electrical")]
    pub struct Farad = "F", [kg -1, m -2, s 4, a 2];

    /// Admittance, conductance or susceptance in siemens, S (kg⁻¹·m⁻²·s³·A², A/V).
    #[cfg(feature = "electrical")]
    pub struct Siemens = "S", [kg -1, m -2, s 3, a 2];

    /// Inductance in henries, H (kg·m²·s⁻²·A⁻², Wb/A).
    #[cfg(feature = "electrical")]
    pub struct Henry = "H", [kg 1, m 2, s -2, a -2];

    /// Electric charge in coulombs, C (s·A).
    #[cfg(feature = "electrical")]
    pub struct Coulomb = "C", [s 1, a 1];

    /// Power in watts, W (kg·m²·s⁻³, J/s, V·A).
    #[cfg(feature = "electrical")]
    pub struct Watt = "W", [kg 1, m 2, s -3];

    /// Magnetic flux in webers, Wb (kg·m²·s⁻²·A⁻¹, V·s).
    #[cfg(feature = "electrical")]
    pub struct Weber = "Wb", [kg 1, m 2, s -2, a -1];

    /// Magnetic flux density in teslas, T (kg·s⁻²·A⁻¹, Wb/m²).
    #[cfg(feature = "electrical")]
    pub struct Tesla = "T", [kg 1, s -2, a -1];

    // Mechanical

    /// Length in meters, m.
    #[cfg(feature = "mechanical")]
    pub struct Meter = "m", [m 1];

    /// Mass in grams, g (10⁻³ kg): 5 kg is `Gram::kilo(5u8)`.
    #[cfg(feature = "mechanical")]
    pub struct Gram = "g", [kg 1], Pow10(-3);

    /// Force or weight in newtons, N (kg·m·s⁻²).
    #[cfg(feature = "mechanical")]
    pub struct Newton = "N", [kg 1, m 1, s -2];

    /// Pressure or stress in pascals, Pa (kg·m⁻¹·s⁻², N/m²).
    #[cfg(feature = "mechanical")]
    pub struct Pascal = "Pa", [kg 1, m -1, s -2];

    /// Energy, work or heat in joules, J (kg·m²·s⁻², N·m, W·s).
    #[cfg(feature = "mechanical")]
    pub struct Joule = "J", [kg 1, m 2, s -2];

    /// Torque or moment of force in newton-meters, N·m, counted as joules per radian (kg·m²·s⁻²·rad⁻¹): not the
    /// dimension of a [Joule], and a torque times an [AngularVelocity] is a [Watt].
    #[cfg(feature = "mechanical")]
    pub struct Torque = "N·m", [kg 1, m 2, s -2, rad -1];

    /// Volume in litres, L (10⁻³ m³).
    #[cfg(feature = "mechanical")]
    pub struct Litre = "L", [m 3], Pow10(-3);

    /// Volumetric flow in cubic meters per second, m³/s.
    #[cfg(feature = "mechanical")]
    pub struct VolumetricFlow = "m³/s", [m 3, s -1];

    /// Speed in meters per second, m/s.
    #[cfg(feature = "mechanical")]
    pub struct Speed = "m/s", [m 1, s -1];

    /// Acceleration in meters per second squared, m/s².
    #[cfg(feature = "mechanical")]
    pub struct Acceleration = "m/s²", [m 1, s -2];

    /// Jerk in meters per second cubed, m/s³.
    #[cfg(feature = "mechanical")]
    pub struct Jerk = "m/s³", [m 1, s -3];

    /// Snap or jounce in meters per second to the fourth, m/s⁴.
    #[cfg(feature = "mechanical")]
    pub struct Snap = "m/s⁴", [m 1, s -4];

    /// Yank, the rate of change of force, in newtons per second, N/s (kg·m·s⁻³).
    #[cfg(feature = "mechanical")]
    pub struct Yank = "N/s", [kg 1, m 1, s -3];

    // Angle

    /// Angle in radians, rad: a base dimension of its own here, so an angle doesn't mix with a [Ratio].
    #[cfg(feature = "angle")]
    pub struct Radian = "rad", [rad 1];

    /// Angle in degrees of arc, ° (π/180 rad, exactly: [Scale::Exact]).
    #[cfg(feature = "angle")]
    pub struct Degree = "°", [rad 1], Exact(1 / 180, pi 1);

    /// Angle in turns (revolutions), rev (2π rad, exactly). A binary angle where 65536 is a full turn is
    /// `Fixed<Turn, Rational<1, 65536>, u16>`.
    #[cfg(feature = "angle")]
    pub struct Turn = "rev", [rad 1], Exact(2 / 1, pi 1);

    /// Solid angle in steradians, sr (rad²).
    #[cfg(feature = "angle")]
    pub struct Steradian = "sr", [rad 2];

    /// Angular velocity in radians per second, rad/s (rad·s⁻¹): not a [Hertz], a turn is 2π rad.
    #[cfg(feature = "angle")]
    pub struct AngularVelocity = "rad/s", [rad 1, s -1];

    /// Angular acceleration in radians per second squared, rad/s² (rad·s⁻²).
    #[cfg(feature = "angle")]
    pub struct AngularAcceleration = "rad/s²", [rad 1, s -2];

    // Thermal

    /// Thermodynamic temperature in kelvins, K.
    #[cfg(feature = "thermal")]
    pub struct Kelvin = "K", [k 1];

    /// Temperature in degrees Celsius, °C (K − 273.15). No products: a product with an offset scale means nothing.
    #[cfg(feature = "thermal")]
    pub struct DegreeCelsius = "°C", [k 1], Affine { factor: 1.0, offset: 273.15 };

    // Chemical

    /// Amount of substance in moles, mol.
    #[cfg(feature = "chemical")]
    pub struct Mole = "mol", [mol 1];

    /// Catalytic activity in katals, kat (mol·s⁻¹).
    #[cfg(feature = "chemical")]
    pub struct Katal = "kat", [mol 1, s -1];

    // Photometric

    /// Luminous intensity in candelas, cd.
    #[cfg(feature = "photometric")]
    pub struct Candela = "cd", [cd 1];

    /// Luminous flux in lumens, lm (cd·sr, cd·rad²).
    #[cfg(feature = "photometric")]
    pub struct Lumen = "lm", [cd 1, rad 2];

    /// Illuminance in lux, lx (cd·rad²·m⁻², lm/m²).
    #[cfg(feature = "photometric")]
    pub struct Lux = "lx", [cd 1, rad 2, m -2];

    // Radiation

    /// Radioactivity in becquerels, Bq (s⁻¹).
    #[cfg(feature = "radiation")]
    pub struct Becquerel = "Bq", [s -1];

    /// Absorbed dose in grays, Gy (m²·s⁻², J/kg).
    #[cfg(feature = "radiation")]
    pub struct Gray = "Gy", [m 2, s -2];

    /// Equivalent dose in sieverts, Sv (m²·s⁻², J/kg).
    #[cfg(feature = "radiation")]
    pub struct Sievert = "Sv", [m 2, s -2];

    /// Energy in electron-volts, eV (1.602176634·10⁻¹⁹ J).
    #[cfg(feature = "radiation")]
    pub struct ElectronVolt = "eV", [kg 1, m 2, s -2], Exact(1602176634 / 1, pow10 -28);

    // Dimensionless

    /// A fraction: a duty cycle, a probability, a scale factor, an efficiency (dimensionless, 1). 0.5 is half, not a
    /// level in decibels. Written with `%`, `‰`, `ppm`, `ppb` for `Centi`, `Milli`, `Micro`, `Nano`. Any quantity times a ratio keeps its
    /// unit (48 V · 0.5 = 24 V).
    #[cfg(feature = "dimensionless")]
    pub struct Ratio = "", [];

    // Information

    /// Information in bits, bit: the base of information, so a size never mixes with a [Ratio] or a count of things.
    #[cfg(feature = "information")]
    pub struct Bit = "bit", [bit 1];

    /// Information in bytes, B (8 [Bit]): a data size, a buffer's capacity, an offset. A rate is `Per<Byte, Second>`.
    #[cfg(feature = "information")]
    pub struct Byte = "B", [bit 1], Exact(8 / 1);

    /// Information in 32-bit words, word (4 [Byte]): flash and RAM sizes of a 32-bit MCU.
    #[cfg(feature = "information")]
    pub struct Word = "word", [bit 1], Exact(32 / 1);

    // Counting: each marker is a base dimension of its own (a counted base, named by its crate and marker name), so
    // samples don't add to bytes, and become bytes only through a `Per<Byte, Sample>`. An API that counts something else
    // declares its own marker with `exact_units::counted!(Name)`, see [Unit].

    /// A count of things, item: a queue's length, a number of filters, channels, presses, repeats, steps.
    #[cfg(feature = "counting")]
    pub struct Item = "item", counted;

    /// A count of samples, sample: an ADC's, a logic analyzer's; a sample rate is `Per<Sample, Second>`.
    #[cfg(feature = "counting")]
    pub struct Sample = "sample", counted;

    /// A count of frames, frame (a bus's, a video's).
    #[cfg(feature = "counting")]
    pub struct Frame = "frame", counted;

    /// A count of packets, packet.
    #[cfg(feature = "counting")]
    pub struct Packet = "packet", counted;

    /// A count of cycles, cycle: clock cycles, time quanta; time only with a clock (`Per<Cycle, Second>`).
    #[cfg(feature = "counting")]
    pub struct Cycle = "cycle", counted;

    /// A count of blocks, block: of a block device, whose size differs per device (bytes through `Per<Byte, Block>`).
    #[cfg(feature = "counting")]
    pub struct Block = "block", counted;

    /// A count of sectors, sector: of a flash or a disk (bytes through `Per<Byte, Sector>`).
    #[cfg(feature = "counting")]
    pub struct Sector = "sector", counted;

    /// A count of chunks, chunk: of a transfer or a file format (bytes through `Per<Byte, Chunk>`).
    #[cfg(feature = "counting")]
    pub struct Chunk = "chunk", counted;

    /// A count of characters, char: of a UART line (an idle time in characters).
    #[cfg(feature = "counting")]
    pub struct Char = "char", counted;

    /// A count of raw codes of a converter, code: an ADC's or DAC's output before scaling (LSB).
    #[cfg(feature = "counting")]
    pub struct Code = "code", counted;
}

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

/// `√U`, the square root of a unit: an amplitude spectral density in V/√Hz is `Per<Volt, Sqrt<Hertz>>`. Its
/// dimension has the root flag ([Dim::sqrt](crate::Dim::sqrt)): the exponents of V²/Hz, so the algebra stays integer.
/// `Sqrt<Sqrt<U>>` doesn't compile (a fourth root isn't a [Dim]).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sqrt<U> {
    _u: PhantomData<U>,
}

impl<U: Unit> Unit for Sqrt<U> {
    const DIM: Dim<'static> = U::DIM.sqrt();
    const EXPR: &'static UnitExpr = &UnitExpr::Sqrt(U::EXPR);
    #[cfg(feature = "shrink_wrap")]
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Sqrt(RefBox::Ref { value: U::SI_EXPR });
}

impl<A: Unit, B: Unit> Unit for Per<A, B> {
    const DIM: Dim<'static> = A::DIM.div(B::DIM);
    const EXPR: &'static UnitExpr = &UnitExpr::Div(A::EXPR, B::EXPR);
    #[cfg(feature = "shrink_wrap")]
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Div(
        RefBox::Ref { value: A::SI_EXPR },
        RefBox::Ref { value: B::SI_EXPR },
    );
}

impl<A: Unit, B: Unit> Unit for Times<A, B> {
    const DIM: Dim<'static> = A::DIM.mul(B::DIM);
    const EXPR: &'static UnitExpr = &UnitExpr::Mul(A::EXPR, B::EXPR);
    #[cfg(feature = "shrink_wrap")]
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Mul(
        RefBox::Ref { value: A::SI_EXPR },
        RefBox::Ref { value: B::SI_EXPR },
    );
}

/// The named unit of this crate called `name` (the marker's name, `"Volt"`), if its feature is enabled: what a
/// dynamic client needs to show a `Quantity<Volt>` it only knows from introspection data.
pub fn by_name(name: &str) -> Option<&'static UnitExpr> {
    let all = ALL.iter();
    #[cfg(feature = "customary")]
    let all = all.chain(CUSTOMARY);
    all.copied()
        .find(|expr| matches!(expr, UnitExpr::Named { name: n, .. } if *n == name))
}

/// Cycles per second are hertz: the counted [Cycle] and the hertz of a clock are different dimensions (Hz is s⁻¹),
/// so going from one to the other is this named step, never an implicit conversion.
#[cfg(all(feature = "counting", feature = "time"))]
impl<P, T: crate::Storage, B: crate::Bounds<T>> crate::Fixed<Per<Cycle, Second>, P, T, B> {
    /// The same number in Hz.
    pub fn to_hertz(self) -> crate::Fixed<Hertz, P, T, B> {
        crate::Fixed::new_unchecked(self.get())
    }
}
